// A tray app must not open a console window. The dump flags still print: when
// one is used, win::attach_console hooks the terminal that launched us.
#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

//! RunZoo — an animal runs across the menu bar as fast as the system is busy,
//! and wears the colour of how bad things are.
//!
//! Started from the idea in RunCat365 (Takuto Nakamura, Apache-2.0). All of the
//! code is new; the original is Windows-only, so there was nothing to reuse.
//!
//! The measuring, the drawing and the menu are platform-independent; `mac` and
//! `win` only turn pixel buffers into native images and menu nodes into native
//! menus. That is also why every `--dump-*` flag works everywhere: they answer
//! questions about pixels, and pixels are the part that does not vary.
mod animal;
mod draw;
mod dump;
mod menu;
mod metrics;
mod prefs;
mod sprites;
mod state;
mod sys;
mod tint;

#[cfg(target_os = "macos")]
mod mac;
#[cfg(target_os = "windows")]
mod win;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let has = |f: &str| args.iter().any(|a| a == f);

    #[cfg(target_os = "windows")]
    if args.iter().any(|a| a.starts_with("--dump") || a == "--probe") {
        win::attach_console();
    }

    if has("--dump-tint") {
        dump::tint_table();
        return;
    }
    if has("--dump-spark-demo") {
        dump::spark_demo();
        return;
    }
    if has("--dump-sprites") {
        dump::sprites();
        return;
    }
    if has("--dump-menu") {
        dump::menu_tree();
        return;
    }
    if has("--probe") {
        let n = args
            .iter()
            .position(|a| a == "--probe")
            .and_then(|i| args.get(i + 1))
            .and_then(|s| s.parse().ok())
            .unwrap_or(2);
        dump::probe(n);
        return;
    }

    #[cfg(target_os = "macos")]
    mac::run();
    #[cfg(target_os = "windows")]
    win::run();
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    eprintln!("RunZoo has no tray for this platform yet. The --dump flags still work.");
}
