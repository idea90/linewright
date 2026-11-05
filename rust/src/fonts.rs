//! Fonts for text in any language.
//!
//! The system's fonts are found by file name and by family name. Text uses the font its drawing names when that
//! is installed (legacy fonts such as the Alice Lao fonts only show correctly in their own font), then a chain of
//! fallback fonts that together cover the world's scripts. Right-to-left text is put in display order here, since
//! the text painters lay glyphs out left to right one by one.

use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use crate::model::geometry::{Point, pt};

/// One font in a file. A collection (.ttc) holds several, picked by index.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct FontFile {
    pub path: PathBuf,
    pub index: u32,
}

/// The interface font, first one found: what the Python version asked Qt for, then the usual Linux fonts.
const UI_FONTS: [&str; 5] =
    ["segoeui.ttf", "NotoSans-Regular.ttf", "Cantarell-Regular.otf", "DejaVuSans.ttf", "LiberationSans-Regular.ttf"];
const MONO_FONTS: [&str; 6] =
    ["CascadiaMono.ttf", "consola.ttf", "NotoSansMono-Regular.ttf", "DejaVuSansMono.ttf", "LiberationMono-Regular.ttf", "cour.ttf"];

/// Fonts for the scripts the interface font lacks. From each group the first one found is used: the one Windows
/// ships, then Noto and other fonts Linux distributions ship.
const FALLBACKS: [&[&str]; 14] = [
    // Lao and Thai fonts whose marks sit over their letter without OpenType positioning come first; Lao UI and
    // Leelawadee UI need it to place them.
    &["Saysettha OT.ttf", "Phetsarath_OT.ttf", "NotoSansLao-Regular.ttf", "NotoSansLaoUI-Regular.ttf", "LaoUI.ttf"],
    &["LeelawUI.ttf", "leelawad.ttf", "NotoSansThai-Regular.ttf", "NotoSansThaiUI-Regular.ttf", "Garuda.ttf", "Loma.ttf"],
    &["NotoSansKhmer-Regular.ttf", "KhmerUI.ttf"],
    &["NotoSansMyanmar-Regular.ttf", "mmrtext.ttf"],
    &["Nirmala.ttc", "Nirmala.ttf", "NotoSansDevanagari-Regular.ttf", "Lohit-Devanagari.ttf"],
    &["msyh.ttc", "msyh.ttf", "NotoSansCJK-Regular.ttc", "NotoSansCJKsc-Regular.otf", "NotoSansSC-Regular.otf", "wqy-microhei.ttc"],
    &["YuGothM.ttc", "meiryo.ttc", "NotoSansCJKjp-Regular.otf", "NotoSansJP-Regular.otf"],
    &["malgun.ttf", "NotoSansCJKkr-Regular.otf", "NotoSansKR-Regular.otf", "NanumGothic.ttf"],
    &["arial.ttf", "NotoSansArabic-Regular.ttf", "NotoNaskhArabic-Regular.ttf"],
    &["NotoSansHebrew-Regular.ttf", "DejaVuSans.ttf"],
    &["NotoSansArmenian-Regular.ttf", "NotoSansGeorgian-Regular.ttf"],
    &["ebrima.ttf", "NotoSansEthiopic-Regular.ttf"],
    &["seguisym.ttf", "NotoSansSymbols2-Regular.ttf", "NotoSansSymbols-Regular.ttf"],
    &["DroidSansFallbackFull.ttf", "DroidSansFallback.ttf"],
];

struct Entry {
    file: FontFile,
    names: Vec<String>, // lower case: family, full name and typographic family, in every language the font has
}

/// The fonts installed on this computer.
pub struct FontCatalog {
    files: Vec<PathBuf>,
    by_file_name: HashMap<String, PathBuf>,
    entries: OnceLock<Vec<Entry>>,
    resolved: Mutex<HashMap<String, Option<FontFile>>>,
}

/// The folders fonts are looked for in: the system's, then any listed in LINEWRIGHT_FONT_DIRS (separated like PATH).
fn font_folders() -> Vec<PathBuf> {
    let mut folders = vec![];
    if let Some(extra) = std::env::var_os("LINEWRIGHT_FONT_DIRS") {
        folders.extend(std::env::split_paths(&extra));
    }
    if cfg!(windows) {
        folders.extend(std::env::var_os("WINDIR").map(|w| PathBuf::from(w).join("Fonts")));
        folders.extend(std::env::var_os("LOCALAPPDATA").map(|l| PathBuf::from(l).join("Microsoft").join("Windows").join("Fonts")));
    } else {
        folders.extend(["/usr/share/fonts", "/usr/local/share/fonts"].map(PathBuf::from));
        if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
            folders.push(home.join(".local/share/fonts"));
            folders.push(home.join(".fonts"));
        }
        if cfg!(target_os = "macos") {
            folders.extend(["/System/Library/Fonts", "/Library/Fonts"].map(PathBuf::from));
        }
    }
    folders
}

fn is_font(path: &Path) -> bool {
    path.extension().is_some_and(|e| ["ttf", "otf", "ttc", "otc"].iter().any(|k| e.eq_ignore_ascii_case(k)))
}

fn collect(folder: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(folder) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() && depth > 0 {
            collect(&path, depth - 1, out);
        } else if is_font(&path) {
            out.push(path);
        }
    }
}

fn read_at(file: &mut File, offset: u64, length: usize) -> Option<Vec<u8>> {
    file.seek(SeekFrom::Start(offset)).ok()?;
    let mut buffer = vec![0; length];
    file.read_exact(&mut buffer).ok()?;
    Some(buffer)
}

fn u16_at(data: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_be_bytes(data.get(at..at + 2)?.try_into().ok()?))
}

fn u32_at(data: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(data.get(at..at + 4)?.try_into().ok()?))
}

/// The names in a font's name table, read without loading the whole file: big fonts are tens of megabytes.
fn font_names(file: &mut File, offset: u64) -> Option<Vec<String>> {
    let header = read_at(file, offset, 12)?;
    let tables = u16_at(&header, 4)? as usize;
    let directory = read_at(file, offset + 12, tables * 16)?;
    let record = (0..tables).map(|i| i * 16).find(|&at| directory.get(at..at + 4) == Some(b"name"))?;
    let (start, length) = (u32_at(&directory, record + 8)? as u64, u32_at(&directory, record + 12)? as usize);
    let table = read_at(file, start, length.min(1 << 20))?;
    let (count, strings) = (u16_at(&table, 2)? as usize, u16_at(&table, 4)? as usize);
    let mut names = vec![];
    for i in 0..count {
        let at = 6 + i * 12;
        let (platform, name_id) = (u16_at(&table, at)?, u16_at(&table, at + 6)?);
        if ![1, 4, 16].contains(&name_id) {
            continue;
        }
        let (size, place) = (u16_at(&table, at + 8)? as usize, u16_at(&table, at + 10)? as usize);
        let Some(bytes) = table.get(strings + place..strings + place + size) else { continue };
        let name = match platform {
            0 | 3 => String::from_utf16_lossy(&bytes.as_chunks::<2>().0.iter().map(|c| u16::from_be_bytes(*c)).collect::<Vec<_>>()),
            1 => bytes.iter().map(|&b| b as char).collect(),
            _ => continue,
        };
        let name = name.trim().to_lowercase();
        if !name.is_empty() && !names.contains(&name) {
            names.push(name);
        }
    }
    Some(names)
}

/// Every font in a file with its names.
fn read_entries(path: &Path) -> Vec<Entry> {
    let Ok(mut file) = File::open(path) else { return vec![] };
    let Some(head) = read_at(&mut file, 0, 12) else { return vec![] };
    let offsets: Vec<u64> = if &head[..4] == b"ttcf" {
        let count = u32_at(&head, 8).unwrap_or(0).min(64) as usize;
        let table = read_at(&mut file, 12, count * 4).unwrap_or_default();
        (0..count).filter_map(|i| u32_at(&table, i * 4)).map(u64::from).collect()
    } else {
        vec![0]
    };
    offsets
        .into_iter()
        .enumerate()
        .filter_map(|(index, offset)| {
            let names = font_names(&mut file, offset)?;
            Some(Entry { file: FontFile { path: path.to_path_buf(), index: index as u32 }, names })
        })
        .collect()
}

/// How well a font stands in for a legacy font of its family. Legacy fonts (the Alice and Saysettha Lao fonts, and
/// many Thai, Khmer and Burmese ones) draw their script at the Latin-1 code points and have no glyphs in the
/// script's own Unicode blocks, while the family's Unicode editions draw Latin there. So: most of Latin-1 covered,
/// then as little of the scripts' blocks as possible, then as much of Latin-1 as possible.
fn legacy_likeness(file: &FontFile) -> (bool, isize, usize) {
    const MOST_OF_LATIN1: usize = 80; // of 96; legacy fonts leave a few codes empty
    let Some(data) = load(file) else { return (false, 0, 0) };
    let Ok(face) = ttf_parser::Face::parse(&data, file.index) else { return (false, 0, 0) };
    let has = |range: std::ops::RangeInclusive<u32>| range.filter_map(char::from_u32).filter(|c| face.glyph_index(*c).is_some()).count();
    let latin1 = has(0xA0..=0xFF);
    let script_glyphs = has(0x0370..=0x1FFF) + has(0x3040..=0x30FF) + has(0x4E00..=0x4EFF) + has(0xAC00..=0xACFF);
    (latin1 >= MOST_OF_LATIN1, -(script_glyphs as isize), latin1)
}

impl FontCatalog {
    fn scan() -> FontCatalog {
        let mut files = vec![];
        for folder in font_folders() {
            collect(&folder, 4, &mut files);
        }
        let by_file_name = files.iter().filter_map(|p| Some((p.file_name()?.to_string_lossy().to_lowercase(), p.clone()))).collect();
        FontCatalog { files, by_file_name, entries: OnceLock::new(), resolved: Mutex::new(HashMap::new()) }
    }

    fn entries(&self) -> &[Entry] {
        self.entries.get_or_init(|| self.files.iter().flat_map(|path| read_entries(path)).collect())
    }

    pub fn find_file(&self, file_name: &str) -> Option<FontFile> {
        let name = Path::new(file_name).file_name()?.to_string_lossy().to_lowercase();
        self.by_file_name.get(&name).map(|path| FontFile { path: path.clone(), index: 0 })
    }

    /// A font by family or full name. When that exact font is missing, one of the same family (the same first word,
    /// such as "Alice4" for "Alice4 2000") stands in: the most legacy-like, since a missing font named like that is
    /// usually a legacy font whose text only reads right in a font that draws the same shapes at the same codes.
    pub fn find_family(&self, family: &str) -> Option<FontFile> {
        let wanted = family.trim().to_lowercase();
        if wanted.is_empty() {
            return None;
        }
        let entries = self.entries();
        if let Some(exact) = entries.iter().find(|e| e.names.contains(&wanted)) {
            return Some(exact.file.clone());
        }
        let first_word = wanted.split_whitespace().next()?;
        let mut kin: Vec<&Entry> =
            entries.iter().filter(|e| e.names.iter().any(|n| n == first_word || n.starts_with(&format!("{first_word} ")))).collect();
        kin.sort_by(|a, b| b.names[0].cmp(&a.names[0])); // so equal ones go to the first name alphabetically
        kin.into_iter().max_by_key(|e| legacy_likeness(&e.file)).map(|e| e.file.clone())
    }

    /// Every family name installed, sorted.
    pub fn family_names(&self) -> Vec<String> {
        let mut names: Vec<String> = self.entries().iter().filter_map(|e| e.names.first().cloned()).collect();
        names.sort();
        names.dedup();
        names
    }

    /// The font a drawing names: a font file ("times.ttf") or a family ("Alice4 2000"). Line fonts (.shx) and
    /// fonts that are not installed give None, and the text uses the interface font.
    pub fn resolve(&self, name: &str) -> Option<FontFile> {
        let key = name.trim().to_lowercase();
        if let Some(found) = self.resolved.lock().ok()?.get(&key) {
            return found.clone();
        }
        let found = if key.ends_with(".shx") || key.is_empty() {
            None
        } else if is_font(Path::new(&key)) {
            self.find_file(&key).or_else(|| self.find_family(Path::new(&key).file_stem()?.to_str()?))
        } else {
            self.find_family(&key)
        };
        self.resolved.lock().ok()?.insert(key, found.clone());
        found
    }

    fn first_file(&self, candidates: &[&str]) -> Option<FontFile> {
        candidates.iter().find_map(|name| self.find_file(name))
    }

    pub fn ui_font(&self) -> Option<FontFile> {
        self.first_file(&UI_FONTS)
    }

    pub fn mono_font(&self) -> Option<FontFile> {
        self.first_file(&MONO_FONTS)
    }

    /// The fonts for scripts the interface font lacks, in the order they are tried.
    pub fn fallbacks(&self) -> Vec<FontFile> {
        let mut found: Vec<FontFile> = vec![];
        for group in FALLBACKS {
            if let Some(file) = self.first_file(group).filter(|f| !found.contains(f)) {
                found.push(file);
            }
        }
        found
    }
}

/// The fonts of this computer, found the first time they are asked for.
pub fn catalog() -> &'static FontCatalog {
    static CATALOG: OnceLock<FontCatalog> = OnceLock::new();
    CATALOG.get_or_init(FontCatalog::scan)
}

/// A font file's bytes, or None when it could not be read.
type FontBytes = Option<Arc<Vec<u8>>>;

/// A font file's bytes, read once.
pub fn load(file: &FontFile) -> FontBytes {
    static LOADED: OnceLock<Mutex<HashMap<PathBuf, FontBytes>>> = OnceLock::new();
    let mut loaded = LOADED.get_or_init(Default::default).lock().ok()?;
    loaded.entry(file.path.clone()).or_insert_with(|| std::fs::read(&file.path).ok().map(Arc::new)).clone()
}

// ------------------------------------------------------------------------------------------------ display order

fn is_right_to_left(c: char) -> bool {
    matches!(c as u32, 0x0590..=0x08FF | 0xFB1D..=0xFDFF | 0xFE70..=0xFEFF)
}

fn is_arabic(c: char) -> bool {
    matches!(c as u32, 0x0600..=0x06FF | 0x0750..=0x077F | 0x08A0..=0x08FF)
}

/// One line of text in the order its glyphs appear from left to right: Arabic letters take their joined forms and
/// right-to-left runs are reversed, as the Unicode bidirectional algorithm says. Other text is left as it is.
pub fn display_line(line: &str) -> String {
    if !line.chars().any(is_right_to_left) {
        return line.to_string();
    }
    let shaped = if line.chars().any(is_arabic) { ar_reshaper::reshape_line(line) } else { line.to_string() };
    let info = unicode_bidi::BidiInfo::new(&shaped, None);
    info.paragraphs.iter().map(|paragraph| info.reorder_line(paragraph, paragraph.range.clone()).into_owned()).collect()
}

/// Every line of a text in display order.
pub fn display_text(text: &str) -> String {
    text.split('\n').map(display_line).collect::<Vec<_>>().join("\n")
}

// ------------------------------------------------------------------------------------------------ outlines

/// A step of a glyph outline, in text units: x along the baseline, y up.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PathOp {
    Move(Point),
    Line(Point),
    Curve(Point, Point, Point),
    Close,
}

struct Outline {
    ops: Vec<PathOp>,
    scale: f64,
    x: f64,
    last: Point,
}

impl Outline {
    fn at(&self, x: f32, y: f32) -> Point {
        pt(self.x + x as f64 * self.scale, y as f64 * self.scale)
    }
}

impl ttf_parser::OutlineBuilder for Outline {
    fn move_to(&mut self, x: f32, y: f32) {
        self.last = self.at(x, y);
        self.ops.push(PathOp::Move(self.last));
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.last = self.at(x, y);
        self.ops.push(PathOp::Line(self.last));
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        // A quadratic curve is the cubic with control points two thirds of the way to the quadratic one.
        let (control, end) = (self.at(x1, y1), self.at(x, y));
        let first = self.last + (control - self.last) * (2.0 / 3.0);
        let second = end + (control - end) * (2.0 / 3.0);
        self.last = end;
        self.ops.push(PathOp::Curve(first, second, end));
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.last = self.at(x, y);
        self.ops.push(PathOp::Curve(self.at(x1, y1), self.at(x2, y2), self.last));
    }
    fn close(&mut self) {
        self.ops.push(PathOp::Close);
    }
}

/// The outlines of one line of text set in the first of the fonts that has each character, `size` high, and how
/// far it reaches along the baseline. Characters no font has are left out.
pub fn outline_line(line: &str, fonts: &[FontFile], size: f64) -> (Vec<PathOp>, f64) {
    let data: Vec<(Arc<Vec<u8>>, u32)> = fonts.iter().filter_map(|f| Some((load(f)?, f.index))).collect();
    let faces: Vec<ttf_parser::Face> = data.iter().filter_map(|(bytes, index)| ttf_parser::Face::parse(bytes, *index).ok()).collect();
    let mut outline = Outline { ops: vec![], scale: 1.0, x: 0.0, last: pt(0.0, 0.0) };
    for c in display_line(line).chars() {
        let Some((face, glyph)) = faces.iter().find_map(|face| Some((face, face.glyph_index(c)?))) else { continue };
        // Sized like the screen's text: the font's full height, ascent to descent, is the text size.
        let height = (face.ascender() as f64 - face.descender() as f64).max(1.0);
        outline.scale = size / height;
        face.outline_glyph(glyph, &mut outline);
        outline.x += face.glyph_hor_advance(glyph).unwrap_or(0) as f64 * outline.scale;
    }
    let width = outline.x;
    (outline.ops, width)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn right_to_left_text_is_put_in_display_order() {
        assert_eq!(display_line("abc"), "abc");
        assert_eq!(display_line("שלום"), "םולש");
        let arabic = display_line("سلام");
        assert!(arabic.chars().all(|c| !is_arabic(c)), "joined forms come from the presentation forms block: {arabic:?}");
        assert_eq!(display_text("A\nשלום"), "A\nםולש");
    }

    #[test]
    fn names_are_read_from_a_font_file() {
        let Some(file) = catalog().ui_font() else { return }; // no fonts on this computer
        let entries = read_entries(&file.path);
        assert!(!entries.is_empty() && !entries[0].names.is_empty(), "{:?}", file.path);
        let family = entries[0].names[0].clone();
        let found = catalog().find_family(&family).expect("the family is found");
        assert!(read_entries(&found.path).iter().any(|e| e.names.contains(&family)));
    }

    #[test]
    fn text_becomes_outlines() {
        let Some(file) = catalog().ui_font() else { return };
        let (ops, width) = outline_line("Hi", &[file], 10.0);
        assert!(ops.iter().any(|op| matches!(op, PathOp::Close)));
        assert!(width > 5.0 && width < 20.0, "{width}");
    }
}
