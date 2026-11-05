//! The settings the user changes with commands, such as the autosave interval, kept in a small file in the user's
//! settings folder.

use std::cell::RefCell;
use std::fs;
use std::path::PathBuf;
use std::rc::Rc;

use crate::commands::defaults::Defaults;
use crate::commands::prompts::{Answer, Io, Prompt, PromptKind, Step};

/// How many minutes pass between autosaves when nothing has been set.
pub const DEFAULT_SAVETIME: u32 = 10;
/// The longest autosave interval SAVETIME takes, in minutes. Zero turns autosave off.
pub const MAX_SAVETIME: u32 = 600;
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

/// The autosave interval in minutes that was last set, or the default.
pub fn savetime() -> u32 {
    folder().and_then(|d| fs::read_to_string(d.join(SETTINGS_FILE)).ok()).map_or(DEFAULT_SAVETIME, |text| parse_savetime(&text))
}

/// Keep the autosave interval for later sessions. A file that cannot be written is simply not kept.
pub fn set_savetime(minutes: u32) {
    if let Some(folder) = folder() {
        let _ = fs::create_dir_all(&folder);
        let _ = fs::write(folder.join(SETTINGS_FILE), format!("savetime={minutes}\n"));
    }
}

/// SAVETIME: asks for the autosave interval in minutes, as AutoCAD does, until a whole number from 0 to 600 is typed.
/// The command leaves the value in `defaults` for the window to set and keep.
pub async fn savetime_command(io: Io, defaults: Rc<RefCell<Defaults>>) {
    let current = defaults.borrow().autosave_minutes;
    loop {
        let prompt = Prompt::new(PromptKind::Number, format!("Enter new value for SAVETIME <{current}>:"))
            .default(Answer::Number(f64::from(current)));
        let Some(Answer::Number(value)) = io.ask(prompt).await else { return };
        if value.fract() == 0.0 && (0.0..=f64::from(MAX_SAVETIME)).contains(&value) {
            let minutes = value as u32;
            defaults.borrow_mut().autosave_request = Some(minutes);
            let message = if minutes == 0 { "Autosave is off.".to_string() } else { format!("Autosave every {minutes} minutes.") };
            io.step(Step::Report(message));
            return;
        }
        io.step(Step::Report(format!("Enter a whole number of minutes from 0 to {MAX_SAVETIME}.")));
    }
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
}
