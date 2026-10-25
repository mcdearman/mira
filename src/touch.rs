//! Fingers on a screen.
//!
//! [`Touches`] has every finger that is down, where it went down and where it is now, and
//! what the fingers did this frame that a game would name: a tap, or a swipe one of four
//! ways. A swipe is told as soon as the finger has gone far enough, not when it is lifted,
//! so that a game answers to it at once.
//!
//! A game is better bound to these through [`Actions`](crate::actions::Actions), as it is
//! to keys: `actions.bind("up", Swipe::Up)` beside `actions.bind("up", KeyCode::KeyW)`, and
//! the game asks for "up" and does not know which it was.
//!
//! On a machine with no screen to touch, `Touches::mouse_too` has the left mouse button
//! stand for a finger, so that a game made for a phone can be tried at a desk.
//!
//! A game that is steered rather than swiped (a player run about, a car driven) has the
//! screen's two halves: a finger held on the left half and drawn away from where it went
//! down is a stick (`Touches::dragged`), one held on the right a button
//! (`Touches::held`), and either half can be tapped on its own (`Touches::tapped_on`).

use glam::Vec2;

use crate::{
    ecs::{Res, ResMut},
    input::{ButtonInput, Mouse, MouseButton},
};

/// One of the four ways a finger can be swiped, as the screen is held: up is toward its top.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Swipe {
    Up,
    Down,
    Left,
    Right,
}

/// A half of the screen, as it is held: a finger is on the half it went down on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Side {
    Left,
    Right,
}

/// What a finger did that a game would name.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Gesture {
    /// Put down and lifted without going anywhere: where.
    Tap(Vec2),
    /// Drawn far enough one way: which, and where it began.
    Swiped(Swipe, Vec2),
}

/// A finger that is down.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Touch {
    /// Which finger: the same number for as long as it stays down.
    pub id: u64,
    /// Where it went down, and where it is, in physical pixels from the top left.
    pub start: Vec2,
    pub position: Vec2,
    /// Whether it has been told of as a swipe already: it is then no tap when lifted, and
    /// no second swipe until it is lifted and put down again.
    swiped: bool,
}

/// The fingers that are down, and what they did this frame. A resource.
pub struct Touches {
    down: Vec<Touch>,
    gestures: Vec<Gesture>,
    /// How far a finger must go, in physical pixels, to have swiped. Less than this and
    /// lifted is a tap.
    pub swipe_distance: f32,
    /// Whether the left mouse button stands for a finger.
    pub mouse_too: bool,
    /// How far, in physical pixels, a finger held as a stick must go from where it went down
    /// before it pushes it at all.
    pub drag_dead_zone: f32,
    /// How wide the screen is, in physical pixels, for telling its halves apart: kept up to
    /// date from the window. Nought until it is known, when every finger is on the left.
    pub screen_width: f32,
}

impl Default for Touches {
    fn default() -> Self {
        Self { down: Vec::new(), gestures: Vec::new(), swipe_distance: 36.0, mouse_too: false, drag_dead_zone: 24.0, screen_width: 0.0 }
    }
}

/// The number the mouse has as a finger, where it stands for one.
const MOUSE: u64 = u64::MAX;

impl Touches {
    /// A finger went down.
    pub fn began(&mut self, id: u64, at: Vec2) {
        self.down.retain(|touch| touch.id != id);
        self.down.push(Touch { id, start: at, position: at, swiped: false });
    }

    /// A finger that is down moved. Once it is far enough from where it went down, it has
    /// swiped, the way it has gone furthest.
    pub fn moved(&mut self, id: u64, to: Vec2) {
        let far = self.swipe_distance;
        let Some(touch) = self.down.iter_mut().find(|touch| touch.id == id) else { return };
        touch.position = to;
        let gone = to - touch.start;
        if !touch.swiped && gone.length() >= far {
            touch.swiped = true;
            // Down the screen is more y.
            let way = if gone.x.abs() > gone.y.abs() {
                if gone.x > 0.0 { Swipe::Right } else { Swipe::Left }
            } else if gone.y > 0.0 {
                Swipe::Down
            } else {
                Swipe::Up
            };
            self.gestures.push(Gesture::Swiped(way, touch.start));
        }
    }

    /// A finger was lifted. One that went nowhere tapped.
    pub fn ended(&mut self, id: u64, at: Vec2) {
        self.moved(id, at);
        if let Some(place) = self.down.iter().position(|touch| touch.id == id) {
            let touch = self.down.remove(place);
            if !touch.swiped {
                self.gestures.push(Gesture::Tap(touch.start));
            }
        }
    }

    /// A finger was taken away by the system, as when a call comes in: it did nothing.
    pub fn cancelled(&mut self, id: u64) {
        self.down.retain(|touch| touch.id != id);
    }

    /// The fingers that are down.
    pub fn iter(&self) -> impl Iterator<Item = &Touch> {
        self.down.iter()
    }

    /// What the fingers did this frame.
    pub fn gestures(&self) -> &[Gesture] {
        &self.gestures
    }

    /// The way a finger swiped this frame, if one did.
    pub fn swiped(&self) -> Option<Swipe> {
        self.gestures.iter().find_map(|gesture| match gesture {
            Gesture::Swiped(way, _) => Some(*way),
            Gesture::Tap(_) => None,
        })
    }

    /// Where a finger tapped this frame, if one did.
    pub fn tapped(&self) -> Option<Vec2> {
        self.gestures.iter().find_map(|gesture| match gesture {
            Gesture::Tap(at) => Some(*at),
            Gesture::Swiped(..) => None,
        })
    }

    /// Forgets what the fingers did, at the end of a frame. Those still down stay down.
    pub fn clear(&mut self) {
        self.gestures.clear();
    }

    /// The half of the screen a place is on.
    pub fn side_of(&self, at: Vec2) -> Side {
        if self.screen_width > 0.0 && at.x >= self.screen_width / 2.0 { Side::Right } else { Side::Left }
    }

    /// Whether a finger is down on a half of the screen, or anywhere.
    pub fn held(&self, side: Option<Side>) -> bool {
        self.down.iter().any(|touch| side.is_none_or(|side| self.side_of(touch.start) == side))
    }

    /// Whether a half of the screen was tapped this frame.
    pub fn tapped_on(&self, side: Side) -> bool {
        self.gestures.iter().any(|gesture| matches!(gesture, Gesture::Tap(at) if self.side_of(*at) == side))
    }

    /// Whether a finger held on the left half is drawn the given way from where it went down,
    /// past the dead zone: the four of these are a stick, two of them at once on a slant.
    pub fn dragged(&self, way: Swipe) -> bool {
        self.down.iter().filter(|touch| self.side_of(touch.start) == Side::Left).any(|touch| {
            let gone = touch.position - touch.start;
            let far = gone.length();
            if far < self.drag_dead_zone {
                return false;
            }
            // Down the screen is more y. A third of the way off the line still counts, so that
            // a slant pushes both ways.
            let along = gone / far;
            match way {
                Swipe::Up => along.y < -0.38,
                Swipe::Down => along.y > 0.38,
                Swipe::Left => along.x < -0.38,
                Swipe::Right => along.x > 0.38,
            }
        })
    }
}

/// Has the left mouse button stand for a finger, where that is asked for.
pub(crate) fn mouse_as_a_finger(mut touches: ResMut<Touches>, buttons: Res<ButtonInput<MouseButton>>, mouse: Res<Mouse>, window: Option<Res<crate::window::Window>>) {
    if !touches.mouse_too {
        return;
    }
    if let Some(window) = window {
        touches.screen_width = window.size().x as f32;
    }
    let Some(at) = mouse.position else {
        // Off the window with the button down: the finger is gone.
        if buttons.just_released(MouseButton::Left) {
            touches.cancelled(MOUSE);
        }
        return;
    };
    if buttons.just_pressed(MouseButton::Left) {
        touches.began(MOUSE, at);
    } else if buttons.just_released(MouseButton::Left) {
        touches.ended(MOUSE, at);
    } else if buttons.pressed(MouseButton::Left) {
        touches.moved(MOUSE, at);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_finger_that_goes_far_enough_has_swiped_the_way_it_went_furthest() {
        let mut touches = Touches::default();
        touches.began(1, Vec2::new(200.0, 400.0));
        touches.moved(1, Vec2::new(210.0, 380.0));
        assert!(touches.swiped().is_none(), "not far enough yet");
        // Told as soon as it is far enough, while the finger is still down.
        touches.moved(1, Vec2::new(215.0, 350.0));
        assert_eq!(touches.swiped(), Some(Swipe::Up));
        assert_eq!(touches.gestures(), [Gesture::Swiped(Swipe::Up, Vec2::new(200.0, 400.0))]);
        assert_eq!(touches.iter().count(), 1);
        // Once only, however much further it goes, and lifting it is no tap.
        touches.clear();
        touches.moved(1, Vec2::new(400.0, 300.0));
        touches.ended(1, Vec2::new(420.0, 300.0));
        assert!(touches.gestures().is_empty() && touches.iter().next().is_none());
        // Each of the other ways.
        for (to, way) in [(Vec2::new(0.0, 60.0), Swipe::Down), (Vec2::new(-60.0, 10.0), Swipe::Left), (Vec2::new(60.0, -20.0), Swipe::Right)] {
            touches.clear();
            touches.began(2, Vec2::new(300.0, 300.0));
            touches.ended(2, Vec2::new(300.0, 300.0) + to);
            assert_eq!(touches.swiped(), Some(way));
        }
    }

    #[test]
    fn a_finger_lifted_where_it_went_down_tapped() {
        let mut touches = Touches::default();
        touches.began(7, Vec2::new(50.0, 60.0));
        touches.moved(7, Vec2::new(54.0, 58.0));
        assert!(touches.tapped().is_none(), "not until it is lifted");
        touches.ended(7, Vec2::new(55.0, 57.0));
        assert_eq!(touches.tapped(), Some(Vec2::new(50.0, 60.0)));
        assert!(touches.swiped().is_none());
        // Two fingers are told apart, and one the system takes away did nothing.
        touches.clear();
        touches.began(1, Vec2::ZERO);
        touches.began(2, Vec2::new(500.0, 0.0));
        touches.cancelled(1);
        touches.ended(2, Vec2::new(500.0, 0.0));
        assert_eq!(touches.gestures(), [Gesture::Tap(Vec2::new(500.0, 0.0))]);
        assert!(touches.iter().next().is_none());
    }

    #[test]
    fn the_left_half_is_a_stick_the_right_a_button_and_each_can_be_tapped() {
        let mut touches = Touches { screen_width: 1000.0, ..Default::default() };
        // A thumb on the left, drawn up and to the right: both ways at once, past the dead zone.
        touches.began(1, Vec2::new(200.0, 500.0));
        touches.moved(1, Vec2::new(210.0, 490.0));
        assert!(!touches.dragged(Swipe::Up) && touches.held(Some(Side::Left)), "not past the dead zone yet");
        touches.moved(1, Vec2::new(240.0, 450.0));
        assert!(touches.dragged(Swipe::Up) && touches.dragged(Swipe::Right));
        assert!(!touches.dragged(Swipe::Down) && !touches.dragged(Swipe::Left));
        // A thumb on the right is held, and is no stick.
        touches.ended(1, Vec2::new(240.0, 450.0));
        touches.began(2, Vec2::new(800.0, 500.0));
        touches.moved(2, Vec2::new(800.0, 300.0));
        assert!(touches.held(Some(Side::Right)) && touches.held(None) && !touches.held(Some(Side::Left)));
        assert!(!touches.dragged(Swipe::Up), "the right thumb pushes nothing");
        touches.ended(2, Vec2::new(800.0, 300.0));
        assert!(!touches.held(Some(Side::Right)));
        // Taps, told apart by their half.
        touches.clear();
        touches.began(3, Vec2::new(100.0, 100.0));
        touches.ended(3, Vec2::new(100.0, 100.0));
        assert!(touches.tapped_on(Side::Left) && !touches.tapped_on(Side::Right));
        // With the screen's width unknown, every finger is on the left.
        let unknown = Touches::default();
        assert_eq!(unknown.side_of(Vec2::new(5000.0, 0.0)), Side::Left);
    }

    #[test]
    fn how_far_is_far_enough_can_be_changed() {
        let mut touches = Touches { swipe_distance: 100.0, ..Default::default() };
        touches.began(1, Vec2::ZERO);
        touches.ended(1, Vec2::new(80.0, 0.0));
        assert_eq!(touches.gestures(), [Gesture::Tap(Vec2::ZERO)]);
    }
}
