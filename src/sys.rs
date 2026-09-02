//! The two things RunZoo asks the operating system for that are not drawing:
//! say something out loud, and hand the user over to the task manager.

#[cfg(target_os = "macos")]
mod imp {
    use std::process::Command;

    pub const TASK_MANAGER: &str = "Open Activity Monitor";

    /// Addressed by bundle id rather than by name: the name is localised, and
    /// the path has moved between macOS releases, while the id has not.
    pub fn open_task_manager() {
        let _ = Command::new("open").arg("-b").arg("com.apple.ActivityMonitor").spawn();
    }

    pub fn open_url(url: &str) {
        let _ = Command::new("open").arg(url).spawn();
    }

    pub fn notify(title: &str, body: &str) {
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
}

#[cfg(target_os = "windows")]
mod imp {
    use std::process::Command;

    pub const TASK_MANAGER: &str = "Open Task Manager";

    pub fn open_task_manager() {
        // Task Manager asks for elevation on some machines; if the user says no
        // there is nothing for us to do about it, and nothing to report.
        let _ = Command::new("taskmgr.exe").spawn();
    }

    /// Hand the URL to whatever the user has set as their browser.
    ///
    /// `cmd /C start` would do it too, but it flashes a console window on its
    /// way past, which is exactly what building a GUI subsystem app was meant
    /// to avoid.
    pub fn open_url(url: &str) {
        use windows_sys::Win32::UI::Shell::ShellExecuteW;
        let verb: Vec<u16> = "open\0".encode_utf16().collect();
        let target: Vec<u16> = url.encode_utf16().chain(std::iter::once(0)).collect();
        unsafe {
            ShellExecuteW(
                std::ptr::null_mut(),
                verb.as_ptr(),
                target.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL,
            )
        };
    }

    /// Windows delivers this one through the tray icon itself, so the message
    /// is handed to the tray layer rather than shelled out. See win::notify.
    pub fn notify(title: &str, body: &str) {
        crate::win::balloon(title, body);
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
mod imp {
    pub const TASK_MANAGER: &str = "Open the task manager";

    pub fn open_task_manager() {}

    pub fn open_url(url: &str) {
        eprintln!("open {url}");
    }

    pub fn notify(title: &str, body: &str) {
        eprintln!("{title}: {body}");
    }
}

pub use imp::{notify, open_task_manager, open_url, TASK_MANAGER};
