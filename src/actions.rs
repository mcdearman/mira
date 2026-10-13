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
    input::{key_named, ButtonInput, KeyCode, MouseButton},
};

/// A key or a mouse button: what an action can be bound to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Binding {
    Key(KeyCode),
    Mouse(MouseButton),
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
        }
    }

    /// The binding with that name, if there is one.
    pub fn named(name: &str) -> Option<Self> {
        let name = name.trim();
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

    fn down(&self, keys: &ButtonInput<KeyCode>, buttons: &ButtonInput<MouseButton>) -> bool {
        match self {
            Self::Key(key) => keys.pressed(*key),
            Self::Mouse(button) => buttons.pressed(*button),
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

    /// Takes every binding from an action or an axis.
    pub fn unbind(&mut self, name: &str) -> &mut Self {
        self.buttons.remove(name);
        self.axes.remove(name);
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

    /// The actions there are, and the axes, in the order of their names.
    pub fn names(&self) -> (Vec<&str>, Vec<&str>) {
        (self.buttons.keys().map(String::as_str).collect(), self.axes.keys().map(String::as_str).collect())
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
        self.went_down.clear();
        self.went_up.clear();
        if let Some(action) = self.hearing.clone() {
            // The first thing newly pressed is the action's from now on.
            let pressed = keys.get_just_pressed().next().map(|key| Binding::Key(*key)).or_else(|| buttons.get_just_pressed().next().map(|button| Binding::Mouse(*button)));
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
            let now = bindings.iter().any(|binding| binding.down(keys, buttons));
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
            let pull: f32 = pairs.iter().map(|(less, more)| more.down(keys, buttons) as i32 as f32 - less.down(keys, buttons) as i32 as f32).sum();
            self.values.insert(axis.clone(), pull.clamp(-1.0, 1.0));
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
        for (axis, pairs) in &self.axes {
            let names: Vec<String> = pairs.iter().map(|(less, more)| format!("{}/{}", less.name(), more.name())).collect();
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
            if parts.iter().any(|part| part.contains('/')) {
                let mut pairs = Vec::new();
                for part in parts {
                    let (less, more) = part.split_once('/').ok_or_else(|| wrong("an axis is pairs, each `Less/More`"))?;
                    pairs.push((known(less)?, known(more)?));
                }
                axes.push((name.to_owned(), pairs));
            } else {
                buttons.push((name.to_owned(), parts.into_iter().map(known).collect::<Result<Vec<_>, _>>()?));
            }
        }
        for (name, bindings) in buttons {
            self.axes.remove(&name);
            self.buttons.insert(name, bindings);
        }
        for (name, pairs) in axes {
            self.buttons.remove(&name);
            self.axes.insert(name, pairs);
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
pub(crate) fn update_actions(mut actions: ResMut<Actions>, keys: Res<ButtonInput<KeyCode>>, buttons: Res<ButtonInput<MouseButton>>) {
    actions.update(&keys, &buttons);
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