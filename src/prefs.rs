//! The handful of settings worth remembering: animal, source, accent, alert.
//!
//! Each platform keeps them where that platform expects to find them, which is
//! why this is an interface rather than a file format.

#[cfg(target_os = "macos")]
mod imp {
    use objc2::rc::Retained;
    use objc2_foundation::{NSString, NSUserDefaults};

    fn defaults() -> Retained<NSUserDefaults> {
        NSUserDefaults::standardUserDefaults()
    }

    pub fn load_str(key: &str) -> Option<String> {
        defaults().stringForKey(&NSString::from_str(key)).map(|s| s.to_string())
    }

    pub fn save_str(key: &str, val: &str) {
        unsafe {
            defaults().setObject_forKey(Some(&*NSString::from_str(val)), &NSString::from_str(key))
        };
    }

    pub fn save_bool(key: &str, val: bool) {
        defaults().setBool_forKey(val, &NSString::from_str(key));
    }

    pub fn load_bool(key: &str, fallback: bool) -> bool {
        let d = defaults();
        let k = NSString::from_str(key);
        if d.objectForKey(&k).is_some() {
            d.boolForKey(&k)
        } else {
            fallback
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    //! One `key=value` line per setting, under the user's config directory.
    //! Four settings do not need a format with a parser behind it.
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    fn path() -> Option<PathBuf> {
        let base = std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))?;
        Some(base.join("RunZoo").join("settings.conf"))
    }

    fn read() -> BTreeMap<String, String> {
        let mut map = BTreeMap::new();
        let Some(p) = path() else { return map };
        let Ok(text) = std::fs::read_to_string(p) else { return map };
        for line in text.lines() {
            if let Some((k, v)) = line.split_once('=') {
                map.insert(k.trim().to_string(), v.trim().to_string());
            }
        }
        map
    }

    fn write(map: &BTreeMap<String, String>) {
        let Some(p) = path() else { return };
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let body: String = map.iter().map(|(k, v)| format!("{k}={v}\n")).collect();
        let _ = std::fs::write(p, body);
    }

    pub fn load_str(key: &str) -> Option<String> {
        read().get(key).cloned()
    }

    pub fn save_str(key: &str, val: &str) {
        let mut map = read();
        map.insert(key.to_string(), val.to_string());
        write(&map);
    }

    pub fn save_bool(key: &str, val: bool) {
        save_str(key, if val { "1" } else { "0" });
    }

    pub fn load_bool(key: &str, fallback: bool) -> bool {
        match load_str(key).as_deref() {
            Some("1") | Some("true") => true,
            Some("0") | Some("false") => false,
            _ => fallback,
        }
    }
}

pub use imp::{load_bool, load_str, save_bool, save_str};

pub const K_ANIMAL: &str = "animal";
pub const K_SOURCE: &str = "source";
pub const K_ALERT: &str = "alertEnabled";
pub const K_ACCENT: &str = "accentColour";
