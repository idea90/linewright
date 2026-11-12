//! The settings the user changes with commands, such as the autosave interval, kept in a small file in the user's
//! settings folder.

use std::fs;
use std::path::PathBuf;

pub use crate::commands::defaults::{DEFAULT_SAVETIME, MAX_SAVETIME};

const SETTINGS_FILE: &str = "settings.txt";

/// Where the settings live: %APPDATA%\Linewright on Windows, ~/.config/linewright elsewhere.
fn folder() -> Option<PathBuf> {
    if cfg!(windows) {
        std::env::var_os("APPDATA").map(|d| PathBuf::from(d).join("Linewright"))
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
            .map(|d| d.join("linewright"))
    }
}

/// The folder autosave copies are written into, inside the settings folder.
pub fn autosave_folder() -> Option<PathBuf> {
    folder().map(|d| d.join("autosave"))
}

/// The SAVETIME value in the settings text, as `savetime=<minutes>`. A value that is missing or out of range gives the
/// default.
fn parse_savetime(text: &str) -> u32 {
    text.lines()
        .find_map(|line| line.trim().strip_prefix("savetime="))
        .and_then(|value| value.trim().parse::<u32>().ok())
        .filter(|minutes| *minutes <= MAX_SAVETIME)
        .unwrap_or(DEFAULT_SAVETIME)
}

/// The settings file's text, empty when there is none yet.
fn read() -> String {
    folder().and_then(|d| fs::read_to_string(d.join(SETTINGS_FILE)).ok()).unwrap_or_default()
}

/// One setting's value in the settings text, as `name=value`.
fn value_in<'a>(text: &'a str, name: &str) -> Option<&'a str> {
    text.lines().find_map(|line| line.trim().strip_prefix(name)?.strip_prefix('=')).map(str::trim)
}

/// The settings text with one setting set, the others kept as they are.
fn with_value(text: &str, name: &str, value: &str) -> String {
    let mut out: String = text
        .lines()
        .filter(|line| line.trim().strip_prefix(name).is_none_or(|rest| !rest.starts_with('=')))
        .map(|line| format!("{line}\n"))
        .collect();
    out.push_str(&format!("{name}={value}\n"));
    out
}

/// Keep one setting for later sessions. A file that cannot be written is simply not kept.
fn set(name: &str, value: &str) {
    if let Some(folder) = folder() {
        let _ = fs::create_dir_all(&folder);
        let _ = fs::write(folder.join(SETTINGS_FILE), with_value(&read(), name, value));
    }
}

/// The autosave interval in minutes that was last set, or the default.
pub fn savetime() -> u32 {
    parse_savetime(&read())
}

/// Keep the autosave interval for later sessions.
pub fn set_savetime(minutes: u32) {
    set("savetime", &minutes.to_string());
}

/// Whether Linewright looks for a newer release by itself, once a day. On unless turned off.
pub fn update_checks() -> bool {
    value_in(&read(), "update_checks") != Some("off")
}

pub fn set_update_checks(on: bool) {
    set("update_checks", if on { "on" } else { "off" });
}

/// When Linewright last looked for a newer release, in seconds since 1970.
pub fn last_update_check() -> u64 {
    value_in(&read(), "last_update_check").and_then(|v| v.parse().ok()).unwrap_or(0)
}

pub fn set_last_update_check(seconds: u64) {
    set("last_update_check", &seconds.to_string());
}

/// A version the user chose to skip: it is not offered again, though a later one is.
pub fn skipped_version() -> Option<String> {
    value_in(&read(), "skip_version").filter(|v| !v.is_empty()).map(str::to_string)
}

pub fn set_skipped_version(version: &str) {
    set("skip_version", version);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_saved_interval_is_read_back_or_falls_back_to_the_default() {
        assert_eq!(parse_savetime("savetime=25\n"), 25);
        assert_eq!(parse_savetime("  savetime= 0 \n"), 0, "zero turns autosave off");
        assert_eq!(parse_savetime(""), DEFAULT_SAVETIME);
        assert_eq!(parse_savetime("savetime=601\n"), DEFAULT_SAVETIME, "beyond the longest interval");
        assert_eq!(parse_savetime("savetime=soon\n"), DEFAULT_SAVETIME);
    }

    #[test]
    fn setting_one_value_keeps_the_others() {
        let text = with_value("savetime=25\nupdate_checks=on\n", "update_checks", "off");
        assert_eq!(value_in(&text, "savetime"), Some("25"));
        assert_eq!(value_in(&text, "update_checks"), Some("off"));
        assert_eq!(text.matches("update_checks").count(), 1);
        assert_eq!(parse_savetime(&with_value(&text, "last_update_check", "1")), 25);
        assert_eq!(value_in("skip_version=\n", "skip_version"), Some(""));
        assert_eq!(value_in("savetimes=4\n", "savetime"), None, "a longer name is another setting");
    }
}
