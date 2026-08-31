//! 동물 목록. 프레임 그림은 tools/gen_sprites.py 가 만들고 sprites.rs 에 박힌다.
use crate::sprites::FRAMES;

pub struct Animal {
    pub key: &'static str,
    pub label: &'static str,
    /// 걸음 배속. 같은 부하라도 코끼리는 느긋하고 다람쥐는 부산하다.
    pub tempo: f32,
}

pub static ANIMALS: &[Animal] = &[
    Animal { key: "cat", label: "고양이", tempo: 1.00 },
    Animal { key: "dog", label: "강아지", tempo: 1.05 },
    Animal { key: "rattlesnake", label: "방울뱀", tempo: 0.85 },
    Animal { key: "squirrel", label: "다람쥐", tempo: 1.40 },
    Animal { key: "rabbit", label: "토끼", tempo: 1.20 },
    Animal { key: "elephant", label: "코끼리", tempo: 0.60 },
    Animal { key: "chicken", label: "닭", tempo: 1.30 },
];

pub fn index_of(key: &str) -> usize {
    ANIMALS.iter().position(|a| a.key == key).unwrap_or(0)
}

pub fn frames(key: &str) -> &'static [&'static [u8]] {
    FRAMES.iter().find(|(k, _)| *k == key).map(|(_, f)| *f).unwrap_or(FRAMES[0].1)
}
