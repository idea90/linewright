//! AutoCAD's hatch pattern files (.pat): each pattern a heading "*NAME, description", then one line per family of
//! parallel lines: angle, x-origin, y-origin, offset along, offset across, then its dashes. The files are read from
//! where AutoCAD keeps them, while the program runs; a few common patterns work without them.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::OnceLock;

/// One family of lines as a pattern file gives it.
#[derive(Clone, Debug, PartialEq)]
pub struct PatFamily {
    pub angle: f64,
    pub x: f64,
    pub y: f64,
    pub along: f64,
    pub across: f64,
    pub dashes: Vec<f64>,
}

/// Every pattern in a file's text, by upper-case name. Comments, and lines that are not numbers, are left out.
pub fn parse(text: &str) -> BTreeMap<String, Vec<PatFamily>> {
    let mut patterns: BTreeMap<String, Vec<PatFamily>> = BTreeMap::new();
    let mut current: Option<String> = None;
    for line in text.lines() {
        let line = line.split(';').next().unwrap_or("").trim().trim_start_matches('\u{feff}');
        if line.is_empty() {
            continue;
        }
        if let Some(heading) = line.strip_prefix('*') {
            let name = heading.split(',').next().unwrap_or("").trim().to_uppercase();
            current = (!name.is_empty()).then_some(name);
            continue;
        }
        let Some(name) = &current else { continue };
        let Ok(numbers) = line.split(',').map(|n| n.trim().parse::<f64>()).collect::<Result<Vec<f64>, _>>() else { continue };
        if let [angle, x, y, along, across, dashes @ ..] = numbers.as_slice() {
            patterns.entry(name.clone()).or_default().push(PatFamily {
                angle: *angle,
                x: *x,
                y: *y,
                along: *along,
                across: *across,
                dashes: dashes.to_vec(),
            });
        }
    }
    patterns
}

/// Where AutoCAD keeps acadiso.pat: each product's Support folder under the user's settings, newest first; on WSL,
/// the same under /mnt/c. LINEWRIGHT_PAT_DIRS comes first.
fn files() -> Vec<PathBuf> {
    fn search(folder: &std::path::Path, depth: usize, found: &mut Vec<PathBuf>) {
        for entry in std::fs::read_dir(folder).into_iter().flatten().flatten() {
            let path = entry.path();
            if path.is_dir() {
                if depth > 0 {
                    search(&path, depth - 1, found);
                }
            } else if entry.file_name().to_string_lossy().eq_ignore_ascii_case("acadiso.pat") {
                found.push(path);
            }
        }
    }
    let mut folders: Vec<PathBuf> =
        std::env::var_os("LINEWRIGHT_PAT_DIRS").map(|v| std::env::split_paths(&v).collect()).unwrap_or_default();
    let mut roots: Vec<PathBuf> = std::env::var_os("APPDATA").map(|v| PathBuf::from(v).join("Autodesk")).into_iter().collect();
    for users in std::fs::read_dir("/mnt/c/Users").into_iter().flatten().flatten() {
        roots.push(users.path().join("AppData/Roaming/Autodesk"));
    }
    let mut found = vec![];
    for folder in folders.drain(..) {
        search(&folder, 0, &mut found);
    }
    let given = found.len();
    for root in roots {
        search(&root, 4, &mut found);
    }
    found[given..].sort_by(|a, b| b.cmp(a));
    found
}

/// The patterns on this computer, read once: the first acadiso.pat found.
pub fn library() -> &'static BTreeMap<String, Vec<PatFamily>> {
    static LIBRARY: OnceLock<BTreeMap<String, Vec<PatFamily>>> = OnceLock::new();
    LIBRARY.get_or_init(|| {
        files()
            .iter()
            .find_map(|path| {
                let bytes = std::fs::read(path).ok()?;
                let patterns = parse(&String::from_utf8_lossy(&bytes));
                (!patterns.is_empty()).then_some(patterns)
            })
            .unwrap_or_default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_pattern_file_gives_each_pattern_s_families() {
        let text = ";; a comment\n*ANSI31, ANSI Iron\n45, 0,0, 0,3.175\n\n*Brick,Bricks ; note\n0, 0,0, 0,6\n90, 0,0, 6,6, 6,-6\n*EMPTY\nnot numbers\n";
        let patterns = parse(text);
        assert_eq!(patterns["ANSI31"], vec![PatFamily { angle: 45.0, x: 0.0, y: 0.0, along: 0.0, across: 3.175, dashes: vec![] }]);
        assert_eq!(patterns["BRICK"].len(), 2);
        assert_eq!(patterns["BRICK"][1].dashes, vec![6.0, -6.0]);
        assert!(!patterns.contains_key("EMPTY"));
    }
}
