//! Input by what it is for, not by which key it is.
//!
//! A game asks whether "jump" is pressed, and [`Actions`] knows that jump is the space bar
//! here: so the keys can be changed, by the player or for another keyboard, without the game
//! being touched. An action may have several keys and mouse buttons, any of which presses
//! it. An axis is a pair of them, one for each way, read as a number from -1 to 1.
//!
//! ```ignore
//! fn bind(mut actions: ResMut<Actions>) {
//!     actions.bind("jump", KeyCode::Space).bind("fire", MouseButton::Left);
//!     actions.bind_axis("strafe", KeyCode::KeyA, KeyCode::KeyD);
//! }
//!
//! fn play(actions: Res<Actions>) {
//!     if actions.just_pressed("jump") { /* … */ }
//!     let along = actions.value("strafe");
//! }
//! ```
//!
//! [`Actions::rebind`] hears the next key or button pressed and gives the action that
//! instead, which is all a settings screen needs. What is bound can be written out as text
//! and read back ([`Actions::to_text`], [`Actions::read_text`]): a line an action.

use std::collections::{BTreeMap, BTreeSet};

use glam::Vec2;

use crate::{
    ecs::{Res, ResMut},
    gamepad::{Gamepads, PadAxis, PadButton},
    input::{key_named, ButtonInput, KeyCode, MouseButton},
    touch::{Swipe, Touches},
};

/// Everything an action can be pressed by, as it is this frame.
#[derive(Clone, Copy)]
pub struct Inputs<'a> {
    pub keys: &'a ButtonInput<KeyCode>,
    pub buttons: &'a ButtonInput<MouseButton>,
    pub touches: Option<&'a Touches>,
    pub pads: Option<&'a Gamepads>,
}

/// What an action can be bound to: a key, a mouse button, a button of a controller, or
/// what a finger does on a screen. A swipe or a tap is over in the frame it happens: it presses its action for that
/// frame, as a key struck and let go at once would.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Binding {
    Key(KeyCode),
    Mouse(MouseButton),
    Swipe(Swipe),
    Tap,
    Pad(PadButton),
}

impl From<PadButton> for Binding {
    fn from(button: PadButton) -> Self {
        Self::Pad(button)
    }
}

impl From<Swipe> for Binding {
    fn from(way: Swipe) -> Self {
        Self::Swipe(way)
    }
}

impl From<KeyCode> for Binding {
    fn from(key: KeyCode) -> Self {
        Self::Key(key)
    }
}

impl From<MouseButton> for Binding {
    fn from(button: MouseButton) -> Self {
        Self::Mouse(button)
    }
}

impl Binding {
    /// What it is called where bindings are written down: a key as winit names it (`Space`,
    /// `KeyW`, `ArrowLeft`), a mouse button as `MouseLeft`, `MouseRight`, `MouseMiddle`.
    pub fn name(&self) -> String {
        match self {
            Self::Key(key) => format!("{key:?}"),
            Self::Mouse(MouseButton::Left) => "MouseLeft".into(),
            Self::Mouse(MouseButton::Right) => "MouseRight".into(),
            Self::Mouse(MouseButton::Middle) => "MouseMiddle".into(),
            Self::Mouse(MouseButton::Back) => "MouseBack".into(),
            Self::Mouse(MouseButton::Forward) => "MouseForward".into(),
            Self::Mouse(MouseButton::Other(number)) => format!("Mouse{number}"),
            Self::Swipe(way) => format!("Swipe{way:?}"),
            Self::Tap => "Tap".into(),
            Self::Pad(button) => button.name(),
        }
    }

    /// The binding with that name, if there is one.
    pub fn named(name: &str) -> Option<Self> {
        let name = name.trim();
        match name.to_ascii_lowercase().as_str() {
            "tap" => return Some(Self::Tap),
            "swipeup" => return Some(Self::Swipe(Swipe::Up)),
            "swipedown" => return Some(Self::Swipe(Swipe::Down)),
            "swipeleft" => return Some(Self::Swipe(Swipe::Left)),
            "swiperight" => return Some(Self::Swipe(Swipe::Right)),
            _ => {}
        }
        if let Some(button) = PadButton::named(name) {
            return Some(Self::Pad(button));
        }
        if let Some(button) = name.strip_prefix("Mouse").or_else(|| name.strip_prefix("mouse")) {
            return Some(Self::Mouse(match button.to_ascii_lowercase().as_str() {
                "left" => MouseButton::Left,
                "right" => MouseButton::Right,
                "middle" => MouseButton::Middle,
                "back" => MouseButton::Back,
                "forward" => MouseButton::Forward,
                number => MouseButton::Other(number.parse().ok()?),
            }));
        }
        key_named(name).map(Self::Key)
    }

    fn down(&self, from: &Inputs) -> bool {
        match self {
            Self::Key(key) => from.keys.pressed(*key),
            Self::Mouse(button) => from.buttons.pressed(*button),
            Self::Swipe(way) => from.touches.is_some_and(|touches| touches.swiped() == Some(*way)),
            Self::Tap => from.touches.is_some_and(|touches| touches.tapped().is_some()),
            Self::Pad(button) => from.pads.is_some_and(|pads| pads.pressed(*button)),
        }
    }
}

/// The actions a game has, what each is bound to, and which are pressed. A resource.
#[derive(Default)]
pub struct Actions {
    /// An action's bindings: any of them presses it.
    buttons: BTreeMap<String, Vec<Binding>>,
    /// An axis's pairs: the first of a pair pulls it toward -1, the second toward 1.
    axes: BTreeMap<String, Vec<(Binding, Binding)>>,
    /// An axis's sticks and triggers, which say where it is themselves.
    sticks: BTreeMap<String, Vec<PadAxis>>,
    down: BTreeSet<String>,
    went_down: BTreeSet<String>,
    went_up: BTreeSet<String>,
    values: BTreeMap<String, f32>,
    /// The action waiting to be given whatever is pressed next.
    hearing: Option<String>,
}

impl Actions {
    /// Binds an action to a key or a mouse button, besides whatever it is bound to already.
    pub fn bind(&mut self, action: &str, to: impl Into<Binding>) -> &mut Self {
        let (bindings, to) = (self.buttons.entry(action.to_owned()).or_default(), to.into());
        if !bindings.contains(&to) {
            bindings.push(to);
        }
        self
    }

    /// Binds an axis to a pair: `toward_less` pulls it to -1, `toward_more` to 1, and both
    /// or neither leave it at nought. An axis may have several pairs.
    pub fn bind_axis(&mut self, axis: &str, toward_less: impl Into<Binding>, toward_more: impl Into<Binding>) -> &mut Self {
        let (pairs, pair) = (self.axes.entry(axis.to_owned()).or_default(), (toward_less.into(), toward_more.into()));
        if !pairs.contains(&pair) {
            pairs.push(pair);
        }
        self
    }

    /// Binds an axis to a stick or a trigger of a controller, besides its pairs: the axis
    /// is where the stick is, past its dead zone.
    pub fn bind_stick(&mut self, axis: &str, stick: PadAxis) -> &mut Self {
        let sticks = self.sticks.entry(axis.to_owned()).or_default();
        if !sticks.contains(&stick) {
            sticks.push(stick);
        }
        self
    }

    /// Takes every binding from an action or an axis.
    pub fn unbind(&mut self, name: &str) -> &mut Self {
        self.buttons.remove(name);
        self.axes.remove(name);
        self.sticks.remove(name);
        self
    }

    /// What an action is bound to.
    pub fn bindings(&self, action: &str) -> &[Binding] {
        self.buttons.get(action).map_or(&[], Vec::as_slice)
    }

    /// The pairs an axis is bound to.
    pub fn axis_bindings(&self, axis: &str) -> &[(Binding, Binding)] {
        self.axes.get(axis).map_or(&[], Vec::as_slice)
    }

    /// The sticks and triggers an axis is bound to.
    pub fn stick_bindings(&self, axis: &str) -> &[PadAxis] {
        self.sticks.get(axis).map_or(&[], Vec::as_slice)
    }

    /// The names of the axes: those with pairs and those with sticks.
    fn axis_names(&self) -> BTreeSet<&str> {
        self.axes.keys().chain(self.sticks.keys()).map(String::as_str).collect()
    }

    /// The actions there are, and the axes, in the order of their names.
    pub fn names(&self) -> (Vec<&str>, Vec<&str>) {
        (self.buttons.keys().map(String::as_str).collect(), self.axis_names().into_iter().collect())
    }

    /// Whether the action is held down.
    pub fn pressed(&self, action: &str) -> bool {
        self.down.contains(action)
    }

    /// Whether it was pressed this frame.
    pub fn just_pressed(&self, action: &str) -> bool {
        self.went_down.contains(action)
    }

    /// Whether it was let go this frame.
    pub fn just_released(&self, action: &str) -> bool {
        self.went_up.contains(action)
    }

    /// Where an axis is, from -1 to 1.
    pub fn value(&self, axis: &str) -> f32 {
        self.values.get(axis).copied().unwrap_or(0.0)
    }

    /// Two axes as a direction, no longer than one: so that going both ways at once is no
    /// faster than going one.
    pub fn direction(&self, across: &str, along: &str) -> Vec2 {
        Vec2::new(self.value(across), self.value(along)).clamp_length_max(1.0)
    }

    /// Has the action given whatever key or mouse button is pressed next, in place of what
    /// it is bound to now. Until then no action is pressed, so that the key chosen does not
    /// also play the game.
    pub fn rebind(&mut self, action: &str) {
        self.hearing = Some(action.to_owned());
    }

    /// The action waiting to be bound, if one is.
    pub fn rebinding(&self) -> Option<&str> {
        self.hearing.as_deref()
    }

    /// Leaves the action waiting to be bound as it was.
    pub fn cancel_rebind(&mut self) {
        self.hearing = None;
    }

    /// Brings what is pressed up to date with the keys and buttons, once a frame.
    pub fn update(&mut self, keys: &ButtonInput<KeyCode>, buttons: &ButtonInput<MouseButton>) {
        self.update_from(&Inputs { keys, buttons, touches: None, pads: None });
    }

    /// The same, with fingers and controllers as well: everything there is to be pressed by.
    pub fn update_from(&mut self, from: &Inputs) {
        let (keys, buttons) = (from.keys, from.buttons);
        self.went_down.clear();
        self.went_up.clear();
        if let Some(action) = self.hearing.clone() {
            // The first thing newly pressed is the action's from now on.
            let pressed = keys.get_just_pressed().next().map(|key| Binding::Key(*key)).or_else(|| buttons.get_just_pressed().next().map(|button| Binding::Mouse(*button))).or_else(|| from.pads.and_then(|pads| pads.get_just_pressed().next()).map(Binding::Pad));
            if let Some(binding) = pressed {
                self.buttons.insert(action, vec![binding]);
                self.hearing = None;
            }
            // Nothing is pressed while a key is being chosen, nor in the frame it is.
            self.went_up = std::mem::take(&mut self.down);
            self.values.clear();
            return;
        }
        for (action, bindings) in &self.buttons {
            let now = bindings.iter().any(|binding| binding.down(from));
            match (now, self.down.contains(action)) {
                (true, false) => {
                    self.down.insert(action.clone());
                    self.went_down.insert(action.clone());
                }
                (false, true) => {
                    self.down.remove(action);
                    self.went_up.insert(action.clone());
                }
                _ => {}
            }
        }
        // An action unbound while it was held is let go.
        let gone: Vec<String> = self.down.iter().filter(|action| !self.buttons.contains_key(*action)).cloned().collect();
        for action in gone {
            self.down.remove(&action);
            self.went_up.insert(action);
        }
        self.values.clear();
        for (axis, pairs) in &self.axes {
            let pull: f32 = pairs.iter().map(|(less, more)| more.down(from) as i32 as f32 - less.down(from) as i32 as f32).sum();
            self.values.insert(axis.clone(), pull);
        }
        if let Some(pads) = from.pads {
            for (axis, sticks) in &self.sticks {
                let pull: f32 = sticks.iter().map(|stick| pads.axis(*stick)).sum();
                *self.values.entry(axis.clone()).or_default() += pull;
            }
        }
        for value in self.values.values_mut() {
            *value = value.clamp(-1.0, 1.0);
        }
    }

    /// What is bound, as text: a line for each action, `jump = Space, MouseLeft`, and for
    /// each axis, its pairs with a stroke between the two ways, `strafe = KeyA/KeyD`.
    pub fn to_text(&self) -> String {
        let mut text = String::new();
        for (action, bindings) in &self.buttons {
            let names: Vec<String> = bindings.iter().map(Binding::name).collect();
            text += &format!("{action} = {}\n", names.join(", "));
        }
        for axis in self.axis_names() {
            let pairs = self.axis_bindings(axis).iter().map(|(less, more)| format!("{}/{}", less.name(), more.name()));
            let names: Vec<String> = pairs.chain(self.stick_bindings(axis).iter().map(PadAxis::name)).collect();
            text += &format!("{axis} = {}\n", names.join(", "));
        }
        text
    }

    /// Takes the bindings written in `text`, as [`to_text`](Self::to_text) writes them, for
    /// the actions and axes it names; others are left as they are. A line that cannot be
    /// read is told of, with its number, and nothing is changed.
    pub fn read_text(&mut self, text: &str) -> Result<(), String> {
        let (mut buttons, mut axes) = (Vec::new(), Vec::new());
        for (number, line) in text.lines().enumerate() {
            let line = line.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            let wrong = |what: &str| format!("line {}: {what}", number + 1);
            let (name, bound) = line.split_once('=').ok_or_else(|| wrong("no `=`"))?;
            let name = name.trim();
            if name.is_empty() {
                return Err(wrong("no name before `=`"));
            }
            let known = |part: &str| Binding::named(part).ok_or_else(|| wrong(&format!("`{}` is not a key or a mouse button", part.trim())));
            let parts: Vec<&str> = bound.split(',').map(str::trim).filter(|part| !part.is_empty()).collect();
            if parts.iter().any(|part| part.contains('/') || PadAxis::named(part).is_some()) {
                let (mut pairs, mut sticks) = (Vec::new(), Vec::new());
                for part in parts {
                    if let Some(stick) = PadAxis::named(part) {
                        sticks.push(stick);
                        continue;
                    }
                    let (less, more) = part.split_once('/').ok_or_else(|| wrong("an axis is pairs, each `Less/More`, and sticks"))?;
                    pairs.push((known(less)?, known(more)?));
                }
                axes.push((name.to_owned(), pairs, sticks));
            } else {
                buttons.push((name.to_owned(), parts.into_iter().map(known).collect::<Result<Vec<_>, _>>()?));
            }
        }
        for (name, bindings) in buttons {
            self.axes.remove(&name);
            self.sticks.remove(&name);
            self.buttons.insert(name, bindings);
        }
        for (name, pairs, sticks) in axes {
            self.buttons.remove(&name);
            // An axis with only sticks has no pairs, and one with only pairs no sticks.
            self.axes.remove(&name);
            self.sticks.remove(&name);
            if !pairs.is_empty() {
                self.axes.insert(name.clone(), pairs);
            }
            if !sticks.is_empty() {
                self.sticks.insert(name, sticks);
            }
        }
        Ok(())
    }

    /// Writes what is bound to a file, for the player's choices to be there next time.
    pub fn save(&self, path: impl AsRef<std::path::Path>) -> std::io::Result<()> {
        std::fs::write(path, self.to_text())
    }

    /// Reads bindings from a file written by [`save`](Self::save). A file that is not there
    /// leaves things as they are and is no error: the player has chosen nothing yet.
    pub fn load(&mut self, path: impl AsRef<std::path::Path>) -> Result<(), String> {
        match std::fs::read_to_string(&path) {
            Ok(text) => self.read_text(&text).map_err(|why| format!("{}: {why}", path.as_ref().display())),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(err) => Err(format!("{}: {err}", path.as_ref().display())),
        }
    }
}

/// Brings the actions up to date with the frame's keys and buttons, before the game reads
/// them.
pub(crate) fn update_actions(mut actions: ResMut<Actions>, keys: Res<ButtonInput<KeyCode>>, buttons: Res<ButtonInput<MouseButton>>, touches: Option<Res<Touches>>, pads: Option<Res<Gamepads>>) {
    actions.update_from(&Inputs { keys: &keys, buttons: &buttons, touches: touches.as_deref(), pads: pads.as_deref() });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inputs() -> (ButtonInput<KeyCode>, ButtonInput<MouseButton>) {
        (ButtonInput::default(), ButtonInput::default())
    }

    #[test]
    fn an_action_is_pressed_by_any_of_what_it_is_bound_to() {
        let (mut keys, mut buttons) = inputs();
        let mut actions = Actions::default();
        actions.bind("fire", KeyCode::Space).bind("fire", MouseButton::Left).bind("fire", KeyCode::Space);
        assert_eq!(actions.bindings("fire").len(), 2, "the same key twice is once");
        actions.update(&keys, &buttons);
        assert!(!actions.pressed("fire") && !actions.pressed("nothing of the name"));

        keys.press(KeyCode::Space);
        actions.update(&keys, &buttons);
        assert!(actions.pressed("fire") && actions.just_pressed("fire"));
        // Held by the button as well, then by the button alone: pressed throughout, and
        // not pressed anew.
        buttons.press(MouseButton::Left);
        actions.update(&keys, &buttons);
        keys.release(KeyCode::Space);
        actions.update(&keys, &buttons);
        assert!(actions.pressed("fire") && !actions.just_pressed("fire") && !actions.just_released("fire"));
        buttons.release(MouseButton::Left);
        actions.update(&keys, &buttons);
        assert!(!actions.pressed("fire") && actions.just_released("fire"));
        actions.update(&keys, &buttons);
        assert!(!actions.just_released("fire"));
    }

    #[test]
    fn an_axis_is_pulled_each_way_and_two_make_a_direction_no_longer_than_one() {
        let (mut keys, buttons) = inputs();
        let mut actions = Actions::default();
        actions.bind_axis("across", KeyCode::KeyA, KeyCode::KeyD).bind_axis("across", KeyCode::ArrowLeft, KeyCode::ArrowRight);
        actions.bind_axis("along", KeyCode::KeyS, KeyCode::KeyW);
        keys.press(KeyCode::KeyD);
        actions.update(&keys, &buttons);
        assert_eq!((actions.value("across"), actions.value("along"), actions.value("no such")), (1.0, 0.0, 0.0));
        // Both of a pair is neither; two pairs the same way are still one.
        keys.press(KeyCode::KeyA);
        actions.update(&keys, &buttons);
        assert_eq!(actions.value("across"), 0.0);
        keys.release(KeyCode::KeyA);
        keys.press(KeyCode::ArrowRight);
        keys.press(KeyCode::KeyW);
        actions.update(&keys, &buttons);
        assert_eq!(actions.value("across"), 1.0);
        let direction = actions.direction("across", "along");
        assert!((direction.length() - 1.0).abs() < 1e-6 && direction.x > 0.7 && direction.y > 0.7);
    }

    #[test]
    fn an_action_is_given_the_next_key_pressed() {
        let (mut keys, mut buttons) = inputs();
        let mut actions = Actions::default();
        actions.bind("jump", KeyCode::Space).bind("fire", MouseButton::Left);
        keys.press(KeyCode::Space);
        actions.update(&keys, &buttons);
        keys.clear();
        actions.rebind("jump");
        assert_eq!(actions.rebinding(), Some("jump"));
        // While a key is waited for nothing is pressed, though the old key is still held.
        actions.update(&keys, &buttons);
        assert!(!actions.pressed("jump") && actions.just_released("jump"));
        keys.release(KeyCode::Space);
        keys.clear();
        // The key chosen is the action's, and does not press it in the frame it is chosen.
        keys.press(KeyCode::KeyJ);
        actions.update(&keys, &buttons);
        assert_eq!((actions.bindings("jump"), actions.rebinding()), (&[Binding::Key(KeyCode::KeyJ)][..], None));
        assert!(!actions.pressed("jump"));
        keys.clear();
        actions.update(&keys, &buttons);
        assert!(actions.pressed("jump") && actions.just_pressed("jump"));
        // A mouse button will do as well; and one that is cancelled is left as it was.
        actions.rebind("jump");
        buttons.press(MouseButton::Right);
        actions.update(&keys, &buttons);
        assert_eq!(actions.bindings("jump"), [Binding::Mouse(MouseButton::Right)]);
        actions.rebind("fire");
        actions.cancel_rebind();
        assert_eq!(actions.bindings("fire"), [Binding::Mouse(MouseButton::Left)]);
    }

    #[test]
    fn a_key_played_into_a_running_game_presses_its_action_that_frame() {
        use crate::app::{App, Stage};
        #[derive(Default)]
        struct Jumps(u32);
        let mut app = App::new();
        app.add_plugins(crate::time::TimePlugin).add_plugins(crate::input::InputPlugin);
        app.init_resource::<Jumps>().add_systems(Stage::Update, |actions: Res<Actions>, mut jumps: ResMut<Jumps>| {
            jumps.0 += actions.just_pressed("jump") as u32;
        });
        app.world.resource_mut::<Actions>().bind("jump", KeyCode::Space);
        app.world.resource_mut::<crate::input::InjectedInput>().tap(KeyCode::Space, 3);
        for _ in 0..6 {
            app.update();
        }
        // Pressed once, held for three frames and let go: one jump.
        assert_eq!(app.world.resource::<Jumps>().0, 1);
        assert!(!app.world.resource::<Actions>().pressed("jump"));
    }

    #[test]
    fn a_controller_presses_actions_and_its_sticks_are_axes() {
        let (mut keys, buttons) = inputs();
        let mut pads = Gamepads::default();
        let mut actions = Actions::default();
        actions.bind("jump", KeyCode::Space).bind("jump", PadButton::South);
        actions.bind_axis("across", KeyCode::KeyA, KeyCode::KeyD).bind_stick("across", PadAxis::LeftX);
        actions.bind_stick("gas", PadAxis::RightPull);
        let update = |actions: &mut Actions, keys: &ButtonInput<KeyCode>, pads: &Gamepads| actions.update_from(&Inputs { keys, buttons: &buttons, touches: None, pads: Some(pads) });

        pads.press(1, PadButton::South);
        pads.set_axis(1, PadAxis::LeftX, 0.575);
        pads.set_axis(1, PadAxis::RightPull, 1.0);
        update(&mut actions, &keys, &pads);
        assert!(actions.just_pressed("jump"));
        assert!((actions.value("across") - 0.5).abs() < 1e-5, "{}", actions.value("across"));
        assert_eq!(actions.value("gas"), 1.0);
        // A key and the stick the same way are no more than one; against each other they
        // cancel as far as the stick is pushed.
        keys.press(KeyCode::KeyD);
        update(&mut actions, &keys, &pads);
        assert_eq!(actions.value("across"), 1.0);
        keys.release(KeyCode::KeyD);
        keys.press(KeyCode::KeyA);
        update(&mut actions, &keys, &pads);
        assert!((actions.value("across") + 0.5).abs() < 1e-5);
        // Unplugged: the button is let go and the stick is at rest.
        keys.release(KeyCode::KeyA);
        pads.disconnected(1);
        update(&mut actions, &keys, &pads);
        assert!(actions.just_released("jump") && actions.value("across") == 0.0 && actions.value("gas") == 0.0);

        // Written down and read back, sticks beside pairs and alone.
        let text = actions.to_text();
        assert_eq!(text, "jump = Space, PadSouth\nacross = KeyA/KeyD, PadLeftX\ngas = PadRightPull\n");
        let mut read = Actions::default();
        read.read_text(&text).unwrap();
        assert_eq!(read.to_text(), text);
        assert_eq!(read.names(), (vec!["jump"], vec!["across", "gas"]));
        // A file that leaves the stick out takes it away.
        read.read_text("across = KeyA/KeyD").unwrap();
        assert!(read.stick_bindings("across").is_empty() && read.axis_bindings("across").len() == 1);

        // Choosing a button for an action hears a controller's as it hears a key.
        keys.clear();
        actions.rebind("jump");
        pads.press(2, PadButton::East);
        update(&mut actions, &keys, &pads);
        assert_eq!(actions.bindings("jump"), [Binding::Pad(PadButton::East)]);
    }

    #[test]
    fn a_finger_presses_an_action_as_a_key_does() {
        use crate::app::{App, Stage};
        #[derive(Default)]
        struct Did(Vec<&'static str>);
        let mut app = App::new();
        app.add_plugins(crate::time::TimePlugin).add_plugins(crate::input::InputPlugin);
        app.init_resource::<Did>().add_systems(Stage::Update, |actions: Res<Actions>, mut did: ResMut<Did>| {
            did.0.extend(["up", "left", "fire"].into_iter().filter(|action| actions.just_pressed(action)));
        });
        app.world.resource_mut::<Actions>().bind("up", KeyCode::KeyW).bind("up", Swipe::Up).bind("left", Swipe::Left).bind("fire", Binding::Tap);
        // A finger drawn up the screen over three frames, then one put down and lifted, then
        // one drawn to the left.
        app.world.resource_mut::<crate::input::InjectedInput>().swipe(Vec2::new(300.0, 500.0), Vec2::new(305.0, 380.0), 3);
        for _ in 0..5 {
            app.update();
        }
        app.world.resource_mut::<crate::input::InjectedInput>().swipe(Vec2::new(100.0, 100.0), Vec2::new(100.0, 100.0), 1);
        for _ in 0..3 {
            app.update();
        }
        app.world.resource_mut::<crate::input::InjectedInput>().swipe(Vec2::new(400.0, 300.0), Vec2::new(300.0, 310.0), 2);
        for _ in 0..4 {
            app.update();
        }
        // Each pressed its action once, for the frame it happened, and no longer.
        assert_eq!(app.world.resource::<Did>().0, ["up", "fire", "left"]);
        assert!(!app.world.resource::<Actions>().pressed("up"));
        // And they are written down and read back as keys are.
        let text = app.world.resource::<Actions>().to_text();
        assert_eq!(text, "fire = Tap\nleft = SwipeLeft\nup = KeyW, SwipeUp\n");
        let mut read = Actions::default();
        read.read_text(&text).unwrap();
        assert_eq!(read.to_text(), text);
    }

    #[test]
    fn bindings_are_written_out_and_read_back() {
        let mut actions = Actions::default();
        actions.bind("jump", KeyCode::Space).bind("jump", MouseButton::Other(7)).bind("fire", MouseButton::Left);
        actions.bind_axis("strafe", KeyCode::KeyA, KeyCode::KeyD).bind_axis("strafe", KeyCode::ArrowLeft, KeyCode::ArrowRight);
        let text = actions.to_text();
        assert_eq!(text, "fire = MouseLeft\njump = Space, Mouse7\nstrafe = KeyA/KeyD, ArrowLeft/ArrowRight\n");
        let mut read = Actions::default();
        read.bind("crouch", KeyCode::KeyC);
        read.read_text(&format!("# the player's own\n\n{text}")).unwrap();
        assert_eq!(read.to_text(), format!("crouch = KeyC\n{text}"), "what the text does not name is left");
        // A player's file with a mistake in it is told of and changes nothing.
        let wrong = read.read_text("jump = KeyJ\nfire = Trigger").unwrap_err();
        assert!(wrong.contains("line 2") && wrong.contains("Trigger"), "{wrong}");
        assert_eq!(read.bindings("jump").len(), 2);
        assert!(read.read_text("no equals sign").is_err() && read.read_text("strafe = KeyA/KeyD, KeyQ").is_err());
        // A file that is not there is a player who has chosen nothing.
        assert!(read.load("no/such/bindings.txt").is_ok());
        assert!(Binding::named("mouseleft") == Some(Binding::Mouse(MouseButton::Left)) && Binding::named("space") == Some(Binding::Key(KeyCode::Space)));
    }
}