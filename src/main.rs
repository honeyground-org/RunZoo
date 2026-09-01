//! RunZoo — an animal runs across the menu bar as fast as the system is busy,
//! and wears the colour of how bad things are.
//!
//! Started from the idea in RunCat365 (Takuto Nakamura, Apache-2.0). All of the
//! code is new; the original is Windows-only, so there was nothing to reuse.
mod animal;
mod metrics;
mod render;
mod sprites;
mod tint;

use std::cell::RefCell;
use std::process::Command;
use std::time::{Duration, Instant};

use objc2::rc::Retained;
use objc2::runtime::{NSObjectProtocol, ProtocolObject, Sel};
use objc2::{
    define_class, msg_send, sel, DefinedClass, MainThreadMarker, MainThreadOnly,
};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSControlStateValueOff, NSControlStateValueOn,
    NSImage, NSMenu, NSMenuDelegate, NSMenuItem, NSStatusBar, NSStatusItem,
    NSVariableStatusItemLength,
};
use objc2_foundation::{NSObject, NSRunLoop, NSRunLoopCommonModes, NSString, NSTimer, NSUserDefaults};

use animal::ANIMALS;
use metrics::{Metrics, Source};
use tint::{Rgb, PALETTE};

/// Stay above this for this long and we call it an overload
const OVERLOAD_PERCENT: f32 = 85.0;
const OVERLOAD_HOLD: Duration = Duration::from_secs(30);
/// Come back below this to clear the alarm (stops it chattering at the edge)
const RECOVER_PERCENT: f32 = 70.0;

const K_ANIMAL: &str = "animal";
const K_SOURCE: &str = "source";
const K_ALERT: &str = "alertEnabled";
const K_ACCENT: &str = "accentColour";

// ---------------------------------------------------------------- preferences
fn defaults() -> Retained<NSUserDefaults> {
    NSUserDefaults::standardUserDefaults()
}

fn load_str(key: &str) -> Option<String> {
    defaults().stringForKey(&NSString::from_str(key)).map(|s| s.to_string())
}

fn save_str(key: &str, val: &str) {
    unsafe {
        defaults().setObject_forKey(Some(&*NSString::from_str(val)), &NSString::from_str(key))
    };
}

fn save_bool(key: &str, val: bool) {
    defaults().setBool_forKey(val, &NSString::from_str(key));
}

fn load_bool(key: &str, fallback: bool) -> bool {
    let d = defaults();
    let k = NSString::from_str(key);
    if d.objectForKey(&k).is_some() {
        d.boolForKey(&k)
    } else {
        fallback
    }
}

// ---------------------------------------------------------------- state
struct App {
    metrics: Metrics,
    source: Source,
    animal: usize,
    /// Coverage of each animation frame, decoded once per animal. Recolouring
    /// is then a pixel loop over these rather than a PNG decode.
    masks: Vec<Option<render::Mask>>,
    frames: Vec<Retained<NSImage>>,
    frame: usize,
    interval: f64,
    alert_on: bool,
    over_since: Option<Instant>,
    alerted: bool,
    /// Index into tint::PALETTE
    accent: usize,
    /// The severity step the current frames were painted for
    level: u8,
}

impl App {
    fn accent_rgb(&self) -> Option<Rgb> {
        PALETTE[self.accent].rgb
    }

    /// The calm end of the gradient.
    fn base(&self) -> Rgb {
        tint::CALM
    }

    /// What the animal is painted in right now.
    fn colour(&self) -> Rgb {
        tint::level_colour(self.level, self.base(), self.accent_rgb())
    }

    /// What severity step the frames *should* be painted for. Monochrome pins
    /// it, because in that mode macOS does the tinting.
    fn wanted_level(&self, load: f32) -> u8 {
        if self.accent_rgb().is_some() {
            tint::level(load)
        } else {
            0
        }
    }
}

/// Turn load into a frame interval (ms). Follows the original RunCat curve,
/// multiplied by each animal's gait so the elephant ambles and the squirrel
/// fusses.
fn interval_for(load: f32, tempo: f32) -> f64 {
    let speed = (load / 5.0).max(1.0) * tempo;
    (500.0 / speed as f64).clamp(33.0, 500.0)
}

fn load_masks(key: &str) -> Vec<Option<render::Mask>> {
    animal::frames(key).iter().map(|png| render::alpha_mask(png)).collect()
}

/// Repaint every frame of the current animal in the current severity colour.
fn build_frames(app: &App) -> Vec<Retained<NSImage>> {
    let colour = app.colour();
    // In monochrome the image stays a macOS template and the system tints it.
    let template = app.accent_rgb().is_none();
    animal::frames(ANIMALS[app.animal].key)
        .iter()
        .zip(app.masks.iter())
        .map(|(png, mask)| match mask {
            Some(m) => render::sprite(m, colour, template),
            // Mask unreadable: fall back to the plain template sprite. Colour is
            // lost for that frame, the animal is not.
            None => render::sprite_from_png(png),
        })
        .collect()
}

fn notify(title: &str, body: &str) {
    fn quote(s: &str) -> String {
        format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
    }
    let _ = Command::new("osascript")
        .arg("-e")
        .arg(format!(
            "display notification {} with title {}",
            quote(body),
            quote(title)
        ))
        .spawn();
}

// ---------------------------------------------------------------- controller
struct Ivars {
    item: Retained<NSStatusItem>,
    app: RefCell<App>,
    timer: RefCell<Option<Retained<NSTimer>>>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "RunZooController"]
    #[ivars = Ivars]
    struct Controller;

    unsafe impl NSObjectProtocol for Controller {}

    unsafe impl NSMenuDelegate for Controller {
        /// Rebuild the whole menu just before it opens. Twenty-odd items is cheap.
        #[unsafe(method(menuNeedsUpdate:))]
        fn menu_needs_update(&self, menu: &NSMenu) {
            self.build_menu(menu);
        }
    }

    impl Controller {
        #[unsafe(method(animate:))]
        fn animate(&self, _t: *mut NSObject) {
            let iv = self.ivars();
            let mut app = iv.app.borrow_mut();
            app.frame = (app.frame + 1) % app.frames.len();
            let img = app.frames[app.frame].clone();
            drop(app);
            if let Some(btn) = iv.item.button(MainThreadMarker::from(self)) {
                btn.setImage(Some(&img));
            }
        }

        #[unsafe(method(fetch:))]
        fn fetch(&self, _t: *mut NSObject) {
            let iv = self.ivars();
            let mut app = iv.app.borrow_mut();
            app.metrics.refresh();

            let load = app.metrics.latest(app.source);
            let tempo = ANIMALS[app.animal].tempo;
            let want = interval_for(load, tempo);
            let changed = (want - app.interval).abs() > 1.0;
            app.interval = want;

            // Overload watch: alert once, and only after it has been held down
            let mut alarm: Option<(String, String)> = None;
            if app.alert_on {
                if load >= OVERLOAD_PERCENT {
                    let since = *app.over_since.get_or_insert_with(Instant::now);
                    if !app.alerted && since.elapsed() >= OVERLOAD_HOLD {
                        app.alerted = true;
                        let culprit = app
                            .metrics
                            .top
                            .first()
                            .map(|p| format!("{} ({:.0}%)", p.name, p.cpu))
                            .unwrap_or_else(|| "unknown".into());
                        alarm = Some((
                            format!("{} overload {:.0}%", app.source.label(), load),
                            format!("High for over 30 seconds. Biggest user: {culprit}"),
                        ));
                    }
                } else if load < RECOVER_PERCENT {
                    app.over_since = None;
                    app.alerted = false;
                }
            }
            drop(app);

            self.repaint();
            if let Some((t, b)) = alarm {
                notify(&t, &b);
            }
            if changed {
                self.restart_animation();
            }
        }

        #[unsafe(method(pickAnimal:))]
        fn pick_animal(&self, sender: &NSMenuItem) {
            let idx = sender.tag() as usize;
            {
                let mut app = self.ivars().app.borrow_mut();
                app.animal = idx;
                app.frame = 0;
                app.masks = load_masks(ANIMALS[idx].key);
            }
            save_str(K_ANIMAL, ANIMALS[idx].key);
            self.repaint_now();
            self.restart_animation();
        }

        #[unsafe(method(pickSource:))]
        fn pick_source(&self, sender: &NSMenuItem) {
            let s = Source::ALL[sender.tag() as usize];
            let mut app = self.ivars().app.borrow_mut();
            app.source = s;
            app.over_since = None;
            app.alerted = false;
            drop(app);
            save_str(K_SOURCE, s.key());
            // A different source usually means a different severity right away.
            self.repaint();
        }

        #[unsafe(method(pickAccent:))]
        fn pick_accent(&self, sender: &NSMenuItem) {
            let idx = sender.tag() as usize;
            self.ivars().app.borrow_mut().accent = idx;
            save_str(K_ACCENT, PALETTE[idx].key);
            self.repaint_now();
        }

        #[unsafe(method(toggleAlert:))]
        fn toggle_alert(&self, _s: &NSMenuItem) {
            let mut app = self.ivars().app.borrow_mut();
            app.alert_on = !app.alert_on;
            app.over_since = None;
            app.alerted = false;
            let on = app.alert_on;
            drop(app);
            save_bool(K_ALERT, on);
        }

        #[unsafe(method(quit:))]
        fn quit(&self, _s: *mut NSObject) {
            NSApplication::sharedApplication(MainThreadMarker::from(self)).terminate(None);
        }
    }
);

impl Controller {
    fn new(mtm: MainThreadMarker, ivars: Ivars) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(ivars);
        unsafe { msg_send![super(this), init] }
    }

    /// Repaint the animal if the severity step or the menu bar appearance moved.
    /// Called once a second, so it must do nothing on the ticks where nothing
    /// changed — which is most of them.
    fn repaint(&self) {
        self.paint(false);
    }

    /// Repaint no matter what. For when the animal or the colour was just picked.
    fn repaint_now(&self) {
        self.paint(true);
    }

    fn paint(&self, force: bool) {
        let iv = self.ivars();
        let mut app = iv.app.borrow_mut();
        let load = app.metrics.latest(app.source);
        let want_level = app.wanted_level(load);
        if !force && !app.frames.is_empty() && want_level == app.level {
            return;
        }
        app.level = want_level;
        let frames = build_frames(&app);
        app.frames = frames;
        if app.frame >= app.frames.len() {
            app.frame = 0;
        }
        let img = app.frames[app.frame].clone();
        drop(app);
        if let Some(btn) = iv.item.button(MainThreadMarker::from(self)) {
            btn.setImage(Some(&img));
        }
    }

    /// Re-arm the timer when the frame interval changes. It has to go in the
    /// common modes or the animal freezes while the menu is open.
    fn restart_animation(&self) {
        let iv = self.ivars();
        if let Some(old) = iv.timer.borrow_mut().take() {
            old.invalidate();
        }
        let secs = iv.app.borrow().interval / 1000.0;
        let t = unsafe {
            NSTimer::timerWithTimeInterval_target_selector_userInfo_repeats(
                secs,
                self,
                sel!(animate:),
                None,
                true,
            )
        };
        unsafe { NSRunLoop::currentRunLoop().addTimer_forMode(&t, NSRunLoopCommonModes) };
        *iv.timer.borrow_mut() = Some(t);
    }

    fn build_menu(&self, menu: &NSMenu) {
        let mtm = MainThreadMarker::from(self);
        let app = self.ivars().app.borrow();
        let base = tint::CALM;
        let accent = app.accent_rgb();
        menu.removeAllItems();

        menu.addItem(&header(mtm, "Load — click a row to drive the animal"));
        for s in Source::ALL {
            if !app.metrics.available[s.idx()] {
                continue;
            }
            let title = format!("{}   {}", s.label(), app.metrics.detail[s.idx()]);
            let it = item(mtm, &title, Some(sel!(pickSource:)), self);
            it.setTag(s.idx() as isize);
            it.setImage(Some(&render::sparkline(&app.metrics.hist[s.idx()], base, accent)));
            it.setState(if s == app.source {
                NSControlStateValueOn
            } else {
                NSControlStateValueOff
            });
            menu.addItem(&it);
        }

        menu.addItem(&NSMenuItem::separatorItem(mtm));
        menu.addItem(&header(mtm, "Top processes"));
        if app.metrics.top.is_empty() {
            menu.addItem(&header(mtm, "   measuring…"));
        }
        for p in app.metrics.top.iter().take(5) {
            let mb = p.mem as f64 / 1024.0 / 1024.0;
            menu.addItem(&header(
                mtm,
                &format!("   {}   {:.0}%   {:.0} MB", p.name, p.cpu, mb),
            ));
        }

        menu.addItem(&NSMenuItem::separatorItem(mtm));

        let zoo = NSMenu::new(mtm);
        for (i, a) in ANIMALS.iter().enumerate() {
            let it = item(mtm, a.label, Some(sel!(pickAnimal:)), self);
            it.setTag(i as isize);
            it.setState(if i == app.animal {
                NSControlStateValueOn
            } else {
                NSControlStateValueOff
            });
            zoo.addItem(&it);
        }
        let zoo_item = item(mtm, &format!("Animal — {}", ANIMALS[app.animal].label), None, self);
        zoo_item.setSubmenu(Some(&zoo));
        menu.addItem(&zoo_item);

        // Colour palette. Every entry carries its own ramp, so you pick the
        // gradient you can see rather than the name of a colour.
        let colours = NSMenu::new(mtm);
        colours.addItem(&header(mtm, "idle    →    overloaded"));
        for (i, a) in PALETTE.iter().enumerate() {
            let it = item(mtm, a.label, Some(sel!(pickAccent:)), self);
            it.setTag(i as isize);
            it.setImage(Some(&render::swatch(base, a.rgb)));
            it.setState(if i == app.accent {
                NSControlStateValueOn
            } else {
                NSControlStateValueOff
            });
            colours.addItem(&it);
        }
        let colour_item = item(
            mtm,
            &format!("Severity colour — {}", PALETTE[app.accent].label),
            None,
            self,
        );
        colour_item.setSubmenu(Some(&colours));
        menu.addItem(&colour_item);

        let alert = item(mtm, "Overload alert", Some(sel!(toggleAlert:)), self);
        alert.setState(if app.alert_on {
            NSControlStateValueOn
        } else {
            NSControlStateValueOff
        });
        menu.addItem(&alert);

        menu.addItem(&NSMenuItem::separatorItem(mtm));
        menu.addItem(&item(mtm, "Quit", Some(sel!(quit:)), self));
    }
}

fn item(
    mtm: MainThreadMarker,
    title: &str,
    action: Option<Sel>,
    target: &Controller,
) -> Retained<NSMenuItem> {
    let it = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &NSString::from_str(title),
            action,
            &NSString::from_str(""),
        )
    };
    if action.is_some() {
        unsafe { it.setTarget(Some(target)) };
    }
    it
}

/// A caption row you cannot click
fn header(mtm: MainThreadMarker, title: &str) -> Retained<NSMenuItem> {
    let it = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            &NSString::from_str(title),
            None,
            &NSString::from_str(""),
        )
    };
    it.setEnabled(false);
    it
}

fn new_app(source: Source, animal_idx: usize, accent: usize, metrics: Metrics) -> App {
    App {
        metrics,
        source,
        animal: animal_idx,
        masks: load_masks(ANIMALS[animal_idx].key),
        frames: Vec::new(),
        frame: 0,
        interval: 500.0,
        alert_on: load_bool(K_ALERT, true),
        over_since: None,
        alerted: false,
        accent,
        level: 0,
    }
}

// ---------------------------------------------------------------- dev flags
fn probe() {
    let mut m = Metrics::new();
    println!("sampling once a second (the first two rounds are warm-up, so throughput reads 0)");
    let n: u32 = std::env::args().nth(2).and_then(|s| s.parse().ok()).unwrap_or(2);
    let base = tint::CALM;
    let accent = PALETTE[tint::accent_index(
        &load_str(K_ACCENT).unwrap_or_else(|| tint::DEFAULT_ACCENT.into()),
    )]
    .rgb;
    for round in 1..=n {
        std::thread::sleep(Duration::from_secs(1));
        m.refresh();
        println!("\n--- round {round} ---");
        for s in Source::ALL {
            let mark = if m.available[s.idx()] { " " } else { "x" };
            let v = m.latest(s);
            println!(
                "{mark} {:<8} {:>6.1}%   sev {:>4.0}%  {}   {}",
                s.label(),
                v,
                tint::severity(v) * 100.0,
                tint::colour_for(v, base, accent).hex(),
                m.detail[s.idx()]
            );
        }
        println!("  top processes:");
        for p in m.top.iter().take(3) {
            println!("    {:<24} {:>5.1}%  {:>6.0} MB", p.name, p.cpu, p.mem as f64 / 1048576.0);
        }
        let tempo = ANIMALS[0].tempo;
        println!("  → frame interval on CPU: {:.0}ms", interval_for(m.latest(Source::Cpu), tempo));
    }
}

/// Print the whole colour ramp. Lets the gradient be checked without ever
/// opening the menu — or even having a menu bar.
fn dump_tint() {
    let steps = [0.0f32, 10.0, 25.0, 40.0, 50.0, 60.0, 70.0, 85.0, 95.0, 100.0];
    {
        let base = tint::CALM;
        println!("\ncalm end = {} (the same in both appearances)", base.hex());
        print!("{:<12}", "load %");
        for s in steps {
            print!("{:>9.0}", s);
        }
        println!();
        for a in PALETTE {
            print!("{:<12}", a.label);
            for s in steps {
                print!("{:>9}", tint::colour_for(s, base, a.rgb).hex());
            }
            println!();
        }
    }
    println!(
        "\nseverity(load) = (load/100)^1.6, quantised to {} steps for redraw",
        tint::LEVELS
    );
}

/// Build the menu for real and print it. Verifies the structure without a click.
fn dump_menu(mtm: MainThreadMarker) {
    let bar = NSStatusBar::systemStatusBar();
    let item_ = bar.statusItemWithLength(NSVariableStatusItemLength);
    let mut metrics = Metrics::new();
    for _ in 0..3 {
        std::thread::sleep(Duration::from_millis(1100));
        metrics.refresh();
    }
    let accent = tint::accent_index(
        &load_str(K_ACCENT).unwrap_or_else(|| tint::DEFAULT_ACCENT.into()),
    );
    let ctrl = Controller::new(
        mtm,
        Ivars {
            item: item_.clone(),
            app: RefCell::new(new_app(Source::Cpu, 0, accent, metrics)),
            timer: RefCell::new(None),
        },
    );
    ctrl.repaint_now();
    let menu = NSMenu::new(mtm);
    ctrl.build_menu(&menu);

    fn walk(menu: &NSMenu, indent: usize) {
        for i in 0..menu.numberOfItems() {
            let it = menu.itemAtIndex(i).unwrap();
            let mark = if it.state() == NSControlStateValueOn { "[v]" }
                       else if !it.isEnabled() { "   " } else { "[ ]" };
            let img = if it.image().is_some() { " [image]" } else { "" };
            let title = it.title().to_string();
            let title = if title.is_empty() { "────────".into() } else { title };
            println!("{}{mark} {title}{img}", "  ".repeat(indent + 1));
            if let Some(sub) = it.submenu() {
                walk(&sub, indent + 1);
            }
        }
    }
    println!("menu:");
    walk(&menu, 0);

    let app = ctrl.ivars().app.borrow();
    println!(
        "\npainting: accent {} · severity step {}/{} · animal {}",
        PALETTE[app.accent].label,
        app.level,
        tint::LEVELS - 1,
        app.colour().hex()
    );

    // Dump raw pixels so the sparklines can be looked at
    let base = app.base();
    let accent = app.accent_rgb();
    let mut raw = Vec::new();
    for s in Source::ALL {
        raw.extend_from_slice(&render::sparkline_buffer(&app.metrics.hist[s.idx()], base, accent));
    }
    std::fs::write("/tmp/runzoo_spark.raw", &raw).unwrap();
    println!(
        "sparkline pixels → /tmp/runzoo_spark.raw (120x28 RGBA, {} of them)",
        Source::ALL.len()
    );
    println!("view with: python3 tools/raw_to_png.py /tmp/runzoo_spark.raw 120 28");
}

/// What a full 60 seconds looks like, drawn from synthetic data.
fn dump_spark_demo() {
    use std::collections::VecDeque;
    /// A named synthetic load curve
    type Shape = (&'static str, fn(usize) -> f32);
    let shapes: [Shape; 4] = [
        ("sawtooth (load rising and falling)", |i| ((i as f32 * 0.35).sin() * 0.5 + 0.5) * 90.0 + 5.0),
        ("step (one jump, then held)", |i| if i < 30 { 15.0 } else { 78.0 }),
        ("spikes (short bursts)", |i| if i % 17 == 0 { 95.0 } else { 8.0 }),
        ("saturated (pinned at 100%)", |_| 100.0),
    ];
    let base = tint::CALM;
    let accent = PALETTE[tint::accent_index(
        &load_str(K_ACCENT).unwrap_or_else(|| tint::DEFAULT_ACCENT.into()),
    )]
    .rgb;
    let mut raw = Vec::new();
    for (name, f) in shapes {
        println!("  {name}");
        let h: VecDeque<f32> = (0..metrics::HISTORY).map(f).collect();
        raw.extend_from_slice(&render::sparkline_buffer(&h, base, accent));
    }
    std::fs::write("/tmp/runzoo_spark.raw", &raw).unwrap();
    println!("→ /tmp/runzoo_spark.raw (120x28 RGBA x4)");
    println!("view with: python3 tools/raw_to_png.py /tmp/runzoo_spark.raw 120 28");
}

/// Every animal, every frame, painted across the severity ramp. This is how the
/// colour work gets checked: one picture instead of staring at the menu bar.
fn dump_sprites() {
    let base = tint::CALM;
    let accent = PALETTE[tint::accent_index(
        &load_str(K_ACCENT).unwrap_or_else(|| tint::DEFAULT_ACCENT.into()),
    )]
    .rgb;
    let loads = [0.0f32, 25.0, 50.0, 70.0, 85.0, 100.0];
    let mut raw = Vec::new();
    let mut rows = 0;
    for a in ANIMALS {
        let masks = load_masks(a.key);
        let Some(Some(m)) = masks.first() else {
            println!("  {}: mask unreadable", a.key);
            continue;
        };
        // One row per animal: the same frame at rising severity, side by side.
        for y in 0..m.h {
            for load in loads {
                let c = tint::colour_for(load, base, accent);
                for x in 0..m.w {
                    let al = m.a[y * m.w + x];
                    let mul = |v: u8| ((v as u16 * al as u16) / 255) as u8;
                    raw.extend_from_slice(&[mul(c.0), mul(c.1), mul(c.2), al]);
                }
            }
        }
        rows += 1;
        println!("  {}: {}", a.key, loads.map(|l| tint::colour_for(l, base, accent).hex()).join(" "));
    }
    std::fs::write("/tmp/runzoo_sprites.raw", &raw).unwrap();
    let w = 40 * loads.len();
    println!("\nloads across each row: {loads:?}");
    println!("→ /tmp/runzoo_sprites.raw ({w}x{} RGBA)", 36 * rows);
    println!("view with: python3 tools/raw_to_png.py /tmp/runzoo_sprites.raw {w} {}", 36 * rows);
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let has = |f: &str| args.iter().any(|a| a == f);

    if has("--dump-tint") {
        dump_tint();
        return;
    }
    if has("--dump-spark-demo") {
        dump_spark_demo();
        return;
    }
    if has("--dump-sprites") {
        // Needs AppKit for the PNG decoder, but no window and no run loop.
        let _ = MainThreadMarker::new().expect("must be started on the main thread");
        dump_sprites();
        return;
    }
    if has("--dump-menu") {
        let mtm = MainThreadMarker::new().expect("must be started on the main thread");
        let app_ns = NSApplication::sharedApplication(mtm);
        app_ns.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
        dump_menu(mtm);
        return;
    }
    if has("--probe") {
        probe();
        return;
    }
    let mtm = MainThreadMarker::new().expect("must be started on the main thread");
    let app_ns = NSApplication::sharedApplication(mtm);
    // Accessory = no Dock icon, lives in the menu bar only
    app_ns.setActivationPolicy(NSApplicationActivationPolicy::Accessory);

    let animal_idx = animal::index_of(&load_str(K_ANIMAL).unwrap_or_else(|| "cat".into()));
    let source = load_str(K_SOURCE)
        .and_then(|k| Source::from_key(&k))
        .unwrap_or(Source::Cpu);
    let accent = tint::accent_index(
        &load_str(K_ACCENT).unwrap_or_else(|| tint::DEFAULT_ACCENT.into()),
    );

    let bar = NSStatusBar::systemStatusBar();
    // Must be variable length. Fixed length makes macOS fit every frame to the
    // slot, which took CPU from 8% to 26% at 40fps (measured, alternating x3).
    let item_ = bar.statusItemWithLength(NSVariableStatusItemLength);

    let ctrl = Controller::new(
        mtm,
        Ivars {
            item: item_.clone(),
            app: RefCell::new(new_app(source, animal_idx, accent, Metrics::new())),
            timer: RefCell::new(None),
        },
    );
    ctrl.repaint_now();

    let menu = NSMenu::new(mtm);
    menu.setDelegate(Some(ProtocolObject::from_ref(&*ctrl)));
    ctrl.build_menu(&menu);
    item_.setMenu(Some(&menu));

    ctrl.restart_animation();
    let fetch = unsafe {
        NSTimer::timerWithTimeInterval_target_selector_userInfo_repeats(
            1.0,
            &ctrl,
            sel!(fetch:),
            None,
            true,
        )
    };
    unsafe { NSRunLoop::currentRunLoop().addTimer_forMode(&fetch, NSRunLoopCommonModes) };

    app_ns.run();
}
