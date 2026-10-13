//! The game's pictures, drawn here a pixel at a time, and its sounds, made from nothing.
//!
//! A picture is rows of letters, each letter a colour and a dot nothing at all. It is made
//! into an image with every pixel several texels wide, so that it stays sharp-edged however
//! the card smooths what it draws.

use mira::prelude::*;

/// How many texels wide a pixel of a picture is made.
const TEXELS: u32 = 8;

/// Makes a picture from its rows. `colours` says what each letter is; a dot is see-through.
pub fn picture(rows: &[&str], colours: &[(char, [u8; 3])]) -> Image {
    let (wide, high) = (rows[0].len() as u32, rows.len() as u32);
    let mut data = vec![0u8; (wide * TEXELS * high * TEXELS * 4) as usize];
    for (y, row) in rows.iter().enumerate() {
        assert_eq!(row.len() as u32, wide, "every row of a picture is as wide as the first: {row}");
        for (x, letter) in row.chars().enumerate() {
            let Some((_, [r, g, b])) = colours.iter().find(|(known, _)| *known == letter) else {
                assert_eq!(letter, '.', "a letter with no colour");
                continue;
            };
            for ty in 0..TEXELS {
                for tx in 0..TEXELS {
                    let at = (((y as u32 * TEXELS + ty) * wide * TEXELS + x as u32 * TEXELS + tx) * 4) as usize;
                    data[at..at + 4].copy_from_slice(&[*r, *g, *b, 255]);
                }
            }
        }
    }
    Image::from_rgba(wide * TEXELS, high * TEXELS, data, true)
}

/// A number from nought up to one that is always the same for the same two numbers: what
/// scatters the specks on a tile.
fn scatter(a: u32, b: u32) -> f32 {
    let mut n = a.wrapping_mul(374_761_393).wrapping_add(b.wrapping_mul(668_265_263));
    n = (n ^ (n >> 13)).wrapping_mul(1_274_126_177);
    ((n ^ (n >> 16)) & 0xffff) as f32 / 65_536.0
}

/// A tile of ground sixteen pixels square: mostly `ground`, with specks of the two others.
/// `kind` gives a different scattering, so that tiles side by side are not all alike. With
/// `dash`, a road's lane marking runs along its upper edge for most of its width.
pub fn tile(kind: u32, ground: [u8; 3], dark: [u8; 3], light: [u8; 3], dash: bool) -> Image {
    let rows: Vec<String> = (0..16u32)
        .map(|y| {
            (0..16u32)
                .map(|x| {
                    let speck = scatter(x + kind * 97, y + kind * 31);
                    if dash && y == 0 && (2..12).contains(&x) {
                        'w'
                    } else if speck > 0.93 {
                        'd'
                    } else if speck < 0.06 {
                        'l'
                    } else {
                        'g'
                    }
                })
                .collect()
        })
        .collect();
    let rows: Vec<&str> = rows.iter().map(String::as_str).collect();
    picture(&rows, &[('g', ground), ('d', dark), ('l', light), ('w', [226, 222, 204])])
}

const FUR: [u8; 3] = [150, 84, 38];
const FUR_DARK: [u8; 3] = [104, 56, 26];
const TAIL: [u8; 3] = [196, 120, 52];
const CREAM: [u8; 3] = [236, 206, 150];
const INK: [u8; 3] = [24, 18, 16];
const BLOOD: [u8; 3] = [150, 22, 22];
const BLOOD_DARK: [u8; 3] = [96, 12, 16];

const SQUIRREL: &[(char, [u8; 3])] = &[('H', FUR), ('B', FUR), ('p', FUR_DARK), ('e', FUR_DARK), ('T', TAIL), ('t', CREAM), ('k', INK), ('n', [70, 40, 30])];

/// The squirrel from above, nose to the top: sitting, and stretched out in a hop.
pub fn squirrel() -> [Image; 2] {
    let sitting = [
        "................",
        "......e..e......",
        ".....eHnnHe.....",
        ".....HkHHkH.....",
        ".....HHHHHH.....",
        "......HHHH......",
        "....pBBBBBBp....",
        "....BBBBBBBB....",
        "....BBBBBBBB....",
        "....pBBBBBBp....",
        ".....BBBBBB.....",
        "......TTTT......",
        ".....TTtTTT.....",
        ".....TTttTT.....",
        "......TTTT......",
        "................",
    ];
    let hopping = [
        "......e..e......",
        ".....eHnnHe.....",
        ".....HkHHkH.....",
        "...p.HHHHHH.p...",
        "...pp.HHHH.pp...",
        "....pBBBBBBp....",
        ".....BBBBBB.....",
        ".....BBBBBB.....",
        "....pBBBBBBp....",
        "...pp.BBBB.pp...",
        "...p..TTTT..p...",
        "......TTTT......",
        "......TtTT......",
        "......TttT......",
        "......TTTT......",
        ".......TT.......",
    ];
    [picture(&sitting, SQUIRREL), picture(&hopping, SQUIRREL)]
}

/// What is left of it, a moment at a time: the splat spreading, and then lying there.
pub fn splat() -> [Image; 4] {
    let colours: &[(char, [u8; 3])] = &[('R', BLOOD), ('r', BLOOD_DARK), ('T', TAIL), ('t', CREAM), ('B', FUR), ('k', INK)];
    let frames: [[&str; 16]; 4] = [
        [
            "................",
            "................",
            "................",
            "................",
            "................",
            "......BBBB......",
            ".....BRRRRB.....",
            ".....BRRRRB.....",
            ".....BRRRRB.....",
            "......BBBB......",
            "......TTTT......",
            "......TttT......",
            "................",
            "................",
            "................",
            "................",
        ],
        [
            "................",
            "................",
            ".......R........",
            "....R...........",
            ".....RRRRRR..R..",
            "....RRRRRRRR....",
            "...RRRrBBrRRR...",
            "...RRRBkkBRRR...",
            "...RRRrBBrRRR...",
            "....RRRRRRRR....",
            ".R...RRRRRR.....",
            "......TTTT...R..",
            "......TttT......",
            "................",
            "................",
            "................",
        ],
        [
            "...R........R...",
            "......R..R......",
            ".R..RRRRRRRR....",
            "...RRRRRRRRRR..R",
            "..RRRRRRRRRRRR..",
            "..RRRrrRRrrRRR..",
            ".RRRRrBBBBrRRRR.",
            ".RRRRrBkkBrRRRR.",
            ".RRRRrBBBBrRRRR.",
            "..RRRrrRRrrRRR..",
            "..RRRRRRRRRRRR..",
            "R..RRRRTTRRRR...",
            "....RRTTtTRR..R.",
            "......TttT......",
            "..R....TT....R..",
            "................",
        ],
        [
            "...r........r...",
            "......r..r......",
            ".r..RRRRRRRR....",
            "...RRRrrrrRRR..r",
            "..RRrrrrrrrrRR..",
            "..RRrrrrrrrrRR..",
            ".RRrrrBBBBrrrRR.",
            ".RRrrrBkkBrrrRR.",
            ".RRrrrBBBBrrrRR.",
            "..RRrrrrrrrrRR..",
            "..RRRrrrrrrRRR..",
            "r..RRRRTTRRRR...",
            "....RRTTtTRR..r.",
            "......TttT......",
            "..r....TT....r..",
            "................",
        ],
    ];
    frames.map(|rows| picture(&rows, colours))
}

/// A car from above, driving to the right, in a paint of its own.
pub fn car(paint: [u8; 3]) -> Image {
    let roof = paint.map(|part| (part as u16 * 5 / 4).min(255) as u8);
    let shade = paint.map(|part| (part as u16 * 2 / 3) as u8);
    let rows = [
        "........................",
        "...kkk...........kkk....",
        ".sCCCCCCCCCCCCCCCCCCCs..",
        "sCCCCCGGGcccccGGGGCCCCY.",
        "sCCCCGGGGcccccGGGGGCCCCY",
        "rCCCCGGGGcccccGGGGGCCCC.",
        "rCCCCGGGGcccccGGGGGCCCC.",
        "sCCCCGGGGcccccGGGGGCCCCY",
        "sCCCCCGGGcccccGGGGCCCCY.",
        ".sCCCCCCCCCCCCCCCCCCCs..",
        "...kkk...........kkk....",
        "........................",
    ];
    picture(&rows, &[('C', paint), ('c', roof), ('s', shade), ('G', [44, 62, 82]), ('k', INK), ('Y', [255, 236, 150]), ('r', [200, 30, 30])])
}

/// A lorry from above, driving to the right: a cab in its paint and a pale box behind.
pub fn lorry(paint: [u8; 3]) -> Image {
    let shade = paint.map(|part| (part as u16 * 2 / 3) as u8);
    let rows = [
        "............................................",
        "...kkk....kkk..................kkk..........",
        ".bbbbbbbbbbbbbbbbbbbbbbbbbbbbbb.sCCCCCCCCs..",
        "bBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBbsCCCCGGGCCY.",
        "bBBBbBBBBBBBbBBBBBBBbBBBBBBBbBBbCCcccGGGGCCY",
        "rBBBbBBBBBBBbBBBBBBBbBBBBBBBbBBbCCcccGGGGCC.",
        "rBBBbBBBBBBBbBBBBBBBbBBBBBBBbBBbCCcccGGGGCC.",
        "bBBBbBBBBBBBbBBBBBBBbBBBBBBBbBBbCCcccGGGGCCY",
        "bBBBBBBBBBBBBBBBBBBBBBBBBBBBBBBbsCCCCGGGCCY.",
        ".bbbbbbbbbbbbbbbbbbbbbbbbbbbbbb.sCCCCCCCCs..",
        "...kkk....kkk..................kkk..........",
        "............................................",
    ];
    let roof = paint.map(|part| (part as u16 * 5 / 4).min(255) as u8);
    picture(
        &rows,
        &[('C', paint), ('c', roof), ('s', shade), ('B', [222, 220, 210]), ('b', [168, 166, 158]), ('G', [44, 62, 82]), ('k', INK), ('Y', [255, 236, 150]), ('r', [200, 30, 30])],
    )
}

/// A tree from above: a round head of leaves, lit from one side.
pub fn tree() -> Image {
    let rows = [
        "......dddd......",
        "....ddLLLLdd....",
        "...dLLLlLLLLd...",
        "..dLLlllLLLLLd..",
        ".dLLllLLLLLLLLd.",
        ".dLLlLLLLLLLLDd.",
        "dLLLLLLLLLLLLDDd",
        "dLLLLLLLLLLLDDDd",
        "dLLLLLLLLLLLDDDd",
        "dLLLLLLLLLLDDDDd",
        ".dLLLLLLLLDDDDd.",
        ".dLLLLLLDDDDDDd.",
        "..dLLLDDDDDDDd..",
        "...dDDDDDDDDd...",
        "....ddDDDDdd....",
        "......dddd......",
    ];
    picture(&rows, &[('L', [62, 140, 62]), ('l', [120, 188, 96]), ('D', [40, 104, 52]), ('d', [26, 70, 40])])
}

/// An acorn.
pub fn acorn() -> Image {
    let rows = [
        "...ss...",
        "..cccc..",
        ".cccccc.",
        ".nNNNNn.",
        ".nNNlNn.",
        ".nNNNNn.",
        "..nNNn..",
        "...nn...",
    ];
    picture(&rows, &[('s', [70, 44, 22]), ('c', [104, 66, 30]), ('N', [200, 140, 60]), ('n', [150, 98, 40]), ('l', [240, 200, 130])])
}

/// A sound made from a sum: `of` gives the wave at a moment, from nought to `seconds`.
fn made(seconds: f32, of: impl Fn(f32) -> f32) -> Sound {
    let rate = 44_100;
    Sound::new(rate, 1, (0..(seconds * rate as f32) as usize).map(|frame| of(frame as f32 / rate as f32).clamp(-1.0, 1.0)).collect())
}

/// A quick rising chirp: a hop.
pub fn hop_sound() -> Sound {
    made(0.07, |t| {
        let pitch = 520.0 + t / 0.07 * 420.0;
        (t * pitch * std::f32::consts::TAU).sin() * 0.4 * (1.0 - t / 0.07).min(t / 0.004).clamp(0.0, 1.0)
    })
}

/// Three rising notes run together: across the road.
pub fn across_sound() -> Sound {
    made(0.5, |t| {
        let (note, into) = ((t / 0.14).min(2.0).floor(), t % 0.14);
        let hertz = [523.25, 659.25, 783.99][note as usize];
        let fade = if note >= 2.0 { (1.0 - (t - 0.28) / 0.22).clamp(0.0, 1.0) } else { (1.0 - into / 0.14).clamp(0.2, 1.0) };
        ((t * hertz * std::f32::consts::TAU).sin() + 0.3 * (t * hertz * 2.0 * std::f32::consts::TAU).sin()) * 0.3 * fade
    })
}

/// A splat: a thump as the wheel comes down, a burst of wet noise that dulls as it dies away,
/// and a squelch under it.
pub fn splat_sound() -> Sound {
    let rate = 44_100usize;
    let frames = (0.45 * rate as f32) as usize;
    let (mut seed, mut dulled, mut samples) = (0x2545_f491u32, 0.0f32, Vec::with_capacity(frames));
    for frame in 0..frames {
        let t = frame as f32 / rate as f32;
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let noise = (seed >> 8) as f32 / (1u32 << 23) as f32 - 1.0;
        // Bright at first and duller with every moment: the noise is let through less and
        // less quickly.
        let through = 0.55 * (-t * 14.0).exp() + 0.04;
        dulled += (noise - dulled) * through;
        let wet = dulled * (-t * 9.0).exp() * 1.6;
        // The thump falls in pitch as it fades.
        let thump = (t * (120.0 - t * 160.0).max(45.0) * std::f32::consts::TAU).sin() * (-t * 16.0).exp() * 0.9;
        // The squelch: the dulled noise again, opening and closing thirty times a second.
        let squelch = dulled * (0.5 + 0.5 * (t * 30.0 * std::f32::consts::TAU).sin()) * (-(t - 0.08).abs() * 12.0).exp() * 1.2;
        samples.push((wet + thump + squelch).clamp(-1.0, 1.0) * 0.8);
    }
    Sound::new(rate as u32, 1, samples)
}

/// A low hum with a rattle in it: an engine.
pub fn engine_sound() -> Sound {
    made(1.0, |t| {
        let turn = t * std::f32::consts::TAU;
        ((turn * 64.0).sin() * 0.5 + (turn * 128.0).sin() * 0.25 + (turn * 192.0).sin() * 0.12) * (0.8 + 0.2 * (turn * 16.0).sin()) * 0.5
    })
}

/// The letters and figures the score board is written in, three pixels by five.
pub fn letter(of: char) -> Option<Image> {
    let rows: [&str; 5] = match of {
        '0' | 'O' => ["www", "w.w", "w.w", "w.w", "www"],
        '1' => [".w.", "ww.", ".w.", ".w.", "www"],
        '2' => ["www", "..w", "www", "w..", "www"],
        '3' => ["www", "..w", "www", "..w", "www"],
        '4' => ["w.w", "w.w", "www", "..w", "..w"],
        '5' => ["www", "w..", "www", "..w", "www"],
        '6' => ["www", "w..", "www", "w.w", "www"],
        '7' => ["www", "..w", ".w.", ".w.", ".w."],
        '8' => ["www", "w.w", "www", "w.w", "www"],
        '9' => ["www", "w.w", "www", "..w", "www"],
        'A' => [".w.", "w.w", "www", "w.w", "w.w"],
        'B' => ["ww.", "w.w", "ww.", "w.w", "ww."],
        'C' => [".ww", "w..", "w..", "w..", ".ww"],
        'D' => ["ww.", "w.w", "w.w", "w.w", "ww."],
        'E' => ["www", "w..", "ww.", "w..", "www"],
        'R' => ["ww.", "w.w", "ww.", "w.w", "w.w"],
        'S' => [".ww", "w..", ".w.", "..w", "ww."],
        'T' => ["www", ".w.", ".w.", ".w.", ".w."],
        _ => return None,
    };
    Some(picture(&rows, &[('w', [250, 244, 224])]))
}

/// The board the score is written on: dark, with a pale edge.
pub fn board() -> Image {
    let rows: Vec<String> = (0..12)
        .map(|y| (0..24).map(|x| if x == 0 || y == 0 || x == 23 || y == 11 { 'e' } else if (x + y) % 9 == 0 { 'd' } else { 'b' }).collect())
        .collect();
    let rows: Vec<&str> = rows.iter().map(String::as_str).collect();
    picture(&rows, &[('b', [30, 34, 48]), ('d', [38, 44, 62]), ('e', [236, 206, 150])])
}

/// The tune that plays behind the game: eight bars that go round, a plucked melody skipping
/// about over a bass that steps from foot to foot, with a tick on the off beats.
pub fn tune() -> Sound {
    const RATE: usize = 44_100;
    // A beat is a quarter note; the melody moves in halves of one.
    let beat = 60.0 / 132.0;
    let frames = (32.0 * beat * RATE as f32) as usize;
    let mut samples = vec![0.0f32; frames];
    // A note by its number, sixty-nine being the A above middle C.
    let hertz = |note: i32| 440.0 * 2f32.powf((note - 69) as f32 / 12.0);
    // Adds a note at a moment, running on past the end of the tune into its beginning, so
    // that it goes round without a seam.
    let mut sound = |at: f32, seconds: f32, wave: &dyn Fn(f32) -> f32| {
        let from = (at * RATE as f32) as usize;
        for frame in 0..(seconds * RATE as f32) as usize {
            samples[(from + frame) % frames] += wave(frame as f32 / RATE as f32);
        }
    };
    let turn = std::f32::consts::TAU;
    // The melody, eight to a bar; nought is a rest.
    let melody: [[i32; 8]; 8] = [
        [76, 79, 84, 79, 76, 79, 84, 0],
        [74, 77, 81, 77, 74, 77, 83, 0],
        [76, 79, 84, 88, 86, 84, 83, 81],
        [79, 0, 79, 81, 79, 0, 0, 0],
        [81, 84, 89, 84, 81, 84, 89, 0],
        [79, 83, 88, 83, 79, 83, 86, 0],
        [84, 83, 81, 79, 77, 76, 74, 77],
        [76, 0, 74, 0, 72, 0, 0, 0],
    ];
    for (bar, notes) in melody.iter().enumerate() {
        for (place, note) in notes.iter().enumerate() {
            if *note == 0 {
                continue;
            }
            let pitch = hertz(*note);
            // Plucked: bright at once and soon gone, with a little wobble as it rings.
            sound((bar * 8 + place) as f32 * beat * 0.5, 0.45, &|t| {
                let wobble = 1.0 + 0.004 * (t * 6.0 * turn).sin();
                let ring = (t * pitch * wobble * turn).sin() + 0.4 * (t * pitch * 2.0 * turn).sin() + 0.18 * (t * pitch * 3.0 * turn).sin();
                ring * (-t * 9.0).exp() * (t / 0.004).min(1.0) * 0.2
            });
        }
    }
    // The bass, four to a bar: from foot to foot.
    let bass: [[i32; 4]; 8] = [[48, 55, 48, 55], [43, 50, 43, 50], [48, 55, 45, 52], [43, 50, 43, 47], [41, 48, 41, 48], [40, 47, 43, 50], [41, 48, 43, 50], [48, 43, 48, 0]];
    for (bar, notes) in bass.iter().enumerate() {
        for (place, note) in notes.iter().enumerate() {
            if *note == 0 {
                continue;
            }
            let pitch = hertz(*note);
            sound((bar * 4 + place) as f32 * beat, 0.4, &|t| {
                let round = (t * pitch * turn).sin() + 0.25 * (t * pitch * 3.0 * turn).sin();
                round * (-t * 6.0).exp() * (t / 0.006).min(1.0) * 0.26
            });
        }
    }
    // A tick on the second and fourth beats, and a softer one between.
    for half in 0..64u32 {
        let loud = match half % 4 {
            2 => 0.1,
            1 | 3 => 0.03,
            _ => continue,
        };
        sound(half as f32 * beat * 0.5, 0.05, &|t| {
            let grit = scatter((t * RATE as f32) as u32, half) * 2.0 - 1.0;
            (grit * 0.6 + (t * 1800.0 * turn).sin() * 0.4) * (-t * 90.0).exp() * loud
        });
    }
    Sound::new(RATE as u32, 1, samples.into_iter().map(|sample| sample.clamp(-1.0, 1.0)).collect())
}
