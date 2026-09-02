//! The developer flags. Every one of them answers a question about what the
//! app would draw, without opening a window - which is what makes the colour
//! work reviewable at all, and what lets a Windows build be checked from a Mac.
use std::collections::VecDeque;
use std::time::Duration;

use crate::animal::ANIMALS;
use crate::draw::{self, Buf, SPARK_LARGE, SPARK_SMALL};
use crate::menu::{self, Node};
use crate::metrics::{self, Metrics, Source};
use crate::state::{interval_for, App};
use crate::tint::{self, PALETTE};

fn accent() -> Option<tint::Rgb> {
    PALETTE[App::new().accent].rgb
}

fn write(path: &str, bufs: &[Buf]) -> (usize, usize) {
    let mut raw = Vec::new();
    for b in bufs {
        raw.extend_from_slice(&b.px);
    }
    std::fs::write(path, &raw).unwrap();
    bufs.first().map(|b| (b.w, b.h)).unwrap_or((0, 0))
}

fn viewer(path: &str, w: usize, h: usize) {
    println!("view with: python3 tools/raw_to_png.py {path} {w} {h} --bg=light");
}

/// Print the whole colour ramp. Lets the gradient be checked without opening
/// the menu - or even having a menu bar.
pub fn tint_table() {
    let steps = [0.0f32, 10.0, 25.0, 40.0, 50.0, 60.0, 70.0, 85.0, 95.0, 100.0];
    println!("calm end = {} (the same in both appearances)", tint::CALM.hex());
    print!("{:<12}", "load %");
    for s in steps {
        print!("{s:>9.0}");
    }
    println!();
    for a in PALETTE {
        print!("{:<12}", a.label);
        for s in steps {
            print!("{:>9}", tint::colour_for(s, tint::CALM, a.rgb).hex());
        }
        println!();
    }
    println!(
        "\nseverity(load) = (load/100)^1.6, quantised to {} steps for redraw",
        tint::LEVELS
    );
}

/// Build the menu for real and print it. Verifies the structure, the ordering
/// and the alignment columns without a click, on any platform.
pub fn menu_tree() {
    let mut app = App::new();
    for _ in 0..3 {
        std::thread::sleep(Duration::from_millis(1100));
        app.metrics.refresh();
    }
    app.needs_repaint(true);

    fn walk(nodes: &[Node], indent: usize) {
        let pad = "  ".repeat(indent + 1);
        for n in nodes {
            match n {
                Node::Separator => println!("{pad}      ────────"),
                Node::Caption(t) => println!("{pad}      {t}"),
                Node::Info(c) => println!("{pad}      {}", c.tabbed()),
                Node::Row { cells, checked, lead, art, .. } => println!(
                    "{pad}{} {}{}{}",
                    if *checked { "[v]" } else { "[ ]" },
                    cells.tabbed(),
                    if *lead { "  [large]" } else { "" },
                    if art.is_some() { " [image]" } else { "" }
                ),
                Node::Sub { label, items } => {
                    println!("{pad}[ ] {label}");
                    walk(items, indent + 1);
                }
            }
        }
    }

    let nodes = menu::build(&app);
    println!("menu:");
    walk(&nodes, 0);

    println!(
        "\npainting: accent {} · severity step {}/{} · animal {}",
        PALETTE[app.accent].label,
        app.level,
        tint::LEVELS - 1,
        app.colour().hex()
    );

    // Every graph the menu would show, as pixels.
    let a = app.accent_rgb();
    let small: Vec<Buf> = Source::ALL
        .iter()
        .map(|s| draw::sparkline(&app.metrics.hist[s.idx()], tint::CALM, a, SPARK_SMALL))
        .collect();
    let (w, h) = write("/tmp/runzoo_spark.raw", &small);
    println!("\nsparkline pixels → /tmp/runzoo_spark.raw ({w}x{h} RGBA, {} of them)", small.len());
    viewer("/tmp/runzoo_spark.raw", w, h);

    let lead = vec![draw::sparkline(
        &app.metrics.hist[app.source.idx()],
        tint::CALM,
        a,
        SPARK_LARGE,
    )];
    let (w, h) = write("/tmp/runzoo_spark_lead.raw", &lead);
    println!("leading row → /tmp/runzoo_spark_lead.raw ({w}x{h} RGBA)");
    viewer("/tmp/runzoo_spark_lead.raw", w, h);
}

/// What a full 60 seconds looks like, drawn from synthetic data.
pub fn spark_demo() {
    /// A named synthetic load curve
    type Shape = (&'static str, fn(usize) -> f32);
    let shapes: [Shape; 4] = [
        ("sawtooth (load rising and falling)", |i| ((i as f32 * 0.35).sin() * 0.5 + 0.5) * 90.0 + 5.0),
        ("step (one jump, then held)", |i| if i < 30 { 15.0 } else { 78.0 }),
        ("spikes (short bursts)", |i| if i % 17 == 0 { 95.0 } else { 8.0 }),
        ("saturated (pinned at 100%)", |_| 100.0),
    ];
    let a = accent();
    let bufs: Vec<Buf> = shapes
        .iter()
        .map(|(name, f)| {
            println!("  {name}");
            let h: VecDeque<f32> = (0..metrics::HISTORY).map(f).collect();
            draw::sparkline(&h, tint::CALM, a, SPARK_SMALL)
        })
        .collect();
    let (w, h) = write("/tmp/runzoo_spark.raw", &bufs);
    println!("→ /tmp/runzoo_spark.raw ({w}x{h} RGBA x{})", bufs.len());
    viewer("/tmp/runzoo_spark.raw", w, h);
}

/// Every animal, every frame, painted across the severity ramp. This is how
/// the colour work gets checked: one picture instead of staring at a menu bar.
pub fn sprites() {
    let a = accent();
    let loads = [0.0f32, 25.0, 50.0, 70.0, 85.0, 100.0];
    let mut rows: Vec<Buf> = Vec::new();
    for an in ANIMALS {
        let mask = crate::animal::masks(an.key)[0];
        // One row per animal: the same frame at rising severity, side by side.
        let cells: Vec<Buf> = loads
            .iter()
            .map(|l| draw::sprite(mask, tint::colour_for(*l, tint::CALM, a)))
            .collect();
        let (cw, ch) = (cells[0].w, cells[0].h);
        let mut row = Buf::new(cw * cells.len(), ch);
        for (i, c) in cells.iter().enumerate() {
            for y in 0..ch {
                let src = y * cw * 4;
                let dst = (y * row.w + i * cw) * 4;
                row.px[dst..dst + cw * 4].copy_from_slice(&c.px[src..src + cw * 4]);
            }
        }
        println!(
            "  {}: {}",
            an.key,
            loads.map(|l| tint::colour_for(l, tint::CALM, a).hex()).join(" ")
        );
        rows.push(row);
    }
    let (w, h) = write("/tmp/runzoo_sprites.raw", &rows);
    println!("\nloads across each row: {loads:?}");
    println!("→ /tmp/runzoo_sprites.raw ({w}x{} RGBA)", h * rows.len());
    viewer("/tmp/runzoo_sprites.raw", w, h);
}

/// Print the measurements, with the severity and colour each one would produce.
pub fn probe(rounds: u32) {
    let mut m = Metrics::new();
    println!("sampling once a second (the first two rounds are warm-up, so throughput reads 0)");
    let a = accent();
    for round in 1..=rounds {
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
                tint::colour_for(v, tint::CALM, a).hex(),
                m.detail[s.idx()]
            );
        }
        println!("  top processes:");
        for p in m.top.iter().take(3) {
            println!("    {:<24} {:>5.1}%  {:>6.0} MB", p.name, p.cpu, p.mem as f64 / 1048576.0);
        }
        println!(
            "  → frame interval on CPU: {:.0}ms",
            interval_for(m.latest(Source::Cpu), ANIMALS[0].tempo)
        );
    }
}
