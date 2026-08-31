//! RunZoo — 메뉴바에서 동물이 시스템 부하만큼 빨리 달린다.
//!
//! RunCat365 (Takuto Nakamura, Apache-2.0) 의 아이디어에서 출발했다.
//! 코드는 전부 새로 썼다. 원본은 Windows 전용이라 재사용할 수 있는 게 없었다.
mod animal;
mod metrics;
mod render;
mod sprites;

use std::cell::RefCell;
use std::process::Command;
use std::time::{Duration, Instant};

use objc2::rc::Retained;
use objc2::runtime::{NSObjectProtocol, ProtocolObject, Sel};
use objc2::{
    define_class, msg_send, sel, AllocAnyThread, DefinedClass, MainThreadMarker, MainThreadOnly,
};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSControlStateValueOff, NSControlStateValueOn,
    NSImage, NSMenu, NSMenuDelegate, NSMenuItem, NSStatusBar, NSStatusItem,
    NSVariableStatusItemLength,
};
use objc2_foundation::{
    NSData, NSObject, NSRunLoop, NSRunLoopCommonModes, NSSize, NSString, NSTimer, NSUserDefaults,
};

use animal::ANIMALS;
use metrics::{Metrics, Source};

/// 이 값을 이만큼 오래 넘고 있으면 과부하로 본다
const OVERLOAD_PERCENT: f32 = 85.0;
const OVERLOAD_HOLD: Duration = Duration::from_secs(30);
/// 여기 아래로 내려와야 경보를 푼다 (경계에서 알림이 떨리는 걸 막는다)
const RECOVER_PERCENT: f32 = 70.0;

const K_ANIMAL: &str = "animal";
const K_SOURCE: &str = "source";
const K_ALERT: &str = "alertEnabled";

// ---------------------------------------------------------------- 설정 저장
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

// ---------------------------------------------------------------- 상태
struct App {
    metrics: Metrics,
    source: Source,
    animal: usize,
    frames: Vec<Retained<NSImage>>,
    frame: usize,
    interval: f64,
    alert_on: bool,
    over_since: Option<Instant>,
    alerted: bool,
}

/// 부하를 프레임 간격(ms)으로 바꾼다. 원본 RunCat 의 곡선을 따르되
/// 동물마다 걸음 배속을 곱해서 코끼리는 느긋하고 다람쥐는 부산하게 만든다.
fn interval_for(load: f32, tempo: f32) -> f64 {
    let speed = (load / 5.0).max(1.0) * tempo;
    (500.0 / speed as f64).clamp(33.0, 500.0)
}

fn load_frames(key: &str) -> Vec<Retained<NSImage>> {
    animal::frames(key)
        .iter()
        .map(|png| {
            let data = NSData::with_bytes(png);
            let img = NSImage::initWithData(NSImage::alloc(), &data).expect("스프라이트 디코드 실패");
            // 40x36 픽셀을 20x18 포인트로 → 레티나에서 픽셀이 1:1 로 떨어진다
            img.setSize(NSSize::new(20.0, 18.0));
            img.setTemplate(true);
            img
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

// ---------------------------------------------------------------- 컨트롤러
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
        /// 메뉴가 열리기 직전마다 통째로 다시 짓는다. 항목이 스무 개 남짓이라 싸다.
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

            // 과부하 감시: 오래 눌려 있을 때만 한 번 알린다
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
                            .unwrap_or_else(|| "알 수 없음".into());
                        alarm = Some((
                            format!("{} 과부하 {:.0}%", app.source.label(), load),
                            format!("30초 넘게 높습니다. 가장 많이 쓰는 건 {culprit}"),
                        ));
                    }
                } else if load < RECOVER_PERCENT {
                    app.over_since = None;
                    app.alerted = false;
                }
            }
            drop(app);

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
            let iv = self.ivars();
            {
                let mut app = iv.app.borrow_mut();
                app.animal = idx;
                app.frame = 0;
                app.frames = load_frames(ANIMALS[idx].key);
                let img = app.frames[0].clone();
                drop(app);
                if let Some(btn) = iv.item.button(MainThreadMarker::from(self)) {
                    btn.setImage(Some(&img));
                }
            }
            save_str(K_ANIMAL, ANIMALS[idx].key);
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

    /// 프레임 간격이 바뀌면 타이머를 새로 건다. 공통 모드에 넣어야
    /// 메뉴를 열어둔 동안에도 동물이 계속 달린다.
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
        menu.removeAllItems();

        menu.addItem(&header(mtm, "부하 (줄을 누르면 그 값이 동물 속도가 됩니다)"));
        for s in Source::ALL {
            if !app.metrics.available[s.idx()] {
                continue;
            }
            let title = format!("{}   {}", s.label(), app.metrics.detail[s.idx()]);
            let it = item(mtm, &title, Some(sel!(pickSource:)), self);
            it.setTag(s.idx() as isize);
            it.setImage(Some(&render::sparkline(&app.metrics.hist[s.idx()])));
            it.setState(if s == app.source {
                NSControlStateValueOn
            } else {
                NSControlStateValueOff
            });
            menu.addItem(&it);
        }

        menu.addItem(&NSMenuItem::separatorItem(mtm));
        menu.addItem(&header(mtm, "많이 쓰는 프로세스"));
        if app.metrics.top.is_empty() {
            menu.addItem(&header(mtm, "   측정 중…"));
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
        let zoo_item = item(mtm, &format!("동물 — {}", ANIMALS[app.animal].label), None, self);
        zoo_item.setSubmenu(Some(&zoo));
        menu.addItem(&zoo_item);

        let alert = item(mtm, "과부하 알림", Some(sel!(toggleAlert:)), self);
        alert.setState(if app.alert_on {
            NSControlStateValueOn
        } else {
            NSControlStateValueOff
        });
        menu.addItem(&alert);

        menu.addItem(&NSMenuItem::separatorItem(mtm));
        menu.addItem(&item(mtm, "종료", Some(sel!(quit:)), self));
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

/// 누를 수 없는 설명 줄
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

fn probe() {
    let mut m = Metrics::new();
    println!("1초 간격으로 재서 찍습니다 (앞 두 번은 예열이라 처리량이 0)");
    let n: u32 = std::env::args().nth(2).and_then(|s| s.parse().ok()).unwrap_or(2);
    for round in 1..=n {
        std::thread::sleep(Duration::from_secs(1));
        m.refresh();
        println!("\n--- {round}회차 ---");
        for s in Source::ALL {
            let mark = if m.available[s.idx()] { " " } else { "x" };
            println!("{mark} {:<8} {:>6.1}%   {}", s.label(), m.latest(s), m.detail[s.idx()]);
        }
        println!("  상위 프로세스:");
        for p in m.top.iter().take(3) {
            println!("    {:<24} {:>5.1}%  {:>6.0} MB", p.name, p.cpu, p.mem as f64 / 1048576.0);
        }
        let tempo = ANIMALS[0].tempo;
        println!("  → CPU 기준 프레임 간격: {:.0}ms", interval_for(m.latest(Source::Cpu), tempo));
    }
}

/// 메뉴를 실제로 조립해서 그대로 찍는다. 클릭 없이 메뉴 구성을 검증하려고 둔다.
fn dump_menu(mtm: MainThreadMarker) {
    let bar = NSStatusBar::systemStatusBar();
    let item_ = bar.statusItemWithLength(NSVariableStatusItemLength);
    let mut metrics = Metrics::new();
    for _ in 0..3 {
        std::thread::sleep(Duration::from_millis(1100));
        metrics.refresh();
    }
    let ctrl = Controller::new(
        mtm,
        Ivars {
            item: item_.clone(),
            app: RefCell::new(App {
                metrics,
                source: Source::Cpu,
                animal: 0,
                frames: load_frames("cat"),
                frame: 0,
                interval: 500.0,
                alert_on: true,
                over_since: None,
                alerted: false,
            }),
            timer: RefCell::new(None),
        },
    );
    let menu = NSMenu::new(mtm);
    ctrl.build_menu(&menu);

    fn walk(menu: &NSMenu, indent: usize) {
        for i in 0..menu.numberOfItems() {
            let it = menu.itemAtIndex(i).unwrap();
            let mark = if it.state() == NSControlStateValueOn { "[v]" }
                       else if !it.isEnabled() { "   " } else { "[ ]" };
            let img = if it.image().is_some() { " 〔그래프〕" } else { "" };
            let title = it.title().to_string();
            let title = if title.is_empty() { "────────".into() } else { title };
            println!("{}{mark} {title}{img}", "  ".repeat(indent + 1));
            if let Some(sub) = it.submenu() {
                walk(&sub, indent + 1);
            }
        }
    }
    println!("메뉴 구성:");
    walk(&menu, 0);

    // 스파크라인을 눈으로 확인하려고 원시 픽셀을 떨군다
    let app = ctrl.ivars().app.borrow();
    let mut raw = Vec::new();
    for s in Source::ALL {
        raw.extend_from_slice(&render::sparkline_buffer(&app.metrics.hist[s.idx()]));
    }
    std::fs::write("/tmp/runzoo_spark.raw", &raw).unwrap();
    println!("\n스파크라인 원시 픽셀 → /tmp/runzoo_spark.raw (120x28 RGBA {}장)", Source::ALL.len());
}

/// 60초치가 다 찬 스파크라인이 어떻게 보이는지 합성 데이터로 확인한다.
fn dump_spark_demo() {
    use std::collections::VecDeque;
    let shapes: [(&str, fn(usize) -> f32); 4] = [
        ("톱니 (부하가 오르내림)", |i| ((i as f32 * 0.35).sin() * 0.5 + 0.5) * 90.0 + 5.0),
        ("계단 (한 번 뛰고 유지)", |i| if i < 30 { 15.0 } else { 78.0 }),
        ("뾰족 (짧은 폭주)", |i| if i % 17 == 0 { 95.0 } else { 8.0 }),
        ("포화 (계속 100%)", |_| 100.0),
    ];
    let mut raw = Vec::new();
    for (name, f) in shapes {
        println!("  {name}");
        let h: VecDeque<f32> = (0..metrics::HISTORY).map(f).collect();
        raw.extend_from_slice(&render::sparkline_buffer(&h));
    }
    std::fs::write("/tmp/runzoo_spark.raw", &raw).unwrap();
}

fn main() {
    if std::env::args().any(|a| a == "--dump-spark-demo") {
        dump_spark_demo();
        return;
    }
    if std::env::args().any(|a| a == "--dump-menu") {
        let mtm = MainThreadMarker::new().expect("메인 스레드에서 시작해야 한다");
        let app_ns = NSApplication::sharedApplication(mtm);
        app_ns.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
        dump_menu(mtm);
        return;
    }
    if std::env::args().any(|a| a == "--probe") {
        probe();
        return;
    }
    let mtm = MainThreadMarker::new().expect("메인 스레드에서 시작해야 한다");
    let app_ns = NSApplication::sharedApplication(mtm);
    // Accessory = Dock 에 뜨지 않고 메뉴바에만 산다
    app_ns.setActivationPolicy(NSApplicationActivationPolicy::Accessory);

    let animal_idx = animal::index_of(&load_str(K_ANIMAL).unwrap_or_else(|| "cat".into()));
    let source = load_str(K_SOURCE)
        .and_then(|k| Source::from_key(&k))
        .unwrap_or(Source::Cpu);

    let bar = NSStatusBar::systemStatusBar();
    // 가변 길이여야 한다. 고정 길이로 두면 매 프레임 이미지를 틀에 맞추느라
    // 40fps 기준 CPU 가 8% -> 26% 로 뛴다 (번갈아 3회씩 실측).
    let item_ = bar.statusItemWithLength(NSVariableStatusItemLength);
    let frames = load_frames(ANIMALS[animal_idx].key);
    if let Some(btn) = item_.button(mtm) {
        btn.setImage(Some(&frames[0]));
    }

    let state = App {
        metrics: Metrics::new(),
        source,
        animal: animal_idx,
        frames,
        frame: 0,
        interval: 500.0,
        alert_on: load_bool(K_ALERT, true),
        over_since: None,
        alerted: false,
    };
    let ctrl = Controller::new(
        mtm,
        Ivars {
            item: item_.clone(),
            app: RefCell::new(state),
            timer: RefCell::new(None),
        },
    );

    let menu = NSMenu::new(mtm);
    menu.setDelegate(Some(ProtocolObject::from_ref(&*ctrl)));
    ctrl.build_menu(&menu);
    item_.setMenu(Some(&menu));

    ctrl.restart_animation();
    let fetch = unsafe {
        NSTimer::timerWithTimeInterval_target_selector_userInfo_repeats(
            1.0,
            &*ctrl,
            sel!(fetch:),
            None,
            true,
        )
    };
    unsafe { NSRunLoop::currentRunLoop().addTimer_forMode(&fetch, NSRunLoopCommonModes) };

    app_ns.run();
}
