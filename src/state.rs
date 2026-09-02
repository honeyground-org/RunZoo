//! What RunZoo knows, and how it changes over time. No windows, no menus.
use std::time::{Duration, Instant};

use crate::animal::{self, ANIMALS};
use crate::draw::{self, Buf};
use crate::metrics::{Metrics, Source};
use crate::prefs;
use crate::tint::{self, Rgb, PALETTE};

/// Stay above this for this long and we call it an overload
pub const OVERLOAD_PERCENT: f32 = 85.0;
pub const OVERLOAD_HOLD: Duration = Duration::from_secs(30);
/// Come back below this to clear the alarm (stops it chattering at the edge)
pub const RECOVER_PERCENT: f32 = 70.0;

/// Turn load into a frame interval (ms). Follows the original RunCat curve,
/// multiplied by each animal's gait so the elephant ambles and the squirrel
/// fusses.
pub fn interval_for(load: f32, tempo: f32) -> f64 {
    let speed = (load / 5.0).max(1.0) * tempo;
    (500.0 / speed as f64).clamp(33.0, 500.0)
}

/// What one second of measuring changed.
pub struct Tick {
    /// The animation timer needs re-arming at a new interval
    pub interval_changed: bool,
    /// An overload has been held long enough to be worth saying out loud
    pub alarm: Option<(String, String)>,
}

pub struct App {
    pub metrics: Metrics,
    pub source: Source,
    pub animal: usize,
    /// The current animal's frames, as packed 1-bit masks
    pub masks: &'static [&'static [u8]],
    pub frame: usize,
    pub interval: f64,
    pub alert_on: bool,
    over_since: Option<Instant>,
    alerted: bool,
    /// Index into tint::PALETTE
    pub accent: usize,
    /// The severity step the painted frames were made for
    pub level: u8,
    /// Whether anything has been painted yet
    painted: bool,
}

impl App {
    pub fn new() -> Self {
        let animal = animal::index_of(&prefs::load_str(prefs::K_ANIMAL).unwrap_or_else(|| "cat".into()));
        App {
            metrics: Metrics::new(),
            source: prefs::load_str(prefs::K_SOURCE)
                .and_then(|k| Source::from_key(&k))
                .unwrap_or(Source::Cpu),
            animal,
            masks: animal::masks(ANIMALS[animal].key),
            frame: 0,
            interval: 500.0,
            alert_on: prefs::load_bool(prefs::K_ALERT, true),
            over_since: None,
            alerted: false,
            accent: tint::accent_index(
                &prefs::load_str(prefs::K_ACCENT).unwrap_or_else(|| tint::DEFAULT_ACCENT.into()),
            ),
            level: 0,
            painted: false,
        }
    }

    pub fn accent_rgb(&self) -> Option<Rgb> {
        PALETTE[self.accent].rgb
    }

    /// What the animal is painted in right now.
    pub fn colour(&self) -> Rgb {
        tint::level_colour(self.level, tint::CALM, self.accent_rgb())
    }

    pub fn load(&self) -> f32 {
        self.metrics.latest(self.source)
    }

    /// What severity step the frames *should* be painted for. Monochrome pins
    /// it, because in that mode the system does the tinting.
    fn wanted_level(&self) -> u8 {
        if self.accent_rgb().is_some() {
            tint::level(self.load())
        } else {
            0
        }
    }

    /// Measure, and report what that changed.
    pub fn tick(&mut self) -> Tick {
        self.metrics.refresh();

        let load = self.load();
        let want = interval_for(load, ANIMALS[self.animal].tempo);
        let interval_changed = (want - self.interval).abs() > 1.0;
        self.interval = want;

        // Overload watch: alert once, and only after it has been held down
        let mut alarm = None;
        if self.alert_on {
            if load >= OVERLOAD_PERCENT {
                let since = *self.over_since.get_or_insert_with(Instant::now);
                if !self.alerted && since.elapsed() >= OVERLOAD_HOLD {
                    self.alerted = true;
                    let culprit = self
                        .metrics
                        .top
                        .first()
                        .map(|p| format!("{} ({:.0}%)", p.name, p.cpu))
                        .unwrap_or_else(|| "unknown".into());
                    alarm = Some((
                        format!("{} overload {:.0}%", self.source.label(), load),
                        format!("High for over 30 seconds. Biggest user: {culprit}"),
                    ));
                }
            } else if load < RECOVER_PERCENT {
                self.over_since = None;
                self.alerted = false;
            }
        }

        Tick { interval_changed, alarm }
    }

    /// Does the animal need repainting? Called once a second, so it must answer
    /// no on the ticks where nothing moved, which is most of them.
    pub fn needs_repaint(&mut self, force: bool) -> bool {
        let want = self.wanted_level();
        if !force && self.painted && want == self.level {
            return false;
        }
        self.level = want;
        self.painted = true;
        true
    }

    /// Every frame of the current animal, painted in the current severity colour.
    pub fn frames(&self) -> Vec<Buf> {
        let colour = self.colour();
        self.masks.iter().map(|m| draw::sprite(m, colour)).collect()
    }

    pub fn advance_frame(&mut self) {
        self.frame = (self.frame + 1) % self.masks.len().max(1);
    }

    pub fn set_animal(&mut self, idx: usize) {
        if idx >= ANIMALS.len() {
            return;
        }
        self.animal = idx;
        self.frame = 0;
        self.masks = animal::masks(ANIMALS[idx].key);
        prefs::save_str(prefs::K_ANIMAL, ANIMALS[idx].key);
    }

    pub fn set_source(&mut self, idx: usize) {
        let Some(&s) = Source::ALL.get(idx) else { return };
        self.source = s;
        self.over_since = None;
        self.alerted = false;
        prefs::save_str(prefs::K_SOURCE, s.key());
    }

    pub fn set_accent(&mut self, idx: usize) {
        if idx >= PALETTE.len() {
            return;
        }
        self.accent = idx;
        prefs::save_str(prefs::K_ACCENT, PALETTE[idx].key);
    }

    pub fn toggle_alert(&mut self) {
        self.alert_on = !self.alert_on;
        self.over_since = None;
        self.alerted = false;
        prefs::save_bool(prefs::K_ALERT, self.alert_on);
    }
}
