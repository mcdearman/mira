//! The battle itself, as plain data stepped at a fixed rate. Nothing here draws or reads input:
//! the same steps run anywhere, a server included, and give the same battle.
//!
//! Ground is the x–z plane, metres. North is −z: the Gauls come from there, and the legion
//! faces it. An angle of facing `a` is the direction `(sin a, cos a)` in (x, z), so 0 faces
//! south (toward the viewer) and π faces north.

use mira::prelude::Vec2;
use std::f32::consts::PI;

/// How often the battle is stepped, a second.
pub const STEPS_A_SECOND: f32 = 20.0;
pub const DT: f32 = 1.0 / STEPS_A_SECOND;

/// How far the commander's voice carries: orders to cohorts nearer than this are heard at once;
/// further ones go by rider.
pub const VOICE: f32 = 45.0;
/// How far a general's presence steadies men.
pub const COMMANDER_REACH: f32 = 55.0;
pub const LEGATE_REACH: f32 = 32.0;
const RIDER_SPEED: f32 = 14.0;
/// Trumpet calls reach the whole legion, a moment after they are sounded.
const TRUMPET_DELAY: f32 = 1.5;

/// Where the lines stand at the start, as depth behind the front, and the gap between lines.
pub const LINE_GAP: f32 = 70.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Rome,
    Gaul,
}

pub fn facing_vector(a: f32) -> Vec2 {
    Vec2::new(a.sin(), a.cos())
}

/// The right hand of someone facing `f`.
pub fn right_of(f: Vec2) -> Vec2 {
    Vec2::new(-f.y, f.x)
}

fn angle_of(v: Vec2) -> f32 {
    v.x.atan2(v.y)
}

fn wrap(a: f32) -> f32 {
    (a + PI).rem_euclid(2.0 * PI) - PI
}

/// A small deterministic random number generator, so a battle replays exactly.
#[derive(Clone, Debug)]
pub struct Dice(u64);

impl Dice {
    pub fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }
    pub fn next(&mut self) -> f32 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 40) as f32 / (1u64 << 24) as f32
    }
    pub fn range(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * self.next()
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Order {
    /// Stand where it is.
    Hold,
    /// Go to a place and face a way.
    MoveTo { to: Vec2, facing: f32 },
    /// Go forward until it meets the enemy.
    Advance,
}

/// What can be asked of a cohort, or of a general.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Command {
    Move { to: Vec2, facing: f32 },
    Advance,
    Halt,
}

/// A body of men that moves and fights as one: a cohort, or a Gallic war band.
#[derive(Clone, Debug)]
pub struct Unit {
    pub name: String,
    pub side: Side,
    /// Which line of the triplex acies, 1 to 3; 0 for the Gauls.
    pub line: u8,
    /// The middle of its front rank.
    pub pos: Vec2,
    pub facing: f32,
    pub files: usize,
    /// Metres between files, and between ranks.
    pub spacing: f32,
    pub rank_gap: f32,
    /// How far each man may stand from his place in the ranks, metres.
    pub loose: f32,
    /// Who stands in each place of the formation, front rank first, file by file; a place
    /// is empty once its man falls and there is no one behind him to step up.
    pub places: Vec<Option<usize>>,
    /// How many are still standing.
    pub count: usize,
    pub start_men: usize,
    pub fatigue: f32,
    pub cohesion: f32,
    pub order: Order,
    pub routed: bool,
    pub pila: bool,
    /// Men owed to the enemy's blows, carried from step to step until a whole man falls.
    pub wounds: f32,
    /// Seconds in contact with the enemy since it was last out of it.
    pub contact: f32,
    /// Metres of front in contact this step.
    pub engaged: f32,
    /// Whether it is under a general's eye this step.
    pub steadied: f32,
    pub kills: usize,
}

impl Unit {
    pub fn width(&self) -> f32 {
        (self.files.max(1) - 1) as f32 * self.spacing
    }
    pub fn ranks(&self) -> usize {
        self.places.len().div_ceil(self.files.max(1))
    }
    pub fn depth(&self) -> f32 {
        self.ranks().max(1) as f32 * self.rank_gap
    }
    pub fn forward(&self) -> Vec2 {
        facing_vector(self.facing)
    }
    /// Where the man in the given place of the formation stands.
    pub fn slot(&self, place: usize) -> Vec2 {
        let file = place % self.files;
        let rank = place / self.files;
        let f = self.forward();
        let across = (file as f32 - (self.files as f32 - 1.0) * 0.5) * self.spacing;
        self.pos + right_of(f) * across - f * (rank as f32 * self.rank_gap)
    }
    /// Whether a point is within the formation (with a margin), for picking with the pointer.
    pub fn covers(&self, p: Vec2, margin: f32) -> bool {
        let d = p - self.pos;
        let f = self.forward();
        let ahead = d.dot(f);
        let across = d.dot(right_of(f));
        ahead < margin && ahead > -self.depth() - margin && across.abs() < self.width() * 0.5 + margin
    }
    pub fn strength(&self) -> f32 {
        self.count as f32 / self.start_men.max(1) as f32
    }
    pub fn alive(&self) -> bool {
        self.count > 0
    }
    pub fn fighting(&self) -> bool {
        self.engaged > 0.0
    }
}

/// One man of a unit.
#[derive(Clone, Debug)]
pub struct Man {
    pub unit: usize,
    pub pos: Vec2,
    /// Where he was at the last step, for drawing between steps.
    pub was: Vec2,
    pub facing: f32,
    pub alive: bool,
    /// Metres walked, which sets the walk's frame.
    pub walked: f32,
    /// Where in the swing of his weapon he is, while fighting.
    pub swing: f32,
    pub fighting: bool,
    pub moving: bool,
    /// His own small offset from his place, so the ranks are not ruled lines.
    pub offset: Vec2,
    pub speed: f32,
}

/// A general on horseback: Caesar, or one of his legates.
#[derive(Clone, Debug)]
pub struct Rider {
    pub name: String,
    pub commander: bool,
    pub pos: Vec2,
    pub was: Vec2,
    pub facing: f32,
    pub target: Option<Vec2>,
    pub walked: f32,
    pub moving: bool,
}

impl Rider {
    pub fn reach(&self) -> f32 {
        if self.commander {
            COMMANDER_REACH
        } else {
            LEGATE_REACH
        }
    }
}

/// An order on its way to a cohort, by rider or by trumpet.
#[derive(Clone, Debug)]
pub struct Message {
    pub unit: usize,
    pub command: Command,
    /// A rider's place, if it goes by rider; a trumpet call has none and arrives at `at`.
    pub rider: Option<Vec2>,
    pub at: f32,
}

/// A volley of pila in the air, to be drawn.
#[derive(Clone, Debug)]
pub struct Volley {
    pub from: Vec2,
    pub to: Vec2,
    pub width: f32,
    pub thrown: f32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    Fighting,
    Won,
    Lost,
}

#[derive(Clone, Debug)]
pub struct Battle {
    pub units: Vec<Unit>,
    pub men: Vec<Man>,
    pub riders: Vec<Rider>,
    pub messages: Vec<Message>,
    pub volleys: Vec<Volley>,
    pub time: f32,
    pub steps: u64,
    /// What has happened lately, newest last.
    pub news: Vec<(f32, String)>,
    pub outcome: Outcome,
    /// When the Gauls set off.
    pub gauls_go: f32,
    pub dice: Dice,
    /// Men fallen on the field: where, which side, and how they lie.
    pub fallen: Vec<usize>,
}

const NUMERALS: [&str; 10] = ["I", "II", "III", "IV", "V", "VI", "VII", "VIII", "IX", "X"];

impl Battle {
    /// A legion of ten cohorts in triplex acies, four, three and three, the cohorts of each line
    /// standing behind the gaps in the line before; and five Gallic war bands to the north.
    pub fn new(seed: u64, cohort_men: usize, band_men: usize) -> Self {
        let mut b = Battle {
            units: Vec::new(),
            men: Vec::new(),
            riders: Vec::new(),
            messages: Vec::new(),
            volleys: Vec::new(),
            time: 0.0,
            steps: 0,
            news: Vec::new(),
            outcome: Outcome::Fighting,
            gauls_go: 5.0,
            dice: Dice::new(seed),
            fallen: Vec::new(),
        };
        let files = 45.min(cohort_men.max(8) / 4);
        let width = (files - 1) as f32 * 0.9;
        // A gap as wide as a cohort between cohorts, so the next line can pass through.
        let pitch = width * 2.0;
        let lines: [(u8, &[f32]); 3] = [(1, &[-1.5, -0.5, 0.5, 1.5]), (2, &[-1.0, 0.0, 1.0]), (3, &[-1.5, 0.0, 1.5])];
        let mut n = 0;
        for (line, places) in lines {
            for &x in places {
                let pos = Vec2::new(x * pitch, (line as f32 - 1.0) * LINE_GAP);
                b.add_unit(format!("Cohort {}", NUMERALS[n]), Side::Rome, line, pos, PI, files, 0.9, 1.2, 0.08, cohort_men);
                n += 1;
            }
        }
        let band_files = 80.min(band_men / 6).max(10);
        for (k, x) in [-2.1f32, -1.05, 0.0, 1.05, 2.1].iter().enumerate() {
            let jitter = b.dice.range(-12.0, 12.0);
            let pos = Vec2::new(x * pitch * 0.95 + jitter, -190.0 + b.dice.range(-20.0, 12.0));
            let name = ["Helvetii", "Tigurini", "Boii", "Tulingi", "Verbigeni"][k].to_string();
            b.add_unit(name, Side::Gaul, 0, pos, 0.0, band_files, 1.0, 1.3, 0.45, band_men);
        }
        b.riders.push(Rider { name: "Caesar".into(), commander: true, pos: Vec2::new(0.0, LINE_GAP * 1.5), was: Vec2::ZERO, facing: PI, target: None, walked: 0.0, moving: false });
        b.riders.push(Rider { name: "Labienus".into(), commander: false, pos: Vec2::new(-pitch * 1.6, LINE_GAP * 0.55), was: Vec2::ZERO, facing: PI, target: None, walked: 0.0, moving: false });
        b.riders.push(Rider { name: "Crassus".into(), commander: false, pos: Vec2::new(pitch * 1.6, LINE_GAP * 0.55), was: Vec2::ZERO, facing: PI, target: None, walked: 0.0, moving: false });
        for r in &mut b.riders {
            r.was = r.pos;
        }
        b.say("The legion stands in triplex acies. The Gauls are forming to the north.");
        b
    }

    #[allow(clippy::too_many_arguments)]
    fn add_unit(&mut self, name: String, side: Side, line: u8, pos: Vec2, facing: f32, files: usize, spacing: f32, rank_gap: f32, loose: f32, count: usize) {
        let index = self.units.len();
        let mut unit = Unit {
            name,
            side,
            line,
            pos,
            facing,
            files,
            spacing,
            rank_gap,
            loose,
            places: Vec::new(),
            count,
            start_men: count,
            fatigue: 0.0,
            cohesion: 1.0,
            order: Order::Hold,
            routed: false,
            pila: side == Side::Rome,
            wounds: 0.0,
            contact: 0.0,
            engaged: 0.0,
            steadied: 0.0,
            kills: 0,
        };
        for place in 0..count {
            let offset = Vec2::new(self.dice.range(-1.0, 1.0), self.dice.range(-1.0, 1.0)) * loose;
            let at = unit.slot(place) + offset;
            let speed = if side == Side::Rome { 1.7 } else { self.dice.range(2.2, 2.9) };
            self.men.push(Man { unit: index, pos: at, was: at, facing, alive: true, walked: self.dice.range(0.0, 3.0), swing: self.dice.next(), fighting: false, moving: false, offset, speed });
            unit.places.push(Some(self.men.len() - 1));
        }
        self.units.push(unit);
    }

    pub fn say(&mut self, what: impl Into<String>) {
        self.news.push((self.time, what.into()));
        if self.news.len() > 6 {
            self.news.remove(0);
        }
    }

    pub fn commander(&self) -> &Rider {
        &self.riders[0]
    }

    /// Gives an order to a cohort: at once if the commander is near enough to shout it, or
    /// else by a rider who must reach it first.
    pub fn order(&mut self, unit: usize, command: Command) {
        let from = self.commander().pos;
        if self.units[unit].pos.distance(from) <= VOICE {
            self.messages.push(Message { unit, command, rider: None, at: self.time + 0.3 });
        } else {
            self.messages.push(Message { unit, command, rider: Some(from), at: f32::INFINITY });
        }
    }

    /// An order sounded on the trumpets: every cohort hears it together, a moment later.
    pub fn sound(&mut self, unit: usize, command: Command) {
        self.messages.push(Message { unit, command, rider: None, at: self.time + TRUMPET_DELAY });
    }

    /// The front line falls back through the gaps of the second, and the second comes forward
    /// through the gaps of the first: fresh men in front, and the checkerboard the other way
    /// about.
    pub fn relieve_the_line(&mut self) {
        let front: Vec<usize> = self.romans_in_line(1);
        let second: Vec<usize> = self.romans_in_line(2);
        if front.is_empty() || second.is_empty() {
            self.say("There is no second line to relieve the first.");
            return;
        }
        let front_z = front.iter().map(|&u| self.units[u].pos.y).sum::<f32>() / front.len() as f32;
        for &u in &front {
            let p = self.units[u].pos;
            self.sound(u, Command::Move { to: Vec2::new(p.x, front_z + LINE_GAP), facing: PI });
            self.units[u].line = 2;
        }
        for &u in &second {
            let p = self.units[u].pos;
            self.sound(u, Command::Move { to: Vec2::new(p.x, front_z), facing: PI });
            self.units[u].line = 1;
        }
        self.say("Trumpets: the line is relieved. The second line goes forward through the gaps.");
    }

    pub fn romans_in_line(&self, line: u8) -> Vec<usize> {
        (0..self.units.len()).filter(|&u| self.units[u].side == Side::Rome && self.units[u].line == line && !self.units[u].routed && self.units[u].alive()).collect()
    }

    fn apply(&mut self, unit: usize, command: Command) {
        let u = &mut self.units[unit];
        if u.routed {
            return;
        }
        u.order = match command {
            Command::Move { to, facing } => Order::MoveTo { to, facing },
            Command::Advance => Order::Advance,
            Command::Halt => Order::Hold,
        };
    }

    /// One step of the battle.
    pub fn step(&mut self) {
        self.time += DT;
        self.steps += 1;
        self.deliver();
        self.gallic_plans();
        self.contacts();
        self.volleys();
        self.fight();
        self.move_units();
        self.move_men();
        self.move_riders();
        self.judge();
    }

    fn deliver(&mut self) {
        let mut arrived = Vec::new();
        let time = self.time;
        for (i, m) in self.messages.iter_mut().enumerate() {
            match &mut m.rider {
                Some(at) => {
                    let to = self.units[m.unit].pos;
                    let d = to - *at;
                    let step = RIDER_SPEED * DT;
                    if d.length() <= step + 2.0 {
                        arrived.push(i);
                    } else {
                        *at += d.normalize() * step;
                    }
                }
                None => {
                    if time >= m.at {
                        arrived.push(i);
                    }
                }
            }
        }
        for &i in arrived.iter().rev() {
            let m = self.messages.remove(i);
            self.apply(m.unit, m.command);
        }
    }

    /// The Gauls charge the nearest cohort they can see, and follow whatever falls back.
    fn gallic_plans(&mut self) {
        if self.time < self.gauls_go {
            return;
        }
        if self.time - DT < self.gauls_go {
            self.say("The Gauls are coming on!");
        }
        for g in 0..self.units.len() {
            if self.units[g].side != Side::Gaul || self.units[g].routed || !self.units[g].alive() {
                continue;
            }
            let gp = self.units[g].pos;
            // Straight at whatever stands across its path: the nearest cohort to either side,
            // the front line before those behind it.
            let target = (0..self.units.len())
                .filter(|&r| self.units[r].side == Side::Rome && !self.units[r].routed && self.units[r].alive())
                .min_by(|&a, &b| {
                    let cost = |r: usize| {
                        let d = self.units[r].pos - gp;
                        d.x.abs() * 1.5 + d.y.abs() * 0.5 + self.units[r].line as f32 * 30.0
                    };
                    cost(a).total_cmp(&cost(b))
                });
            let Some(r) = target else {
                self.units[g].order = Order::Hold;
                continue;
            };
            let ru = &self.units[r];
            let f = ru.forward();
            // Meet its front face to face, keeping to its own side of the field: a war band
            // wider than a cohort spills past its ends.
            let reach = (ru.width() + self.units[g].width()) * 0.5 - 4.0;
            let across = (gp - ru.pos).dot(right_of(f)).clamp(-reach, reach);
            let to = ru.pos + f * 1.4 + right_of(f) * across;
            let facing = wrap(ru.facing + PI);
            self.units[g].order = Order::MoveTo { to, facing };
        }
    }

    /// Which enemy fronts touch, and over how many metres.
    fn contacts(&mut self) {
        for u in &mut self.units {
            u.engaged = 0.0;
        }
        let n = self.units.len();
        for a in 0..n {
            for b in 0..n {
                let (ua, ub) = (&self.units[a], &self.units[b]);
                if ua.side != Side::Rome || ub.side != Side::Gaul || ua.routed || ub.routed || !ua.alive() || !ub.alive() {
                    continue;
                }
                let overlap = front_overlap(ua, ub);
                if overlap > 0.0 {
                    self.units[a].engaged += overlap;
                    self.units[b].engaged += overlap;
                }
            }
        }
        for u in &mut self.units {
            if u.engaged > 0.0 {
                u.contact += DT;
            } else {
                u.contact = 0.0;
            }
        }
    }

    /// Each cohort that still has its pila throws them once, as a war band comes within range.
    fn volleys(&mut self) {
        self.volleys.retain(|v| self.time - v.thrown < 1.4);
        let n = self.units.len();
        for r in 0..n {
            if !self.units[r].pila || self.units[r].routed || self.units[r].side != Side::Rome || self.units[r].line != 1 {
                continue;
            }
            for g in 0..n {
                let (ru, gu) = (&self.units[r], &self.units[g]);
                if gu.side != Side::Gaul || gu.routed || !gu.alive() {
                    continue;
                }
                let d = gu.pos - ru.pos;
                let ahead = d.dot(ru.forward());
                let across = d.dot(right_of(ru.forward())).abs();
                if ahead > 8.0 && ahead < 26.0 && across < (ru.width() + gu.width()) * 0.5 {
                    let (from, to, width) = (ru.pos, gu.pos, ru.width());
                    let name = ru.name.clone();
                    let gname = gu.name.clone();
                    self.units[r].pila = false;
                    self.volleys.push(Volley { from, to, width, thrown: self.time });
                    let hit = ((self.units[g].count as f32) * self.dice.range(0.05, 0.08)) as usize;
                    for _ in 0..hit {
                        self.kill_in_front(g, 3);
                    }
                    self.units[r].kills += hit;
                    self.units[g].cohesion -= 0.12;
                    self.say(format!("{name} looses its pila: {hit} of the {gname} fall."));
                    break;
                }
            }
        }
    }

    fn fight(&mut self) {
        let mut steadied = vec![0.0f32; self.units.len()];
        for r in &self.riders {
            for (i, u) in self.units.iter().enumerate() {
                if u.side == Side::Rome && u.pos.distance(r.pos) <= r.reach() {
                    let s = if r.commander { 1.0 } else { 0.5 };
                    steadied[i] = steadied[i].max(s);
                }
            }
        }
        for (i, u) in self.units.iter_mut().enumerate() {
            u.steadied = steadied[i];
        }
        let n = self.units.len();
        let mut blows = vec![0.0f32; n];
        for ua in &self.units {
            for (ub, blow) in self.units.iter().zip(blows.iter_mut()) {
                if ua.side == ub.side || ua.routed || ub.routed || !ua.alive() || !ub.alive() {
                    continue;
                }
                let (r, g) = if ua.side == Side::Rome { (ua, ub) } else { (ub, ua) };
                let overlap = front_overlap(r, g);
                if overlap <= 0.0 {
                    continue;
                }
                // What a's men do to b's in a step, along the front they share.
                let skill = if ua.side == Side::Rome { 1.35 } else { 1.0 };
                // The first shock of a Gallic charge is the worst of it.
                let impetus = if ua.side == Side::Gaul && ua.contact < 8.0 { 1.7 } else { 1.0 };
                let tired = 1.0 - 0.7 * ua.fatigue;
                let heart = 0.35 + 0.65 * ua.cohesion.max(0.0);
                *blow += DT * overlap * 0.032 * skill * impetus * tired * heart;
            }
        }
        for (unit, blow) in self.units.iter_mut().zip(blows) {
            if unit.routed || !unit.alive() {
                continue;
            }
            let steady = unit.steadied;
            if unit.fighting() {
                let rate = if unit.side == Side::Rome { 1.0 / 95.0 } else { 1.0 / 75.0 };
                unit.fatigue = (unit.fatigue + DT * rate * (1.0 - 0.35 * steady)).min(1.0);
                if unit.fatigue > 0.8 {
                    unit.cohesion -= DT * 0.012 * (1.0 - 0.5 * steady);
                }
            } else {
                unit.fatigue = (unit.fatigue - DT / 60.0).max(0.0);
                unit.cohesion = (unit.cohesion + DT * (0.004 + 0.01 * steady)).min(1.0);
            }
            unit.wounds += blow;
        }
        for u in 0..n {
            let mut fell = 0;
            while self.units[u].wounds >= 1.0 && self.units[u].alive() {
                self.units[u].wounds -= 1.0;
                self.kill_in_front(u, 1);
                fell += 1;
            }
            if fell > 0 {
                let steady = self.units[u].steadied;
                let start = self.units[u].start_men.max(1) as f32;
                self.units[u].cohesion -= fell as f32 / start * 2.4 * (1.0 - 0.4 * steady);
            }
        }
        // Those who break, run.
        for u in 0..n {
            let unit = &self.units[u];
            if unit.routed || !unit.alive() || unit.cohesion > 0.15 {
                continue;
            }
            let name = unit.name.clone();
            let side = unit.side;
            let unit = &mut self.units[u];
            unit.routed = true;
            unit.loose = 3.0;
            let away = if side == Side::Gaul { -1.0 } else { 1.0 };
            let x = unit.pos.x;
            unit.order = Order::MoveTo { to: Vec2::new(x * 1.2, unit.pos.y + away * 500.0), facing: if side == Side::Gaul { PI } else { 0.0 } };
            self.say(if side == Side::Gaul { format!("The {name} break and run!") } else { format!("{name} breaks!") });
        }
    }

    /// One man of the front ranks falls, and the men behind him in his file each step up a
    /// rank.
    fn kill_in_front(&mut self, u: usize, ranks: usize) {
        let unit = &self.units[u];
        let front: Vec<usize> = (0..(unit.files * ranks).min(unit.places.len())).filter(|&k| unit.places[k].is_some()).collect();
        if front.is_empty() {
            return;
        }
        let k = front[(self.dice.next() * front.len() as f32) as usize % front.len()];
        let unit = &mut self.units[u];
        let man = unit.places[k].take().expect("a man stands there");
        let mut j = k;
        while j + unit.files < unit.places.len() {
            unit.places[j] = unit.places[j + unit.files];
            j += unit.files;
        }
        unit.places[j] = None;
        while unit.places.last() == Some(&None) {
            unit.places.pop();
        }
        unit.count -= 1;
        self.men[man].alive = false;
        self.men[man].fighting = false;
        self.men[man].moving = false;
        self.fallen.push(man);
    }

    fn move_units(&mut self) {
        for u in &mut self.units {
            if !u.alive() {
                continue;
            }
            let speed = if u.routed {
                3.2
            } else if u.side == Side::Gaul {
                1.7
            } else {
                1.1
            };
            match u.order {
                Order::Hold => {}
                Order::Advance => {
                    if !u.fighting() {
                        u.pos += u.forward() * speed * DT;
                    }
                }
                Order::MoveTo { to, facing } => {
                    let d = to - u.pos;
                    let dist = d.length();
                    // A charge: faster over the last stretch.
                    let speed = if u.side == Side::Gaul && !u.routed && dist < 60.0 { 2.4 } else { speed };
                    let blocked = u.fighting() && !u.routed && d.dot(u.forward()) > 0.0;
                    if dist > 0.05 && !blocked {
                        u.pos += d * (speed * DT / dist).min(1.0);
                    }
                    let turn = wrap(facing - u.facing);
                    u.facing = wrap(u.facing + turn.clamp(-0.5 * DT, 0.5 * DT));
                    if dist <= 0.05 && turn.abs() < 0.01 && u.side == Side::Rome && !u.routed {
                        u.order = Order::Hold;
                    }
                }
            }
        }
    }

    fn move_men(&mut self) {
        for u in &self.units {
            let fighting_ranks = if u.fighting() { 2 } else { 0 };
            for (place, m) in u.places.iter().enumerate() {
                let Some(&m) = m.as_ref() else { continue };
                let man = &mut self.men[m];
                man.was = man.pos;
                let slot = u.slot(place) + man.offset * (u.loose / 0.45).clamp(0.2, 6.0);
                let d = slot - man.pos;
                let dist = d.length();
                let speed = if u.routed { 3.4 } else { man.speed };
                let step = speed * DT;
                if dist > 0.03 {
                    let mv = d * (step / dist).min(1.0);
                    man.pos += mv;
                    man.walked += mv.length();
                }
                man.moving = dist > 0.15;
                man.facing = if dist > 0.6 { angle_of(d) } else { u.facing };
                man.fighting = place / u.files < fighting_ranks && !u.routed && dist < 1.0;
                if man.fighting {
                    man.swing = (man.swing + DT * if u.side == Side::Rome { 1.1 } else { 0.9 }) % 1.0;
                }
            }
        }
    }

    fn move_riders(&mut self) {
        for r in &mut self.riders {
            r.was = r.pos;
            r.moving = false;
            if let Some(t) = r.target {
                let d = t - r.pos;
                let dist = d.length();
                if dist < 0.3 {
                    r.target = None;
                } else {
                    let mv = d * (4.5 * DT / dist).min(1.0);
                    r.pos += mv;
                    r.walked += mv.length();
                    r.facing = angle_of(d);
                    r.moving = true;
                }
            }
        }
    }

    fn judge(&mut self) {
        if self.outcome != Outcome::Fighting {
            return;
        }
        let gauls_left = self.units.iter().any(|u| u.side == Side::Gaul && !u.routed && u.alive());
        let romans_left = self.units.iter().any(|u| u.side == Side::Rome && !u.routed && u.alive());
        if !gauls_left {
            self.outcome = Outcome::Won;
            self.say("The Gauls are routed. Victory!");
        } else if !romans_left {
            self.outcome = Outcome::Lost;
            self.say("The legion is broken.");
        }
    }

    pub fn standing(&self, side: Side) -> usize {
        self.units.iter().filter(|u| u.side == side && !u.routed).map(|u| u.count).sum()
    }
}

/// How many metres of a Roman unit's front meet a Gallic unit's front, face to face.
fn front_overlap(r: &Unit, g: &Unit) -> f32 {
    let f = r.forward();
    let d = g.pos - r.pos;
    let ahead = d.dot(f);
    if !(-1.5..=3.2).contains(&ahead) {
        return 0.0;
    }
    // Facing each other, more or less.
    if f.dot(g.forward()) > -0.3 {
        return 0.0;
    }
    let across = d.dot(right_of(f));
    let (a0, a1) = (-r.width() * 0.5, r.width() * 0.5);
    let (b0, b1) = (across - g.width() * 0.5, across + g.width() * 0.5);
    (a1.min(b1) - a0.max(b0)).max(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_seed_gives_the_same_battle() {
        let mut a = Battle::new(7, 120, 300);
        let mut b = Battle::new(7, 120, 300);
        for _ in 0..1200 {
            a.step();
            b.step();
        }
        assert_eq!(a.standing(Side::Rome), b.standing(Side::Rome));
        assert_eq!(a.standing(Side::Gaul), b.standing(Side::Gaul));
        assert_eq!(a.men[5].pos, b.men[5].pos);
    }

    #[test]
    fn the_lines_stand_in_a_checkerboard() {
        let b = Battle::new(1, 120, 300);
        let first: Vec<f32> = b.romans_in_line(1).iter().map(|&u| b.units[u].pos.x).collect();
        let second: Vec<f32> = b.romans_in_line(2).iter().map(|&u| b.units[u].pos.x).collect();
        // Each cohort of the second line stands behind a gap of the first, as wide as a cohort.
        let w = b.units[0].width();
        for x in second {
            let nearest = first.iter().map(|f| (f - x).abs()).fold(f32::MAX, f32::min);
            assert!(nearest >= w * 0.99, "a second-line cohort stands behind a first-line one");
        }
    }

    #[test]
    fn relieving_the_line_swaps_the_first_and_second() {
        let mut b = Battle::new(1, 120, 300);
        b.gauls_go = f32::MAX;
        let first = b.romans_in_line(1);
        let second = b.romans_in_line(2);
        let front_z = b.units[first[0]].pos.y;
        b.relieve_the_line();
        for _ in 0..(STEPS_A_SECOND as usize * 120) {
            b.step();
        }
        for &u in &first {
            assert!((b.units[u].pos.y - (front_z + LINE_GAP)).abs() < 0.5);
            assert_eq!(b.units[u].line, 2);
        }
        for &u in &second {
            assert!((b.units[u].pos.y - front_z).abs() < 0.5);
            assert_eq!(b.units[u].line, 1);
        }
    }

    #[test]
    fn far_orders_go_by_rider() {
        let mut b = Battle::new(1, 60, 100);
        b.gauls_go = f32::MAX;
        // Caesar is behind the third line; cohort I is in the first, beyond his voice.
        b.order(0, Command::Advance);
        assert!(b.messages[0].rider.is_some());
        let mut steps = 0;
        while !b.messages.is_empty() {
            b.step();
            steps += 1;
            assert!(steps < 2000);
        }
        assert_eq!(b.units[0].order, Order::Advance);
        assert!(steps as f32 * DT > 5.0, "the rider took no time to arrive");
    }
}
