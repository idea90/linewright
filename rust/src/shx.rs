//! AutoCAD's SHX stroke fonts (romans.shx, simplex.shx, txt.shx…): read from the font files AutoCAD installs, and
//! turned into the strokes AutoCAD draws for text, so text in them looks and measures as it does there.
//!
//! Both kinds of text font are read: the older shapes files (one byte per character) and unifonts (any Unicode
//! character). Each character is a little program of pen moves: vectors in sixteen directions, displacements,
//! arcs by octant, fraction or bulge, sub-shapes, scale changes and a position stack.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use crate::fonts::display_text;
use crate::model::geometry::{Point, direction, pt};
use crate::model::parts::TextSpec;

/// One character: its strokes in font units from its origin on the baseline, and where the next one starts.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Glyph {
    pub strokes: Vec<Vec<Point>>,
    pub advance: Point,
}

/// A stroke font: how tall its capitals are and how far below the baseline it reaches, in font units, and each
/// character's definition bytes.
#[derive(Debug)]
pub struct ShxFont {
    pub above: f64,
    pub below: f64,
    /// Unifonts number their sub-shapes in two bytes, shapes files in one.
    unicode: bool,
    shapes: HashMap<u32, Vec<u8>>,
    glyphs: Mutex<HashMap<u32, Option<Arc<Glyph>>>>,
}

fn u16_at(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes([*bytes.get(at)?, *bytes.get(at + 1)?]))
}

fn u32_at(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes([*bytes.get(at)?, *bytes.get(at + 1)?, *bytes.get(at + 2)?, *bytes.get(at + 3)?]))
}

/// A shape's definition with its name taken off: the bytes after the name's ending zero.
fn without_name(definition: &[u8]) -> Vec<u8> {
    let start = definition.iter().position(|b| *b == 0).map_or(0, |i| i + 1);
    definition[start..].to_vec()
}

impl ShxFont {
    /// Read a font file's bytes. None for a file that is not an SHX text font (or a big font, read elsewhere).
    pub fn parse(bytes: &[u8]) -> Option<ShxFont> {
        let header_end = bytes.iter().position(|b| *b == 0x1a)?;
        let header = String::from_utf8_lossy(&bytes[..header_end]).to_lowercase();
        let mut at = header_end + 1;
        let mut shapes: HashMap<u32, Vec<u8>> = HashMap::new();
        let unicode = header.contains("unifont");
        if unicode {
            let count = u32_at(bytes, at)?;
            let info_length = u16_at(bytes, at + 4)? as usize;
            at += 6;
            shapes.insert(0, bytes.get(at..at + info_length)?.to_vec());
            at += info_length;
            for _ in 1..count {
                let (Some(number), Some(length)) = (u16_at(bytes, at), u16_at(bytes, at + 2)) else { break };
                at += 4;
                let Some(definition) = bytes.get(at..at + length as usize) else { break };
                shapes.insert(number as u32, without_name(definition));
                at += length as usize;
            }
        } else if header.contains("shapes") {
            let count = u16_at(bytes, at + 4)? as usize;
            at += 6;
            let index: Vec<(u16, u16)> =
                (0..count).filter_map(|i| Some((u16_at(bytes, at + i * 4)?, u16_at(bytes, at + i * 4 + 2)?))).collect();
            at += count * 4;
            for (number, length) in index {
                let Some(definition) = bytes.get(at..at + length as usize) else { break };
                shapes.insert(number as u32, if number == 0 { definition.to_vec() } else { without_name(definition) });
                at += length as usize;
            }
        } else {
            return None;
        }
        // Shape 0 says how tall the font is: its name, then the height above and below the baseline.
        let info = shapes.remove(&0)?;
        let after_name = info.iter().position(|b| *b == 0)? + 1;
        let above = *info.get(after_name)? as f64;
        let below = *info.get(after_name + 1)? as f64;
        (above > 0.0).then(|| ShxFont { above, below, unicode, shapes, glyphs: Mutex::new(HashMap::new()) })
    }

    pub fn has(&self, code: u32) -> bool {
        self.shapes.contains_key(&code)
    }

    /// A character's strokes, worked out once.
    pub fn glyph(&self, code: u32) -> Option<Arc<Glyph>> {
        if let Some(known) = self.glyphs.lock().ok()?.get(&code) {
            return known.clone();
        }
        let glyph = self.shapes.get(&code).map(|bytes| {
            let mut pen = Pen::default();
            pen.run(self, bytes, 0);
            pen.finish();
            Arc::new(Glyph { strokes: pen.strokes, advance: pen.position })
        });
        self.glyphs.lock().ok()?.insert(code, glyph.clone());
        glyph
    }
}

/// Direction of each of the sixteen vector directions, counter-clockwise from east.
const DIRECTIONS: [(f64, f64); 16] = [
    (1.0, 0.0),
    (1.0, 0.5),
    (1.0, 1.0),
    (0.5, 1.0),
    (0.0, 1.0),
    (-0.5, 1.0),
    (-1.0, 1.0),
    (-1.0, 0.5),
    (-1.0, 0.0),
    (-1.0, -0.5),
    (-1.0, -1.0),
    (-0.5, -1.0),
    (0.0, -1.0),
    (0.5, -1.0),
    (1.0, -1.0),
    (1.0, -0.5),
];

/// How deep sub-shapes may call sub-shapes, against fonts that loop.
const MAX_DEPTH: usize = 8;
/// Straight pieces per 45 degrees of arc.
const ARC_STEPS_PER_OCTANT: usize = 4;

/// The pen that runs a character's program.
struct Pen {
    position: Point,
    down: bool,
    scale: f64,
    stack: Vec<Point>,
    strokes: Vec<Vec<Point>>,
    current: Vec<Point>,
}

impl Default for Pen {
    fn default() -> Self {
        Pen { position: pt(0.0, 0.0), down: true, scale: 1.0, stack: vec![], strokes: vec![], current: vec![] }
    }
}

impl Pen {
    fn finish(&mut self) {
        if self.current.len() >= 2 {
            self.strokes.push(std::mem::take(&mut self.current));
        }
        self.current.clear();
    }

    fn lift(&mut self) {
        self.down = false;
        self.finish();
    }

    fn move_to(&mut self, to: Point) {
        if self.down {
            if self.current.is_empty() {
                self.current.push(self.position);
            }
            self.current.push(to);
        }
        self.position = to;
    }

    fn arc(&mut self, center: Point, radius: f64, start: f64, sweep: f64) {
        let steps = ((sweep.abs() / 45.0) * ARC_STEPS_PER_OCTANT as f64).ceil().max(1.0) as usize;
        for i in 1..=steps {
            let a = (start + sweep * i as f64 / steps as f64).to_radians();
            self.move_to(center + pt(a.cos(), a.sin()) * radius);
        }
    }

    /// A chord with a bulge, as polylines draw arcs: bulge is the tangent of a quarter of the arc's angle.
    fn bulge_to(&mut self, offset: Point, bulge: f64) {
        let end = self.position + offset;
        if bulge == 0.0 || offset.length() == 0.0 {
            self.move_to(end);
            return;
        }
        let sweep = 4.0 * bulge.atan();
        let chord = offset.length();
        let radius = chord / (2.0 * (sweep / 2.0).sin().abs());
        let middle = self.position.midpoint(end);
        let toward_center = pt(-offset.y, offset.x) * (1.0 / chord);
        let distance = radius * (sweep / 2.0).cos().abs();
        let side = if sweep.abs() > std::f64::consts::PI { -1.0 } else { 1.0 } * sweep.signum();
        let center = middle + toward_center * (distance * side);
        let start = (self.position.y - center.y).atan2(self.position.x - center.x).to_degrees();
        self.arc(center, radius, start, sweep.to_degrees());
        self.position = end;
    }

    /// Run a definition. A command after code 14 only counts for vertical text, so it is read and left out.
    fn run(&mut self, font: &ShxFont, bytes: &[u8], depth: usize) {
        let mut at = 0;
        let mut skip_next = false;
        while at < bytes.len() {
            let skip = std::mem::take(&mut skip_next);
            let code = bytes[at];
            at += 1;
            let byte = |i: usize| bytes.get(i).copied().unwrap_or(0);
            let signed = |i: usize| byte(i) as i8 as f64;
            match code {
                0 => break,
                1 if !skip => self.down = true,
                2 if !skip => self.lift(),
                1 | 2 => {}
                3 => {
                    if !skip && byte(at) != 0 {
                        self.scale /= byte(at) as f64;
                    }
                    at += 1;
                }
                4 => {
                    if !skip {
                        self.scale *= byte(at) as f64;
                    }
                    at += 1;
                }
                5 if !skip => self.stack.push(self.position),
                6 if !skip => {
                    if let Some(back) = self.stack.pop() {
                        self.finish();
                        self.position = back;
                    }
                }
                5 | 6 => {}
                7 => {
                    let number = if font.unicode {
                        let n = u16::from_be_bytes([byte(at), byte(at + 1)]) as u32;
                        at += 2;
                        n
                    } else {
                        at += 1;
                        byte(at - 1) as u32
                    };
                    if !skip
                        && depth < MAX_DEPTH
                        && let Some(sub) = font.shapes.get(&number)
                    {
                        let sub = sub.clone();
                        self.run(font, &sub, depth + 1);
                    }
                }
                8 => {
                    if !skip {
                        self.move_to(self.position + pt(signed(at), signed(at + 1)) * self.scale);
                    }
                    at += 2;
                }
                9 => loop {
                    let (dx, dy) = (signed(at), signed(at + 1));
                    at += 2;
                    if (dx == 0.0 && dy == 0.0) || at > bytes.len() {
                        break;
                    }
                    if !skip {
                        self.move_to(self.position + pt(dx, dy) * self.scale);
                    }
                },
                10 => {
                    let (radius, flags) = (byte(at) as f64 * self.scale, byte(at + 1));
                    at += 2;
                    if !skip {
                        let clockwise = flags & 0x80 != 0;
                        let start = ((flags >> 4) & 7) as f64 * 45.0;
                        let count = match flags & 7 {
                            0 => 8,
                            n => n,
                        } as f64;
                        let a = start.to_radians();
                        let center = self.position - pt(a.cos(), a.sin()) * radius;
                        self.arc(center, radius, start, if clockwise { -count * 45.0 } else { count * 45.0 });
                    }
                }
                11 => {
                    let (start_offset, end_offset) = (byte(at) as f64, byte(at + 1) as f64);
                    let radius = (byte(at + 2) as f64 * 256.0 + byte(at + 3) as f64) * self.scale;
                    let flags = byte(at + 4);
                    at += 5;
                    if !skip {
                        let turn = if flags & 0x80 != 0 { -1.0 } else { 1.0 };
                        let octant = ((flags >> 4) & 7) as f64;
                        let count = match flags & 7 {
                            0 => 8,
                            n => n,
                        } as f64;
                        let start = octant * 45.0 + turn * start_offset * 45.0 / 256.0;
                        let last = octant + turn * (count - 1.0);
                        let end = if end_offset == 0.0 { (last + turn) * 45.0 } else { last * 45.0 + turn * end_offset * 45.0 / 256.0 };
                        let a = start.to_radians();
                        let center = self.position - pt(a.cos(), a.sin()) * radius;
                        self.arc(center, radius, start, end - start);
                    }
                }
                12 => {
                    let (dx, dy, bulge) = (signed(at), signed(at + 1), signed(at + 2));
                    at += 3;
                    if !skip {
                        self.bulge_to(pt(dx, dy) * self.scale, bulge / 127.0);
                    }
                }
                13 => loop {
                    let (dx, dy) = (signed(at), signed(at + 1));
                    at += 2;
                    if (dx == 0.0 && dy == 0.0) || at > bytes.len() {
                        break;
                    }
                    let bulge = signed(at);
                    at += 1;
                    if !skip {
                        self.bulge_to(pt(dx, dy) * self.scale, bulge / 127.0);
                    }
                },
                14 => skip_next = true,
                vector => {
                    if !skip {
                        let length = (vector >> 4) as f64 * self.scale;
                        let (x, y) = DIRECTIONS[(vector & 0x0f) as usize];
                        self.move_to(self.position + pt(x, y) * length);
                    }
                }
            }
        }
    }
}

/// Where else a font may keep a character. Unifonts keep the diameter sign (%%c) at U+2205; older shapes fonts
/// keep degree, plus/minus and diameter at 127, 128 and 129.
fn stand_ins(c: char) -> &'static [u32] {
    match c {
        '\u{2300}' | '\u{2205}' => &[0x2205, 0x2300, 129, 0xd8],
        '°' => &[127],
        '±' => &[128],
        _ => &[],
    }
}

/// A line of text in an SHX font: its strokes from the baseline's left end, sized so capitals are `height` tall
/// (squeezed or stretched by `width`), and how long it is. Characters the font lacks become '?'.
pub fn line_strokes(font: &ShxFont, line: &str, height: f64, width: f64) -> (Vec<Vec<Point>>, f64) {
    let scale = height / font.above;
    let mut strokes = vec![];
    let mut origin = pt(0.0, 0.0);
    for c in line.chars() {
        let glyph =
            font.glyph(c as u32).or_else(|| stand_ins(c).iter().find_map(|code| font.glyph(*code))).or_else(|| font.glyph('?' as u32));
        let Some(glyph) = glyph else {
            origin.x += height * 0.6 / scale;
            continue;
        };
        let place = |p: Point| pt((origin.x + p.x) * scale * width, (origin.y + p.y) * scale);
        strokes.extend(glyph.strokes.iter().map(|s| s.iter().map(|p| place(*p)).collect::<Vec<Point>>()));
        origin = origin + glyph.advance;
    }
    (strokes, origin.x * scale * width)
}

/// A piece of text in an SHX font, as strokes in drawing units: each line aligned and turned as the text says.
pub fn text_strokes(font: &ShxFont, spec: &TextSpec) -> Vec<Vec<Point>> {
    let shown = display_text(&spec.text);
    let lines: Vec<(Vec<Vec<Point>>, f64)> =
        shown.split('\n').map(|line| line_strokes(font, line, spec.height, spec.look.width_factor)).collect();
    let shear = spec.look.shear();
    let widths: Vec<f64> = lines.iter().map(|(_, width)| *width).collect();
    let (along, up) = (direction(spec.rotation), direction(spec.rotation + 90.0));
    lines
        .into_iter()
        .zip(spec.line_origins(&widths))
        .flat_map(|((strokes, _), start)| {
            strokes.into_iter().map(move |s| s.into_iter().map(|p| start + along * (p.x + p.y * shear) + up * p.y).collect())
        })
        .collect()
}

/// The names of the SHX text fonts to choose from, in AutoCAD's folders.
pub fn available() -> Vec<String> {
    let mut names: Vec<String> = folders()
        .iter()
        .flat_map(|folder| std::fs::read_dir(folder).into_iter().flatten().flatten())
        .map(|e| e.file_name().to_string_lossy().to_lowercase())
        .filter(|name| name.ends_with(".shx"))
        .collect();
    names.sort();
    names.dedup();
    names
}

/// The fonts read so far, by lower-case file name; None for one that is nowhere to be found.
fn cache() -> &'static Mutex<HashMap<String, Option<Arc<ShxFont>>>> {
    static CACHE: OnceLock<Mutex<HashMap<String, Option<Arc<ShxFont>>>>> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

/// The folders AutoCAD keeps its fonts in, newest version first, on Windows or (for the same disk seen from WSL)
/// under /mnt/c; then any listed in LINEWRIGHT_FONT_DIRS.
fn font_folders() -> Vec<PathBuf> {
    let mut folders: Vec<PathBuf> =
        std::env::var_os("LINEWRIGHT_FONT_DIRS").map(|v| std::env::split_paths(&v).collect()).unwrap_or_default();
    let mut programs: Vec<PathBuf> =
        ["ProgramFiles", "ProgramFiles(x86)", "ProgramW6432"].iter().filter_map(std::env::var_os).map(PathBuf::from).collect();
    programs.extend(["/mnt/c/Program Files", "/mnt/c/Program Files (x86)"].map(PathBuf::from));
    let mut found = vec![];
    for program_files in programs {
        // AutoCAD 2021\Fonts under Autodesk, and older products such as "Autodesk Land Desktop 2006\Fonts".
        let candidates = std::fs::read_dir(program_files.join("Autodesk")).into_iter().flatten().chain(
            std::fs::read_dir(&program_files)
                .into_iter()
                .flatten()
                .filter(|e| e.as_ref().is_ok_and(|e| e.file_name().to_string_lossy().starts_with("Autodesk "))),
        );
        for entry in candidates.flatten() {
            let fonts = entry.path().join("Fonts");
            if fonts.is_dir() {
                found.push(fonts);
            }
        }
    }
    found.sort_by(|a, b| b.cmp(a));
    found.dedup();
    folders.extend(found);
    folders
}

fn folders() -> &'static [PathBuf] {
    static FOLDERS: OnceLock<Vec<PathBuf>> = OnceLock::new();
    FOLDERS.get_or_init(font_folders)
}

/// Folders of drawings opened, searched first as AutoCAD searches the drawing's own folder.
fn drawing_folders() -> &'static Mutex<Vec<PathBuf>> {
    static FOLDERS: OnceLock<Mutex<Vec<PathBuf>>> = OnceLock::new();
    FOLDERS.get_or_init(Default::default)
}

/// Look for fonts beside a drawing too; fonts that were missing get looked for again.
pub fn add_drawing_folder(folder: &Path) {
    let Ok(mut known) = drawing_folders().lock() else { return };
    if !known.iter().any(|f| f == folder) {
        known.insert(0, folder.to_path_buf());
        if let Ok(mut cache) = cache().lock() {
            cache.retain(|_, font| font.is_some());
        }
    }
}

/// The file an SHX font is in: beside a drawing, or in AutoCAD's font folders. File names match in any case.
fn find(file_name: &str) -> Option<PathBuf> {
    let wanted = file_name.to_lowercase();
    let beside = drawing_folders().lock().map(|f| f.clone()).unwrap_or_default();
    beside.iter().chain(folders()).find_map(|folder| {
        std::fs::read_dir(folder)
            .ok()?
            .flatten()
            .map(|e| e.path())
            .find(|p| p.file_name().is_some_and(|n| n.to_string_lossy().to_lowercase() == wanted))
    })
}

fn load(file: &str) -> Option<Arc<ShxFont>> {
    if let Some(known) = cache().lock().ok()?.get(file) {
        return known.clone();
    }
    let read = find(file).and_then(|path| std::fs::read(path).ok()).and_then(|bytes| ShxFont::parse(&bytes)).map(Arc::new);
    cache().lock().ok()?.insert(file.to_string(), read.clone());
    read
}

/// The SHX font a drawing's text asks for, read once. A named .shx file that is nowhere to be found is replaced
/// by simplex.shx, as AutoCAD does with FONTALT. A bare name like "txt" counts only when that file is there, since
/// it may as well be a TrueType family. None when the text is not in an SHX font, or no font file is found.
pub fn text_font(name: &str) -> Option<Arc<ShxFont>> {
    let file = name.trim().to_lowercase();
    if file.ends_with(".shx") {
        load(&file).or_else(|| load("simplex.shx"))
    } else if !file.is_empty() && !file.contains('.') {
        load(&format!("{file}.shx"))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A unifont with one character: an 'L' of vectors, then a pen-up move on to the next character.
    fn tiny_unifont() -> Vec<u8> {
        let mut bytes = b"AutoCAD-86 unifont 1.0\r\n\x1a".to_vec();
        let info: Vec<u8> = b"Tiny\0".iter().copied().chain([8u8, 2, 0, 0, 0, 0]).collect();
        bytes.extend(2u32.to_le_bytes());
        bytes.extend((info.len() as u16).to_le_bytes());
        bytes.extend(&info);
        // 'L': pen up, 8 north; pen down, 8 south, 4 east; pen up, 2 east.
        let l: Vec<u8> = b"L\0".iter().copied().chain([2u8, 0x84, 1, 0x8C, 0x40, 2, 0x20, 0]).collect();
        bytes.extend((b'L' as u16).to_le_bytes());
        bytes.extend((l.len() as u16).to_le_bytes());
        bytes.extend(&l);
        bytes
    }

    #[test]
    fn a_unifont_character_becomes_its_strokes_and_advance() {
        let font = ShxFont::parse(&tiny_unifont()).unwrap();
        assert_eq!((font.above, font.below), (8.0, 2.0));
        let glyph = font.glyph('L' as u32).unwrap();
        assert_eq!(glyph.strokes, vec![vec![pt(0.0, 8.0), pt(0.0, 0.0), pt(4.0, 0.0)]]);
        assert_eq!(glyph.advance, pt(6.0, 0.0));
        let (strokes, width) = line_strokes(&font, "LL", 4.0, 1.0);
        assert_eq!(strokes.len(), 2);
        assert_eq!(width, 6.0, "two characters 6 apart at half size");
        let (_, missing) = line_strokes(&font, "⌀", 4.0, 1.0);
        assert!((missing - 2.4).abs() < 1e-9, "a font with no '?' either leaves a gap for a character it lacks");
    }

    #[test]
    fn arcs_and_bulges_end_where_they_should() {
        let mut pen = Pen::default();
        let font = ShxFont::parse(&tiny_unifont()).unwrap();
        // A half circle of radius 4, counter-clockwise from octant 0: from (0,0) round to (-8,0).
        pen.run(&font, &[10, 4, 0x04, 0], 0);
        assert!(pen.position.dist(pt(-8.0, 0.0)) < 1e-9);
        let mut pen = Pen::default();
        // A semicircle by bulge: 10 across, bulge 1.
        pen.run(&font, &[12, 10, 0, 127, 0], 0);
        assert!(pen.position.dist(pt(10.0, 0.0)) < 1e-9);
        pen.finish();
        let top = pen.strokes[0].iter().map(|p| p.y.abs()).fold(0.0, f64::max);
        assert!((top - 5.0).abs() < 0.2, "it bows out by the radius: {top}");
    }
}
