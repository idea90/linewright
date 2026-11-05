//! The drawings opened or saved lately, newest first, kept in a small file in the user's settings folder.

use std::path::{Path, PathBuf};

const MAX_RECENT: usize = 8;

/// Where the list lives: %APPDATA%\Linewright on Windows, ~/.config/linewright elsewhere.
fn list_file() -> Option<PathBuf> {
    let folder = if cfg!(windows) {
        std::env::var_os("APPDATA").map(|d| PathBuf::from(d).join("Linewright"))
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
            .map(|d| d.join("linewright"))
    }?;
    Some(folder.join("recent.txt"))
}

/// The list with `path` put first, once, and the oldest beyond the limit dropped.
fn updated(list: &[PathBuf], path: &Path) -> Vec<PathBuf> {
    std::iter::once(path.to_path_buf()).chain(list.iter().filter(|p| p.as_path() != path).cloned()).take(MAX_RECENT).collect()
}

/// The recent drawings that are still there.
pub fn load() -> Vec<PathBuf> {
    let Some(file) = list_file() else { return vec![] };
    let text = std::fs::read_to_string(file).unwrap_or_default();
    text.lines().filter(|l| !l.trim().is_empty()).map(PathBuf::from).filter(|p| p.is_file()).take(MAX_RECENT).collect()
}

/// Put a drawing at the top of the list. A list that cannot be written is simply not kept.
pub fn remember(path: &Path) -> Vec<PathBuf> {
    let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let list = updated(&load(), &path);
    if let Some(file) = list_file() {
        let text: String = list.iter().map(|p| format!("{}\n", p.display())).collect();
        let _ = file.parent().map(std::fs::create_dir_all);
        let _ = std::fs::write(file, text);
    }
    list
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_latest_comes_first_once_and_the_list_stays_short() {
        let list: Vec<PathBuf> = (0..MAX_RECENT).map(|i| PathBuf::from(format!("{i}.dxf"))).collect();
        let again = updated(&list, Path::new("3.dxf"));
        assert_eq!(again[0], PathBuf::from("3.dxf"));
        assert_eq!(again.len(), MAX_RECENT);
        assert_eq!(again.iter().filter(|p| p.as_path() == Path::new("3.dxf")).count(), 1);
        let fresh = updated(&list, Path::new("new.dxf"));
        assert_eq!((fresh[0].as_path(), fresh.len()), (Path::new("new.dxf"), MAX_RECENT));
        assert!(!fresh.contains(&PathBuf::from(format!("{}.dxf", MAX_RECENT - 1))));
    }
}
