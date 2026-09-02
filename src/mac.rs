//! The macOS front: a status item, a menu, and two timers.
//!
//! Everything platform-independent lives elsewhere - this file turns pixel
//! buffers into NSImages and menu nodes into NSMenuItems, and nothing more.
use std::cell::RefCell;

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObjectProtocol, ProtocolObject, Sel};
use objc2::{
    define_class, msg_send, sel, AllocAnyThread, DefinedClass, MainThreadMarker, MainThreadOnly,
};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSBitmapImageRep, NSColor,
    NSControlStateValueOff, NSControlStateValueOn, NSDeviceRGBColorSpace, NSFont,
    NSFontAttributeName, NSForegroundColorAttributeName, NSImage, NSMenu, NSMenuDelegate,
    NSMenuItem, NSMutableParagraphStyle, NSParagraphStyleAttributeName, NSStatusBar, NSStatusItem,
    NSTextAlignment, NSTextTab, NSVariableStatusItemLength,
};
use objc2_foundation::{
    NSArray, NSAttributedString, NSDictionary, NSObject, NSRunLoop, NSRunLoopCommonModes, NSSize,
    NSString, NSTimer,
};

use crate::draw::{self, Buf};
use crate::menu::{self, Art, Cells, Cmd, Node};
use crate::state::App;
use crate::sys;
use crate::tint::{self, PALETTE};

/// Where the numbers line up, in points from the left of the row. Wide enough
/// that the longest reading ("86% · 20.6 GB / 24.0 GB") never reaches back into
/// the label beside it.
const COL_VALUE: f64 = 250.0;
const COL_PROC_CPU: f64 = 200.0;
const COL_PROC_MEM: f64 = 270.0;
/// How much bigger the row driving the animal is drawn.
const LEAD_BUMP: f64 = 2.0;

fn stops(cols: usize) -> &'static [f64] {
    match cols {
        1 => &[COL_VALUE],
        2 => &[COL_PROC_CPU, COL_PROC_MEM],
        _ => &[],
    }
}

// ---------------------------------------------------------------- images
/// Wrap a premultiplied buffer in an NSImage, at half its pixel size in points
/// so that one pixel lands on one pixel of a 2x display.
///
/// `template` marks it as a macOS template image: macOS then throws the colour
/// away and tints the alpha itself for light and dark. So it must be false
/// whenever we mean the colour.
fn image(buf: &Buf, template: bool) -> Retained<NSImage> {
    unsafe {
        let rep = NSBitmapImageRep::initWithBitmapDataPlanes_pixelsWide_pixelsHigh_bitsPerSample_samplesPerPixel_hasAlpha_isPlanar_colorSpaceName_bytesPerRow_bitsPerPixel(
            NSBitmapImageRep::alloc(),
            std::ptr::null_mut(), // pass NULL and the rep owns its buffer (no lifetime worries)
            buf.w as isize,
            buf.h as isize,
            8,
            4,
            true,
            false,
            NSDeviceRGBColorSpace,
            (buf.w * 4) as isize,
            32,
        )
        .expect("could not create bitmap rep");
        std::ptr::copy_nonoverlapping(buf.px.as_ptr(), rep.bitmapData(), buf.px.len());

        let img = NSImage::initWithSize(
            NSImage::alloc(),
            NSSize::new(buf.w as f64 / 2.0, buf.h as f64 / 2.0),
        );
        img.addRepresentation(&rep);
        img.setTemplate(template);
        img
    }
}

fn art_image(app: &App, art: &Art) -> Retained<NSImage> {
    match art {
        Art::Spark(i, g) => {
            let accent = app.accent_rgb();
            let b = draw::sparkline(&app.metrics.hist[*i], tint::CALM, accent, *g);
            image(&b, accent.is_none())
        }
        Art::Swatch(i) => {
            let rgb = PALETTE[*i].rgb;
            image(&draw::swatch(tint::CALM, rgb), rgb.is_none())
        }
    }
}

// ---------------------------------------------------------------- titles
/// A menu row with its numbers right-aligned, and optionally a size up.
///
/// NSMenuItem has no font or alignment of its own, so a row that wants either
/// has to be handed an attributed string: a tab between each column, and a
/// right tab stop to hang that column from.
///
/// `dim` is false for rows you can click. Naming a colour would pin it through
/// the highlight as well, and macOS wants to invert the text itself when a row
/// is under the pointer. Disabled rows never highlight, so those can be given
/// the secondary label colour safely.
fn aligned_title(text: &str, stops: &[f64], bump: f64, dim: bool) -> Retained<NSAttributedString> {
    let menu_font = NSFont::menuFontOfSize(0.0);
    let font = if bump > 0.0 {
        NSFont::menuFontOfSize(menu_font.pointSize() + bump)
    } else {
        menu_font
    };

    let para = NSMutableParagraphStyle::new();
    let opts = NSDictionary::new();
    let tabs: Vec<Retained<NSTextTab>> = stops
        .iter()
        .map(|x| unsafe {
            NSTextTab::initWithTextAlignment_location_options(
                NSTextTab::alloc(),
                NSTextAlignment::Right,
                *x,
                &opts,
            )
        })
        .collect();
    para.setTabStops(Some(&NSArray::from_retained_slice(&tabs)));

    let mut keys: Vec<&NSString> = vec![
        unsafe { NSFontAttributeName },
        unsafe { NSParagraphStyleAttributeName },
    ];
    let mut vals: Vec<&AnyObject> = vec![&font, &para];
    let grey = NSColor::secondaryLabelColor();
    if dim {
        keys.push(unsafe { NSForegroundColorAttributeName });
        vals.push(&grey);
    }
    let attrs = NSDictionary::from_slices(&keys, &vals);

    unsafe {
        NSAttributedString::initWithString_attributes(
            NSAttributedString::alloc(),
            &NSString::from_str(text),
            Some(&attrs),
        )
    }
}

fn new_item(
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
fn caption(mtm: MainThreadMarker, title: &str) -> Retained<NSMenuItem> {
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

fn plain(cells: &Cells) -> String {
    let mut s = cells.label.clone();
    for c in &cells.cols {
        s.push_str("   ");
        s.push_str(c);
    }
    s
}

// ---------------------------------------------------------------- controller
struct Ivars {
    item: Retained<NSStatusItem>,
    app: RefCell<App>,
    frames: RefCell<Vec<Retained<NSImage>>>,
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
            iv.app.borrow_mut().advance_frame();
            let i = iv.app.borrow().frame;
            let img = iv.frames.borrow().get(i).cloned();
            if let (Some(img), Some(btn)) = (img, iv.item.button(MainThreadMarker::from(self))) {
                btn.setImage(Some(&img));
            }
        }

        #[unsafe(method(fetch:))]
        fn fetch(&self, _t: *mut NSObject) {
            let tick = self.ivars().app.borrow_mut().tick();
            self.repaint(false);
            if let Some((t, b)) = tick.alarm {
                sys::notify(&t, &b);
            }
            if tick.interval_changed {
                self.restart_animation();
            }
        }

        /// Every clickable row goes through here; the tag says which one.
        #[unsafe(method(command:))]
        fn command(&self, sender: &NSMenuItem) {
            let Some(cmd) = Cmd::from_tag(sender.tag() as u32) else {
                return;
            };
            match cmd {
                Cmd::PickSource(i) => {
                    self.ivars().app.borrow_mut().set_source(i);
                    // A different source usually means a different severity now.
                    self.repaint(false);
                }
                Cmd::PickAnimal(i) => {
                    self.ivars().app.borrow_mut().set_animal(i);
                    self.repaint(true);
                    self.restart_animation();
                }
                Cmd::PickAccent(i) => {
                    self.ivars().app.borrow_mut().set_accent(i);
                    self.repaint(true);
                }
                Cmd::ToggleAlert => self.ivars().app.borrow_mut().toggle_alert(),
                Cmd::OpenProcesses => sys::open_task_manager(),
                Cmd::Quit => {
                    NSApplication::sharedApplication(MainThreadMarker::from(self)).terminate(None)
                }
            }
        }
    }
);

impl Controller {
    fn new(mtm: MainThreadMarker, ivars: Ivars) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(ivars);
        unsafe { msg_send![super(this), init] }
    }

    /// Repaint the animal if the severity step moved. Called once a second, so
    /// it must do nothing on the ticks where nothing changed.
    fn repaint(&self, force: bool) {
        let iv = self.ivars();
        let mut app = iv.app.borrow_mut();
        if !app.needs_repaint(force) {
            return;
        }
        let template = app.accent_rgb().is_none();
        let frames: Vec<Retained<NSImage>> =
            app.frames().iter().map(|b| image(b, template)).collect();
        if app.frame >= frames.len() {
            app.frame = 0;
        }
        let img = frames.get(app.frame).cloned();
        drop(app);
        *iv.frames.borrow_mut() = frames;
        if let (Some(img), Some(btn)) = (img, iv.item.button(MainThreadMarker::from(self))) {
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
        let app = self.ivars().app.borrow();
        let nodes = menu::build(&app);
        menu.removeAllItems();
        self.fill(menu, &nodes, &app);
    }

    fn fill(&self, menu: &NSMenu, nodes: &[Node], app: &App) {
        let mtm = MainThreadMarker::from(self);
        for n in nodes {
            match n {
                Node::Separator => menu.addItem(&NSMenuItem::separatorItem(mtm)),
                Node::Caption(t) => menu.addItem(&caption(mtm, t)),
                Node::Info(cells) => {
                    let it = caption(mtm, &plain(cells));
                    it.setAttributedTitle(Some(&aligned_title(
                        &cells.tabbed(),
                        stops(cells.cols.len()),
                        0.0,
                        true,
                    )));
                    menu.addItem(&it);
                }
                Node::Row { cells, cmd, checked, lead, art } => {
                    // The plain title is what a screen reader and --dump-menu
                    // read; the attributed one is what gets drawn.
                    let it = new_item(mtm, &plain(cells), Some(sel!(command:)), self);
                    it.setTag(cmd.tag() as isize);
                    if !cells.cols.is_empty() || *lead {
                        it.setAttributedTitle(Some(&aligned_title(
                            &cells.tabbed(),
                            stops(cells.cols.len()),
                            if *lead { LEAD_BUMP } else { 0.0 },
                            false,
                        )));
                    }
                    if let Some(a) = art {
                        it.setImage(Some(&art_image(app, a)));
                    }
                    it.setState(if *checked {
                        NSControlStateValueOn
                    } else {
                        NSControlStateValueOff
                    });
                    menu.addItem(&it);
                }
                Node::Sub { label, items } => {
                    let sub = NSMenu::new(mtm);
                    self.fill(&sub, items, app);
                    let it = new_item(mtm, label, None, self);
                    it.setSubmenu(Some(&sub));
                    menu.addItem(&it);
                }
            }
        }
    }
}

pub fn run() {
    let mtm = MainThreadMarker::new().expect("must be started on the main thread");
    let app_ns = NSApplication::sharedApplication(mtm);
    // Accessory = no Dock icon, lives in the menu bar only
    app_ns.setActivationPolicy(NSApplicationActivationPolicy::Accessory);

    let bar = NSStatusBar::systemStatusBar();
    // Must be variable length. Fixed length makes macOS fit every frame to the
    // slot, which took CPU from 8% to 26% at 40fps (measured, alternating x3).
    let item = bar.statusItemWithLength(NSVariableStatusItemLength);

    let ctrl = Controller::new(
        mtm,
        Ivars {
            item: item.clone(),
            app: RefCell::new(App::new()),
            frames: RefCell::new(Vec::new()),
            timer: RefCell::new(None),
        },
    );
    ctrl.repaint(true);

    let menu = NSMenu::new(mtm);
    menu.setDelegate(Some(ProtocolObject::from_ref(&*ctrl)));
    ctrl.build_menu(&menu);
    item.setMenu(Some(&menu));

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
