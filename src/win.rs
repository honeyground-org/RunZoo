//! The Windows front: a tray icon, a popup menu, and two timers.
//!
//! The same shape as `mac.rs` - pixel buffers become native images, menu nodes
//! become native menu items - against a very different API. Everything above
//! this file is shared, so the two fronts cannot disagree about what the app
//! measures, what it draws, or what the menu says.
//!
//! Two deliberate differences from macOS, both forced by the platform:
//!
//! A tray icon is square and small (`SM_CXSMICON`, usually 16 or 24 logical
//! pixels), where a menu bar lets a wide image sit in it. The animal is scaled
//! down to fit with a box filter and centred.
//!
//! Windows shows a checkmark or a bitmap in a menu row's gutter, not both. Rows
//! that carry a graph therefore go without the checkmark: the row driving the
//! animal is the one at the top of the list, and the submenu labels name the
//! current animal and colour anyway.
use std::cell::RefCell;
use std::ffi::c_void;

use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    CreateBitmap, CreateDIBSection, DeleteObject, GetDC, ReleaseDC, BITMAPINFO, BITMAPINFOHEADER,
    BI_RGB, DIB_RGB_COLORS, HBITMAP,
};
use windows_sys::Win32::System::Console::{AttachConsole, ATTACH_PARENT_PROCESS};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_TIP, NIIF_WARNING, NIM_ADD,
    NIM_DELETE, NIM_MODIFY, NOTIFYICONDATAW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreateIconIndirect, CreatePopupMenu, CreateWindowExW, DefWindowProcW,
    DestroyIcon, DestroyMenu, DispatchMessageW, GetCursorPos, GetMessageW, GetSystemMetrics,
    KillTimer, PostMessageW, PostQuitMessage, RegisterClassW, SetForegroundWindow,
    SetMenuItemInfoW, SetTimer, TrackPopupMenu, TranslateMessage, HICON, HMENU, ICONINFO,
    MENUITEMINFOW, MFS_CHECKED, MFS_DISABLED, MF_POPUP, MF_SEPARATOR, MF_STRING, MIIM_BITMAP,
    MSG, SM_CXSMICON, TPM_BOTTOMALIGN, TPM_RIGHTBUTTON, WM_APP, WM_COMMAND, WM_DESTROY,
    WM_RBUTTONUP, WM_TIMER, WNDCLASSW, WS_OVERLAPPED,
};

use crate::draw::{self, Buf};
use crate::menu::{self, Art, Cmd, Node};
use crate::state::App;
use crate::sys;
use crate::tint::{self, PALETTE};

/// The tray tells us about clicks through a message of our own choosing.
const WM_TRAY: u32 = WM_APP + 1;
const TIMER_FETCH: usize = 1;
const TIMER_ANIM: usize = 2;
const TRAY_ID: u32 = 1;

/// The binary is built as a GUI app so that double-clicking it does not open a
/// console. That also means the dump flags have nowhere to print - unless we
/// borrow the console of whatever launched us, which is what this does.
pub fn attach_console() {
    unsafe { AttachConsole(ATTACH_PARENT_PROCESS) };
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Copy a fixed-size UTF-16 field, truncating rather than overflowing.
fn fill(dst: &mut [u16], s: &str) {
    let src: Vec<u16> = s.encode_utf16().take(dst.len() - 1).collect();
    dst[..src.len()].copy_from_slice(&src);
    dst[src.len()] = 0;
}

// ---------------------------------------------------------------- bitmaps
/// A premultiplied RGBA buffer as a top-down 32bpp DIB.
///
/// Windows wants BGRA where we keep RGBA, and wants the rows the other way up
/// unless the height is given as negative - so the height is given as negative.
unsafe fn dib(buf: &Buf) -> HBITMAP {
    let mut bi: BITMAPINFO = std::mem::zeroed();
    bi.bmiHeader.biSize = std::mem::size_of::<BITMAPINFOHEADER>() as u32;
    bi.bmiHeader.biWidth = buf.w as i32;
    bi.bmiHeader.biHeight = -(buf.h as i32);
    bi.bmiHeader.biPlanes = 1;
    bi.bmiHeader.biBitCount = 32;
    bi.bmiHeader.biCompression = BI_RGB;

    let mut bits: *mut c_void = std::ptr::null_mut();
    let dc = GetDC(std::ptr::null_mut());
    let bmp = CreateDIBSection(dc, &bi, DIB_RGB_COLORS, &mut bits, std::ptr::null_mut(), 0);
    ReleaseDC(std::ptr::null_mut(), dc);
    if bmp.is_null() || bits.is_null() {
        return std::ptr::null_mut();
    }
    let out = std::slice::from_raw_parts_mut(bits as *mut u8, buf.w * buf.h * 4);
    for i in (0..out.len()).step_by(4) {
        out[i] = buf.px[i + 2]; // B
        out[i + 1] = buf.px[i + 1]; // G
        out[i + 2] = buf.px[i]; // R
        out[i + 3] = buf.px[i + 3]; // A
    }
    bmp
}

/// The animal, fitted into the square the tray asks for.
fn tray_buf(frame: &Buf, px: usize) -> Buf {
    let h = (px * frame.h / frame.w).max(1).min(px);
    let small = frame.scaled(px, h);
    let mut out = Buf::new(px, px);
    let top = (px - h) / 2;
    for y in 0..h {
        let src = y * px * 4;
        let dst = (top + y) * px * 4;
        out.px[dst..dst + px * 4].copy_from_slice(&small.px[src..src + px * 4]);
    }
    out
}

unsafe fn icon_from(buf: &Buf) -> HICON {
    let colour = dib(buf);
    if colour.is_null() {
        return std::ptr::null_mut();
    }
    // The colour bitmap carries the alpha, so the mask is only there because
    // ICONINFO insists on one.
    let mask = CreateBitmap(buf.w as i32, buf.h as i32, 1, 1, std::ptr::null());
    let ii = ICONINFO {
        fIcon: 1,
        xHotspot: 0,
        yHotspot: 0,
        hbmMask: mask,
        hbmColor: colour,
    };
    let icon = CreateIconIndirect(&ii);
    DeleteObject(mask as _);
    DeleteObject(colour as _);
    icon
}

// ---------------------------------------------------------------- state
struct Win {
    hwnd: HWND,
    app: App,
    /// One tray icon per animation frame, repainted when severity moves
    icons: Vec<HICON>,
    /// Bitmaps handed to the menu; freed once the menu closes
    bitmaps: Vec<HBITMAP>,
    icon_px: usize,
}

thread_local! {
    static WIN: RefCell<Option<Win>> = const { RefCell::new(None) };
}

impl Win {
    fn tray(&self, flags: u32) -> NOTIFYICONDATAW {
        let mut nid: NOTIFYICONDATAW = unsafe { std::mem::zeroed() };
        nid.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
        nid.hWnd = self.hwnd;
        nid.uID = TRAY_ID;
        nid.uFlags = flags;
        nid.uCallbackMessage = WM_TRAY;
        nid
    }

    fn current_icon(&self) -> HICON {
        self.icons
            .get(self.app.frame)
            .copied()
            .unwrap_or(std::ptr::null_mut())
    }

    fn show_icon(&self) {
        let mut nid = self.tray(NIF_ICON);
        nid.hIcon = self.current_icon();
        unsafe { Shell_NotifyIconW(NIM_MODIFY, &nid) };
    }

    /// Repaint every frame if the severity step moved.
    fn repaint(&mut self, force: bool) {
        if !self.app.needs_repaint(force) {
            return;
        }
        for old in self.icons.drain(..) {
            unsafe { DestroyIcon(old) };
        }
        let px = self.icon_px;
        self.icons = self
            .app
            .frames()
            .iter()
            .map(|b| unsafe { icon_from(&tray_buf(b, px)) })
            .collect();
        if self.app.frame >= self.icons.len() {
            self.app.frame = 0;
        }
        self.show_icon();
    }

    fn restart_animation(&self) {
        unsafe {
            KillTimer(self.hwnd, TIMER_ANIM);
            SetTimer(self.hwnd, TIMER_ANIM, self.app.interval as u32, None);
        }
    }

    fn free_bitmaps(&mut self) {
        for b in self.bitmaps.drain(..) {
            unsafe { DeleteObject(b as _) };
        }
    }
}

/// The overload alert, delivered through the tray icon itself.
pub fn balloon(title: &str, body: &str) {
    WIN.with(|w| {
        let Some(win) = w.borrow().as_ref().map(|w| w.tray(NIF_INFO)) else {
            return;
        };
        let mut nid = win;
        fill(&mut nid.szInfoTitle, title);
        fill(&mut nid.szInfo, body);
        nid.Anonymous.uTimeout = 10_000;
        nid.dwInfoFlags = NIIF_WARNING;
        unsafe { Shell_NotifyIconW(NIM_MODIFY, &nid) };
    });
}

// ---------------------------------------------------------------- menu
/// Windows right-aligns whatever follows a tab, in the accelerator column.
/// Several columns are joined into that one, which still lines their right
/// edge up - the point of the exercise.
fn row_text(cells: &menu::Cells) -> String {
    if cells.cols.is_empty() {
        cells.label.clone()
    } else {
        format!("{}\t{}", cells.label, cells.cols.join("  "))
    }
}

unsafe fn build(win: &mut Win, nodes: &[Node]) -> HMENU {
    let hmenu = CreatePopupMenu();
    for n in nodes {
        match n {
            Node::Separator => {
                AppendMenuW(hmenu, MF_SEPARATOR, 0, std::ptr::null());
            }
            Node::Caption(t) => {
                let w = wide(t);
                AppendMenuW(hmenu, MF_STRING | MFS_DISABLED, 0, w.as_ptr());
            }
            Node::Info(cells) => {
                let w = wide(&row_text(cells));
                AppendMenuW(hmenu, MF_STRING | MFS_DISABLED, 0, w.as_ptr());
            }
            Node::Row { cells, cmd, checked, art, .. } => {
                let text = wide(&row_text(cells));
                let mut flags = MF_STRING;
                // A gutter holds a checkmark or a bitmap, never both.
                if *checked && art.is_none() {
                    flags |= MFS_CHECKED;
                }
                AppendMenuW(hmenu, flags, cmd.tag() as usize, text.as_ptr());
                if let Some(a) = art {
                    if let Some(bmp) = art_bitmap(win, a) {
                        let mut mii: MENUITEMINFOW = std::mem::zeroed();
                        mii.cbSize = std::mem::size_of::<MENUITEMINFOW>() as u32;
                        mii.fMask = MIIM_BITMAP;
                        mii.hbmpItem = bmp;
                        SetMenuItemInfoW(hmenu, cmd.tag(), 0, &mii);
                        win.bitmaps.push(bmp);
                    }
                }
            }
            Node::Sub { label, items } => {
                let sub = build(win, items);
                let w = wide(label);
                AppendMenuW(hmenu, MF_POPUP, sub as usize, w.as_ptr());
            }
        }
    }
    hmenu
}

/// Menu gutters are small, so the graph is scaled down to something that fits
/// beside a line of text rather than dwarfing it.
fn art_bitmap(win: &Win, art: &Art) -> Option<HBITMAP> {
    let accent = win.app.accent_rgb();
    let buf = match art {
        Art::Spark(i, g) => {
            draw::sparkline(&win.app.metrics.hist[*i], tint::CALM, accent, *g).scaled(60, 14)
        }
        Art::Swatch(i) => draw::swatch(tint::CALM, PALETTE[*i].rgb).scaled(30, 11),
    };
    let bmp = unsafe { dib(&buf) };
    if bmp.is_null() {
        None
    } else {
        Some(bmp)
    }
}

unsafe fn popup() {
    let mut pt = POINT { x: 0, y: 0 };
    GetCursorPos(&mut pt);
    let (hwnd, hmenu) = WIN.with(|w| {
        let mut b = w.borrow_mut();
        let Some(win) = b.as_mut() else {
            return (std::ptr::null_mut(), std::ptr::null_mut());
        };
        win.free_bitmaps();
        let nodes = menu::build(&win.app);
        let hmenu = build(win, &nodes);
        (win.hwnd, hmenu)
    });
    if hmenu.is_null() {
        return;
    }
    // Without this the menu refuses to go away when you click elsewhere.
    SetForegroundWindow(hwnd);
    TrackPopupMenu(hmenu, TPM_RIGHTBUTTON | TPM_BOTTOMALIGN, pt.x, pt.y, 0, hwnd, std::ptr::null());
    PostMessageW(hwnd, 0, 0, 0);
    DestroyMenu(hmenu);
}

// ---------------------------------------------------------------- window
unsafe extern "system" fn wnd_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_TIMER => {
            match wp {
                TIMER_FETCH => {
                    let tick = WIN.with(|w| {
                        let mut b = w.borrow_mut();
                        let win = b.as_mut()?;
                        let tick = win.app.tick();
                        win.repaint(false);
                        Some(tick)
                    });
                    if let Some(tick) = tick {
                        if let Some((t, b)) = tick.alarm {
                            sys::notify(&t, &b);
                        }
                        if tick.interval_changed {
                            WIN.with(|w| {
                                if let Some(win) = w.borrow().as_ref() {
                                    win.restart_animation();
                                }
                            });
                        }
                    }
                }
                TIMER_ANIM => WIN.with(|w| {
                    if let Some(win) = w.borrow_mut().as_mut() {
                        win.app.advance_frame();
                        win.show_icon();
                    }
                }),
                _ => {}
            }
            0
        }
        WM_TRAY => {
            // With the classic tray version the click arrives in the low word.
            if (lp as u32) == WM_RBUTTONUP || (lp as u32) == WM_APP {
                popup();
            }
            0
        }
        WM_COMMAND => {
            if let Some(cmd) = Cmd::from_tag((wp & 0xFFFF) as u32) {
                run_command(cmd);
            }
            0
        }
        WM_DESTROY => {
            WIN.with(|w| {
                if let Some(win) = w.borrow().as_ref() {
                    let nid = win.tray(0);
                    Shell_NotifyIconW(NIM_DELETE, &nid);
                }
            });
            PostQuitMessage(0);
            0
        }
        _ => DefWindowProcW(hwnd, msg, wp, lp),
    }
}

fn run_command(cmd: Cmd) {
    match cmd {
        Cmd::OpenProcesses => {
            sys::open_task_manager();
            return;
        }
        Cmd::Quit => {
            WIN.with(|w| {
                if let Some(win) = w.borrow().as_ref() {
                    unsafe { PostMessageW(win.hwnd, WM_DESTROY, 0, 0) };
                }
            });
            return;
        }
        _ => {}
    }
    WIN.with(|w| {
        let mut b = w.borrow_mut();
        let Some(win) = b.as_mut() else { return };
        match cmd {
            Cmd::PickSource(i) => {
                win.app.set_source(i);
                win.repaint(false);
            }
            Cmd::PickAnimal(i) => {
                win.app.set_animal(i);
                win.repaint(true);
                win.restart_animation();
            }
            Cmd::PickAccent(i) => {
                win.app.set_accent(i);
                win.repaint(true);
            }
            Cmd::ToggleAlert => win.app.toggle_alert(),
            Cmd::OpenProcesses | Cmd::Quit => {}
        }
    });
}

pub fn run() {
    unsafe {
        let instance = GetModuleHandleW(std::ptr::null());
        let class = wide("RunZooTray");
        let mut wc: WNDCLASSW = std::mem::zeroed();
        wc.lpfnWndProc = Some(wnd_proc);
        wc.hInstance = instance;
        wc.lpszClassName = class.as_ptr();
        RegisterClassW(&wc);

        // Never shown. It exists to own the tray icon, the timers and the menu.
        let title = wide("RunZoo");
        let hwnd = CreateWindowExW(
            0,
            class.as_ptr(),
            title.as_ptr(),
            WS_OVERLAPPED,
            0,
            0,
            0,
            0,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
            instance,
            std::ptr::null(),
        );
        if hwnd.is_null() {
            eprintln!("RunZoo could not create its window");
            return;
        }

        let icon_px = GetSystemMetrics(SM_CXSMICON).max(16) as usize;
        WIN.with(|w| {
            *w.borrow_mut() = Some(Win {
                hwnd,
                app: App::new(),
                icons: Vec::new(),
                bitmaps: Vec::new(),
                icon_px,
            })
        });

        let (nid, interval) = WIN.with(|w| {
            let mut b = w.borrow_mut();
            let win = b.as_mut().unwrap();
            win.repaint(true);
            let mut nid = win.tray(NIF_ICON | NIF_MESSAGE | NIF_TIP);
            nid.hIcon = win.current_icon();
            fill(&mut nid.szTip, "RunZoo");
            (nid, win.app.interval)
        });
        Shell_NotifyIconW(NIM_ADD, &nid);

        SetTimer(hwnd, TIMER_FETCH, 1000, None);
        SetTimer(hwnd, TIMER_ANIM, interval as u32, None);

        let mut msg: MSG = std::mem::zeroed();
        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}
