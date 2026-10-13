//! Woodsmoke from the chimneys and spray from the fountain, as the engine's particles.

use mira::{glam::Vec3, prelude::*};

/// The breeze, which carries the smoke off across the river.
pub const WIND: Vec3 = Vec3::new(0.8, 0.0, -0.6);

/// The particles there are: the engine's, with this demo's kinds of them.
pub type Smoke = Particles;

pub fn plugin(app: &mut App) {
    app.add_systems(Stage::Startup, |mut smoke: ResMut<Smoke>| smoke.wind = WIND);
}

pub trait Kinds {
    /// Spray thrown up where falling water strikes: a quick, pale burst of droplets and mist.
    fn spray(&mut self, at: Vec3);
    /// A breath of woodsmoke from a campfire or chimney.
    fn woodsmoke(&mut self, at: Vec3);
}

impl Kinds for Smoke {
    fn spray(&mut self, at: Vec3) {
        let (a, b, c) = (self.random(), self.random(), self.random());
        let angle = a * std::f32::consts::TAU;
        self.emit(Particle {
            position: at + Vec3::new(angle.cos(), 0.0, angle.sin()) * 0.08,
            velocity: Vec3::new(angle.cos() * 0.6, 0.9 + b * 0.8, angle.sin() * 0.6),
            age: 0.0,
            life: 0.5 + c * 0.5,
            size: 0.07 + b * 0.05,
            spread: 2.5,
            color: Vec3::new(0.85, 0.88, 0.9),
            opacity: 0.35,
            rise: -0.6,
            drag: 2.5,
            rotation: angle,
            spin: (c - 0.5) * 2.0,
            layer: (a * 4.0) as u32 % 4,
            flash: false,
        });
    }

    fn woodsmoke(&mut self, at: Vec3) {
        let r = self.random();
        let mut p = self.puff(at, Vec3::new(r - 0.5, 0.8, 0.5 - r) * 0.4);
        p.size = 0.25;
        p.spread = 7.0;
        p.life = 16.0;
        p.color = Vec3::new(0.55, 0.55, 0.57);
        p.opacity = 0.35;
        p.rise = 0.9;
        p.drag = 0.5;
        self.emit(p);
    }
}