//! The cast. Frame artwork is produced by tools/gen_sprites.py and baked into
//! sprites.rs.
use crate::sprites::FRAMES;

pub struct Animal {
    pub key: &'static str,
    pub label: &'static str,
    /// Gait multiplier. At the same load an elephant ambles and a squirrel fusses.
    pub tempo: f32,
}

pub static ANIMALS: &[Animal] = &[
    Animal { key: "cat", label: "Cat", tempo: 1.00 },
    Animal { key: "dog", label: "Dog", tempo: 1.05 },
    Animal { key: "rattlesnake", label: "Rattlesnake", tempo: 0.85 },
    Animal { key: "squirrel", label: "Squirrel", tempo: 1.40 },
    Animal { key: "rabbit", label: "Rabbit", tempo: 1.20 },
    Animal { key: "elephant", label: "Elephant", tempo: 0.60 },
    Animal { key: "chicken", label: "Chicken", tempo: 1.30 },
];

pub fn index_of(key: &str) -> usize {
    ANIMALS.iter().position(|a| a.key == key).unwrap_or(0)
}

pub fn frames(key: &str) -> &'static [&'static [u8]] {
    FRAMES.iter().find(|(k, _)| *k == key).map(|(_, f)| *f).unwrap_or(FRAMES[0].1)
}
