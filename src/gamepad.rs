//! Game controllers.
//!
//! [`Gamepads`] has every controller that is plugged in: the buttons held on each, and where
//! its sticks and triggers are. A game is better bound to these through
//! [`Actions`](crate::actions::Actions), as it is to keys: `actions.bind("jump",
//! PadButton::South)` beside the space bar, and `actions.bind_stick("across", PadAxis::LeftX)`
//! beside A and D. Asked that way, any controller plays the game, and so does the keyboard.
//!
//! Buttons are named by where they are, not by what is printed on them: `South` is the
//! lowest of the four on the right, which one maker letters A and another marks with a cross.

use glam::Vec2;

use crate::input::ButtonInput;

/// A button of a controller, named by where it is on it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PadButton {
    /// The four on the right, by where each is among them.
    South,
    East,
    West,
    North,
    /// The upper pair on the shoulders, and the lower, which are pulled.
    LeftBumper,
    RightBumper,
    LeftTrigger,
    RightTrigger,
    /// The two small ones in the middle.
    Select,
    Start,
    /// A stick pressed in.
    LeftStick,
    RightStick,
    /// The four ways of the pad on the left.
    Up,
    Down,
    Left,
    Right,
}

impl PadButton {
    pub const ALL: [Self; 16] = [
        Self::South,
        Self::East,
        Self::West,
        Self::North,
        Self::LeftBumper,
        Self::RightBumper,
        Self::LeftTrigger,
        Self::RightTrigger,
        Self::Select,
        Self::Start,
        Self::LeftStick,
        Self::RightStick,
        Self::Up,
        Self::Down,
        Self::Left,
        Self::Right,
    ];

    /// What it is called where bindings are written down: `PadSouth`, `PadLeftBumper`.
    pub fn name(&self) -> String {
        format!("Pad{self:?}")
    }

    /// The button with that name, if there is one.
    pub fn named(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|button| button.name().eq_ignore_ascii_case(name.trim()))
    }
}

/// Something of a controller that is somewhere between two ends: a stick, across or along,
/// from -1 to 1 (right and up are more), or how far a trigger is pulled, from 0 to 1.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PadAxis {
    LeftX,
    LeftY,
    RightX,
    RightY,
    LeftPull,
    RightPull,
}

impl PadAxis {
    pub const ALL: [Self; 6] = [Self::LeftX, Self::LeftY, Self::RightX, Self::RightY, Self::LeftPull, Self::RightPull];

    /// What it is called where bindings are written down: `PadLeftX`, `PadRightPull`.
    pub fn name(&self) -> String {
        format!("Pad{self:?}")
    }

    /// The axis with that name, if there is one.
    pub fn named(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|axis| axis.name().eq_ignore_ascii_case(name.trim()))
    }
}

/// A controller that is plugged in.
pub struct Gamepad {
    /// Which controller: the same number for as long as it stays plugged in.
    pub id: u64,
    /// What it calls itself.
    pub name: String,
    buttons: ButtonInput<PadButton>,
    axes: [f32; PadAxis::ALL.len()],
}

impl Gamepad {
    /// Whether a button of this controller is held.
    pub fn pressed(&self, button: PadButton) -> bool {
        self.buttons.pressed(button)
    }

    /// Where an axis of this controller is, as the controller says, small wobbles and all.
    pub fn axis(&self, axis: PadAxis) -> f32 {
        self.axes[axis as usize]
    }
}

/// The controllers that are plugged in. A resource.
pub struct Gamepads {
    pads: Vec<Gamepad>,
    /// How far a stick must be from its middle to count as moved at all: a stick let go
    /// does not come quite to rest. From there to its end is read as nought to one.
    pub dead_zone: f32,
}

impl Default for Gamepads {
    fn default() -> Self {
        Self { pads: Vec::new(), dead_zone: 0.15 }
    }
}

impl Gamepads {
    /// A controller was plugged in.
    pub fn connected(&mut self, id: u64, name: impl Into<String>) {
        if !self.pads.iter().any(|pad| pad.id == id) {
            self.pads.push(Gamepad { id, name: name.into(), buttons: ButtonInput::default(), axes: [0.0; PadAxis::ALL.len()] });
        }
    }

    /// A controller was unplugged: what was held on it is held no longer.
    pub fn disconnected(&mut self, id: u64) {
        self.pads.retain(|pad| pad.id != id);
    }

    fn pad(&mut self, id: u64) -> &mut Gamepad {
        // One heard from before it was said to be there is there.
        self.connected(id, "controller");
        self.pads.iter_mut().find(|pad| pad.id == id).expect("it was just added")
    }

    /// A button went down, or came up.
    pub fn press(&mut self, id: u64, button: PadButton) {
        self.pad(id).buttons.press(button);
    }

    pub fn release(&mut self, id: u64, button: PadButton) {
        self.pad(id).buttons.release(button);
    }

    /// A stick or a trigger moved.
    pub fn set_axis(&mut self, id: u64, axis: PadAxis, value: f32) {
        self.pad(id).axes[axis as usize] = value.clamp(-1.0, 1.0);
    }

    /// The controllers there are.
    pub fn iter(&self) -> impl Iterator<Item = &Gamepad> {
        self.pads.iter()
    }

    /// Whether a button is held on any controller.
    pub fn pressed(&self, button: PadButton) -> bool {
        self.pads.iter().any(|pad| pad.buttons.pressed(button))
    }

    /// Whether it went down this frame, on any.
    pub fn just_pressed(&self, button: PadButton) -> bool {
        self.pads.iter().any(|pad| pad.buttons.just_pressed(button))
    }

    /// Whether it came up this frame, on any.
    pub fn just_released(&self, button: PadButton) -> bool {
        self.pads.iter().any(|pad| pad.buttons.just_released(button))
    }

    /// The buttons that went down this frame, on any controller.
    pub fn get_just_pressed(&self) -> impl Iterator<Item = PadButton> + '_ {
        self.pads.iter().flat_map(|pad| pad.buttons.get_just_pressed().copied())
    }

    /// Where an axis is, past the dead zone: of whichever controller has it furthest from
    /// rest, so that any of them plays.
    pub fn axis(&self, axis: PadAxis) -> f32 {
        let dead = self.dead_zone.clamp(0.0, 0.99);
        let furthest = self.pads.iter().map(|pad| pad.axis(axis)).fold(0.0f32, |furthest, value| if value.abs() > furthest.abs() { value } else { furthest });
        if furthest.abs() <= dead {
            return 0.0;
        }
        furthest.signum() * (furthest.abs() - dead) / (1.0 - dead)
    }

    /// The left stick as a direction, and the right.
    pub fn left_stick(&self) -> Vec2 {
        Vec2::new(self.axis(PadAxis::LeftX), self.axis(PadAxis::LeftY))
    }

    pub fn right_stick(&self) -> Vec2 {
        Vec2::new(self.axis(PadAxis::RightX), self.axis(PadAxis::RightY))
    }

    /// Forgets which buttons went down and came up, at the end of a frame. Those held stay
    /// held, and the sticks stay where they are.
    pub fn clear(&mut self) {
        for pad in &mut self.pads {
            pad.buttons.clear();
        }
    }
}

/// The system's controllers, heard from and passed on to [`Gamepads`]. There are none to
/// hear from on a phone, where this is nothing.
#[cfg(not(any(target_os = "android", target_os = "ios")))]
pub(crate) struct Heard(Option<gilrs::Gilrs>);

#[cfg(not(any(target_os = "android", target_os = "ios")))]
impl Heard {
    pub(crate) fn new() -> Self {
        match gilrs::Gilrs::new() {
            Ok(system) => Self(Some(system)),
            Err(err) => {
                log::info!("no game controllers will be heard from: {err}");
                Self(None)
            }
        }
    }

    /// Passes on what the controllers have done since last asked.
    pub(crate) fn pass_on(&mut self, pads: &mut Gamepads) {
        use gilrs::{Axis, Button, EventType};
        let Some(system) = &mut self.0 else { return };
        // Those plugged in before the game began are not announced.
        if pads.pads.is_empty() {
            for (id, pad) in system.gamepads() {
                pads.connected(usize::from(id) as u64, pad.name());
            }
        }
        let button = |button: Button| {
            Some(match button {
                Button::South => PadButton::South,
                Button::East => PadButton::East,
                Button::West => PadButton::West,
                Button::North => PadButton::North,
                Button::LeftTrigger => PadButton::LeftBumper,
                Button::RightTrigger => PadButton::RightBumper,
                Button::LeftTrigger2 => PadButton::LeftTrigger,
                Button::RightTrigger2 => PadButton::RightTrigger,
                Button::Select => PadButton::Select,
                Button::Start => PadButton::Start,
                Button::LeftThumb => PadButton::LeftStick,
                Button::RightThumb => PadButton::RightStick,
                Button::DPadUp => PadButton::Up,
                Button::DPadDown => PadButton::Down,
                Button::DPadLeft => PadButton::Left,
                Button::DPadRight => PadButton::Right,
                _ => return None,
            })
        };
        while let Some(event) = system.next_event() {
            let id = usize::from(event.id) as u64;
            match event.event {
                EventType::Connected => pads.connected(id, system.gamepad(event.id).name()),
                EventType::Disconnected => pads.disconnected(id),
                EventType::ButtonPressed(pressed, _) => {
                    if let Some(pressed) = button(pressed) {
                        pads.press(id, pressed);
                    }
                }
                EventType::ButtonReleased(released, _) => {
                    if let Some(released) = button(released) {
                        pads.release(id, released);
                    }
                }
                // How far a trigger is pulled comes as how far its button is down.
                EventType::ButtonChanged(Button::LeftTrigger2, value, _) => pads.set_axis(id, PadAxis::LeftPull, value),
                EventType::ButtonChanged(Button::RightTrigger2, value, _) => pads.set_axis(id, PadAxis::RightPull, value),
                EventType::AxisChanged(Axis::LeftStickX, value, _) => pads.set_axis(id, PadAxis::LeftX, value),
                EventType::AxisChanged(Axis::LeftStickY, value, _) => pads.set_axis(id, PadAxis::LeftY, value),
                EventType::AxisChanged(Axis::RightStickX, value, _) => pads.set_axis(id, PadAxis::RightX, value),
                EventType::AxisChanged(Axis::RightStickY, value, _) => pads.set_axis(id, PadAxis::RightY, value),
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn any_controller_presses_a_button_and_unplugging_lets_go() {
        let mut pads = Gamepads::default();
        pads.connected(1, "first");
        pads.connected(2, "second");
        pads.press(2, PadButton::South);
        assert!(pads.pressed(PadButton::South) && pads.just_pressed(PadButton::South));
        assert!(!pads.iter().next().unwrap().pressed(PadButton::South), "the first is not the one");
        assert_eq!(pads.get_just_pressed().collect::<Vec<_>>(), [PadButton::South]);
        // Held into the next frame, and no longer just pressed.
        pads.clear();
        assert!(pads.pressed(PadButton::South) && !pads.just_pressed(PadButton::South));
        pads.release(2, PadButton::South);
        assert!(pads.just_released(PadButton::South) && !pads.pressed(PadButton::South));
        // Unplugged with a button down: it is down no longer.
        pads.press(1, PadButton::Start);
        pads.disconnected(1);
        assert!(!pads.pressed(PadButton::Start));
        assert_eq!(pads.iter().map(|pad| pad.name.as_str()).collect::<Vec<_>>(), ["second"]);
        // One heard from that was never announced is there all the same.
        pads.press(9, PadButton::North);
        assert!(pads.pressed(PadButton::North) && pads.iter().count() == 2);
    }

    #[test]
    fn a_stick_at_rest_reads_nought_and_at_its_end_one() {
        let mut pads = Gamepads::default();
        pads.set_axis(1, PadAxis::LeftX, 0.1);
        assert_eq!(pads.axis(PadAxis::LeftX), 0.0, "within the dead zone");
        pads.set_axis(1, PadAxis::LeftX, 1.0);
        assert_eq!(pads.axis(PadAxis::LeftX), 1.0);
        pads.set_axis(1, PadAxis::LeftX, -0.575);
        assert!((pads.axis(PadAxis::LeftX) + 0.5).abs() < 1e-5, "half way from the dead zone to the end");
        // Whichever controller has it furthest is the one read.
        pads.set_axis(2, PadAxis::LeftX, 0.9);
        assert!(pads.axis(PadAxis::LeftX) > 0.8);
        pads.set_axis(1, PadAxis::LeftY, 5.0);
        assert_eq!(pads.left_stick().y, 1.0, "no further than its end");
        assert_eq!(pads.right_stick(), Vec2::ZERO);
    }

    #[test]
    fn buttons_and_axes_are_known_by_their_names() {
        for button in PadButton::ALL {
            assert_eq!(PadButton::named(&button.name()), Some(button));
        }
        for axis in PadAxis::ALL {
            assert_eq!(PadAxis::named(&axis.name()), Some(axis));
        }
        assert_eq!(PadButton::named("padsouth"), Some(PadButton::South));
        assert_eq!(PadButton::named("PadLeftX"), None);
        assert_eq!(PadAxis::named("PadSouth"), None);
    }
}
