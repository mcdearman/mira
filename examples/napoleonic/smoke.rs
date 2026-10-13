//! Powder smoke, dust and woodsmoke, and muzzle flashes, as the engine's particles.

use mira::{glam::Vec3, prelude::*};

/// The breeze, which carries smoke north-east.
pub const WIND: Vec3 = Vec3::new(1.1, 0.0, -0.5);

/// The particles there are: the engine's, with this demo's kinds of them.
pub type Smoke = Particles;

pub fn plugin(app: &mut App) {
    app.add_systems(Stage::Startup, |mut smoke: ResMut<Smoke>| smoke.wind = WIND);
}

pub trait Kinds {
    fn musket(&mut self, muzzle: Vec3, forward: Vec3);
    fn cannon(&mut self, muzzle: Vec3, forward: Vec3);
    fn dust(&mut self, at: Vec3);
    fn woodsmoke(&mut self, at: Vec3);
}

impl Kinds for Smoke {
    /// The cloud a musket throws out: a jet from the muzzle and a puff from the pan.
    fn musket(&mut self, muzzle: Vec3, forward: Vec3) {
        for i in 0..5 {
            let r = self.random();
            let speed = 6.0 + i as f32 * 2.5 + r * 3.0;
            let jitter = Vec3::new(self.random() - 0.5, self.random() * 0.6, self.random() - 0.5) * 1.2;
            let mut p = self.puff(muzzle + forward * (i as f32 * 0.15), forward * speed + jitter);
            p.size = 0.25 + r * 0.15;
            p.spread = 5.0 + r * 3.0;
            p.life = 14.0 + r * 8.0;
            p.color = Vec3::splat(0.78 + r * 0.08);
            p.opacity = 0.75;
            p.drag = 1.6;
            self.emit(p);
        }
        let pan = muzzle - forward * 1.3 + Vec3::new(0.0, 0.05, 0.0);
        let mut p = self.puff(pan, Vec3::Y * 0.6);
        p.size = 0.15;
        p.spread = 3.0;
        p.life = 5.0;
        self.emit(p);
        let mut f = self.puff(muzzle + forward * 0.25, forward * 2.0);
        f.flash = true;
        f.size = 0.45;
        f.spread = 0.3;
        f.life = 0.07;
        f.color = Vec3::new(60.0, 32.0, 9.0);
        self.emit(f);
    }

    /// A field gun's discharge: a great bank of white smoke rolling out ahead.
    fn cannon(&mut self, muzzle: Vec3, forward: Vec3) {
        for i in 0..44 {
            let r = self.random();
            let speed = 4.0 + (i % 22) as f32 * 1.1 + r * 6.0;
            let jitter = Vec3::new(self.random() - 0.5, self.random() * 0.8, self.random() - 0.5) * 2.5;
            let mut p = self.puff(muzzle + forward * ((i % 22) as f32 * 0.2), forward * speed + jitter);
            p.size = 0.4 + r * 0.35;
            p.spread = 3.5 + r * 3.0;
            p.life = 20.0 + r * 12.0;
            p.color = Vec3::splat(0.76 + r * 0.1);
            p.opacity = 0.95;
            p.drag = 1.1;
            p.rise = 0.3;
            self.emit(p);
        }
        // The flash: a short tongue of flame out of the muzzle, brightest and widest at the
        // mouth, gone in a few frames.
        for k in 0..4 {
            let along = k as f32 * 0.45;
            let mut f = self.puff(muzzle + forward * (0.35 + along), forward * 8.0);
            f.flash = true;
            f.size = 0.55 - k as f32 * 0.1;
            f.spread = 0.6;
            f.life = 0.06 + self.random() * 0.03;
            f.color = Vec3::new(60.0, 28.0, 7.0) * (1.0 - k as f32 * 0.2);
            self.emit(f);
        }
    }

    /// Earth thrown up where a roundshot strikes.
    fn dust(&mut self, at: Vec3) {
        for _ in 0..10 {
            let r = self.random();
            let v = Vec3::new(self.random() - 0.5, 2.0 + r * 5.0, self.random() - 0.5) * 2.0;
            let mut p = self.puff(at, v);
            p.size = 0.5 + r * 0.4;
            p.spread = 4.0;
            p.life = 6.0 + r * 4.0;
            p.color = Vec3::new(0.42, 0.35, 0.26);
            p.opacity = 0.9;
            p.rise = -0.3;
            self.emit(p);
        }
    }

    /// A breath of woodsmoke from a campfire or chimney.
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
