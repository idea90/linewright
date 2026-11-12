//! Plot style tables, as AutoCAD's Plot Style Table Editor saves them: color-dependent ones (.ctb) give each of the
//! 255 index colors the color it prints in, whether in grays, how strongly (screening) and with what lineweight;
//! named ones (.stb) give the same to plot styles by name, which layers and objects choose.
//!
//! A table is a short text header, then a zlib-compressed text body of `key=value` lines and `name{ … }` blocks.
//! Tables are found where AutoCAD keeps them; monochrome.ctb, grayscale.ctb and acad.ctb work without AutoCAD too.

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

/// Color values that mean "the object's own color".
const OBJECT_COLOR: i64 = -1;
const OBJECT_COLOR2: i64 = -1006632961; // 0xC3FFFFFF
/// color_policy bits.
const GRAYSCALE: i64 = 2;
/// AutoCAD's lineweight table, in millimeters, for tables that do not carry their own.
const LINEWEIGHTS_MM: [f64; 27] = [
    0.0, 0.05, 0.09, 0.1, 0.13, 0.15, 0.18, 0.2, 0.25, 0.3, 0.35, 0.4, 0.45, 0.5, 0.53, 0.6, 0.65, 0.7, 0.8, 0.9, 1.0, 1.06, 1.2, 1.4,
    1.58, 2.0, 2.11,
];

/// How one index color prints.
#[derive(Clone, Debug, PartialEq)]
pub struct PenStyle {
    /// The color it prints in, or None for the object's own.
    pub color: Option<(u8, u8, u8)>,
    pub grayscale: bool,
    /// Ink strength in percent: 100 prints the color as it is, 0 prints white.
    pub screen: u8,
    /// Its lineweight in millimeters, or None for the object's own.
    pub lineweight: Option<f64>,
}

impl Default for PenStyle {
    fn default() -> Self {
        PenStyle { color: None, grayscale: false, screen: 100, lineweight: None }
    }
}

impl PenStyle {
    /// The color something of `object` color prints in.
    pub fn apply(&self, object: (u8, u8, u8)) -> (u8, u8, u8) {
        let (mut r, mut g, mut b) = self.color.unwrap_or(object);
        if self.grayscale {
            let gray = (0.299 * r as f64 + 0.587 * g as f64 + 0.114 * b as f64).round() as u8;
            (r, g, b) = (gray, gray, gray);
        }
        let screen = self.screen.min(100) as u32;
        let ink = |c: u8| (255 - (255 - c as u32) * screen / 100) as u8;
        (ink(r), ink(g), ink(b))
    }
}

/// The plot style every named table has, which prints objects as they are.
pub const NORMAL_STYLE: &str = "Normal";

#[derive(Clone, Debug, PartialEq)]
pub struct PlotStyleTable {
    pub description: String,
    /// The pens: for index colors 1 to 255 in order, or for the named styles in `names`.
    pub pens: Vec<PenStyle>,
    /// A named table's style names, one for each pen; empty for a color-dependent table.
    pub names: Vec<String>,
}

impl PlotStyleTable {
    /// The pen for an index color (1–255); others take the object's color and lineweight.
    pub fn pen(&self, aci: i32) -> PenStyle {
        usize::try_from(aci - 1).ok().and_then(|i| self.pens.get(i)).cloned().unwrap_or_default()
    }

    /// Whether this is a named table (.stb), whose pens go by the plot style a layer or object names.
    pub fn is_named(&self) -> bool {
        !self.names.is_empty()
    }

    /// The pen of a named plot style, matched as AutoCAD matches names, whatever their case. Normal, and a name the
    /// table does not have, print objects as they are.
    pub fn named_pen(&self, name: &str) -> PenStyle {
        self.names.iter().position(|n| n.eq_ignore_ascii_case(name.trim())).and_then(|i| self.pens.get(i)).cloned().unwrap_or_default()
    }

    fn uniform(description: &str, pen: PenStyle) -> PlotStyleTable {
        PlotStyleTable { description: description.into(), pens: vec![pen; 255], names: vec![] }
    }

    /// A named table whose one style besides Normal prints every object with this pen, and Normal does too.
    fn named(description: &str, pen: PenStyle) -> PlotStyleTable {
        PlotStyleTable { description: description.into(), pens: vec![pen.clone(), pen], names: vec![NORMAL_STYLE.into(), "Style 1".into()] }
    }
}

/// The text inside a .ctb file. None for a file that is not a compressed plot style table.
fn body(bytes: &[u8]) -> Option<String> {
    let marker = b"pmzlibcodec";
    let at = bytes.windows(marker.len()).position(|w| w == marker)? + marker.len();
    // An Adler checksum and the two sizes, then the zlib stream.
    let mut text = Vec::new();
    flate2::read::ZlibDecoder::new(bytes.get(at + 12..)?).read_to_end(&mut text).ok()?;
    Some(text.iter().map(|b| *b as char).collect())
}

/// Read a .ctb file's bytes.
pub fn parse(bytes: &[u8]) -> Option<PlotStyleTable> {
    let text = body(bytes)?;
    let value = |line: &str, key: &str| {
        line.trim().strip_prefix(key).and_then(|v| v.strip_prefix('=')).map(|v| v.trim_start_matches('"').to_string())
    };
    let description = text.lines().find_map(|l| value(l, "description")).unwrap_or_default();
    // The lineweight table, in millimeters by index.
    let mut lineweights: Vec<f64> = LINEWEIGHTS_MM.to_vec();
    if let Some(start) = text.find("custom_lineweight_table{") {
        for line in text[start..].lines().skip(1).take_while(|l| !l.trim().starts_with('}')) {
            if let Some((index, mm)) = line.trim().split_once('=')
                && let (Ok(index), Ok(mm)) = (index.parse::<usize>(), mm.parse::<f64>())
            {
                if index >= lineweights.len() {
                    lineweights.resize(index + 1, 0.0);
                }
                lineweights[index] = mm;
            }
        }
    }
    // A named table says it has no table by color index.
    let named = text.lines().find_map(|l| value(l, "aci_table_available")).is_some_and(|v| v.trim().eq_ignore_ascii_case("FALSE"));
    let start = text.find("plot_style{")?;
    let (mut pens, mut names) = (vec![], vec![]);
    let mut pen: Option<HashMap<String, String>> = None;
    for line in text[start..].lines().skip(1) {
        let trimmed = line.trim();
        if trimmed.ends_with('{') {
            pen = Some(HashMap::new());
        } else if trimmed == "}" {
            match pen.take() {
                Some(fields) => {
                    names.push(fields.get("name").cloned().unwrap_or_default());
                    pens.push(pen_style(&fields, &lineweights));
                }
                None => break, // the end of plot_style
            }
        } else if let (Some(fields), Some((key, v))) = (pen.as_mut(), trimmed.split_once('=')) {
            fields.insert(key.to_string(), v.trim_start_matches('"').to_string());
        }
    }
    if !named {
        names.clear();
    }
    (!pens.is_empty()).then_some(PlotStyleTable { description, pens, names })
}

fn pen_style(fields: &HashMap<String, String>, lineweights: &[f64]) -> PenStyle {
    let int = |key: &str| fields.get(key).and_then(|v| v.trim().parse::<i64>().ok());
    let color = match int("color").unwrap_or(OBJECT_COLOR) {
        OBJECT_COLOR | OBJECT_COLOR2 => None,
        value => Some(((value >> 16) as u8, (value >> 8) as u8, value as u8)),
    };
    // Lineweight 0 is the table's first entry, "use object lineweight".
    let lineweight = match int("lineweight").unwrap_or(0) {
        0 => None,
        index => usize::try_from(index).ok().and_then(|i| lineweights.get(i)).copied(),
    };
    PenStyle {
        color,
        grayscale: int("color_policy").unwrap_or(1) & GRAYSCALE != 0,
        screen: int("screen").unwrap_or(100).clamp(0, 100) as u8,
        lineweight,
    }
}

/// AutoCAD's standard tables, for when their files are not on this computer.
fn built_in(name: &str) -> Option<PlotStyleTable> {
    match name.to_lowercase().as_str() {
        "monochrome.ctb" => {
            Some(PlotStyleTable::uniform("All colors print black", PenStyle { color: Some((0, 0, 0)), ..Default::default() }))
        }
        "grayscale.ctb" => Some(PlotStyleTable::uniform("Colors print as grays", PenStyle { grayscale: true, ..Default::default() })),
        "acad.ctb" => Some(PlotStyleTable::uniform("Objects print as they are", PenStyle::default())),
        "acad.stb" => Some(PlotStyleTable::named("Objects print as they are", PenStyle::default())),
        "monochrome.stb" => {
            Some(PlotStyleTable::named("All objects print black", PenStyle { color: Some((0, 0, 0)), ..Default::default() }))
        }
        _ => None,
    }
}

const BUILT_IN: [&str; 5] = ["acad.ctb", "monochrome.ctb", "grayscale.ctb", "acad.stb", "monochrome.stb"];

/// Folders named "Plot Styles" under a folder, a few levels down.
fn plot_style_folders(root: &Path, depth: usize, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(root) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if !entry.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        if entry.file_name().to_string_lossy().eq_ignore_ascii_case("Plot Styles") {
            out.push(path);
        } else if depth > 0 {
            plot_style_folders(&path, depth - 1, out);
        }
    }
}

/// The latest year a path names, such as 2021 in "AutoCAD 2021"; 0 for none.
fn product_year(path: &str) -> u32 {
    path.split(|c: char| !c.is_ascii_digit()).filter_map(|n| n.parse::<u32>().ok()).filter(|y| (1990..2100).contains(y)).max().unwrap_or(0)
}

/// Where AutoCAD keeps plot style tables: each product's Plotters\Plot Styles in the user's roaming settings, and
/// the copies under Program Files; on WSL, the same folders under /mnt/c. LINEWRIGHT_PLOT_STYLES comes first.
fn search_folders() -> &'static [PathBuf] {
    static FOLDERS: OnceLock<Vec<PathBuf>> = OnceLock::new();
    FOLDERS.get_or_init(|| {
        let mut folders: Vec<PathBuf> =
            std::env::var_os("LINEWRIGHT_PLOT_STYLES").map(|v| std::env::split_paths(&v).collect()).unwrap_or_default();
        let mut roots: Vec<PathBuf> =
            ["APPDATA", "ProgramFiles"].iter().filter_map(std::env::var_os).map(|v| PathBuf::from(v).join("Autodesk")).collect();
        if let Ok(users) = std::fs::read_dir("/mnt/c/Users") {
            roots.extend(users.flatten().map(|u| u.path().join("AppData/Roaming/Autodesk")));
        }
        roots.push(PathBuf::from("/mnt/c/Program Files/Autodesk"));
        let mut found = vec![];
        for root in roots {
            plot_style_folders(&root, 4, &mut found);
        }
        // A user's own settings before Program Files, and newer products (by the year in their name) first.
        found.sort_by_key(|f| {
            let path = f.to_string_lossy().into_owned();
            (path.contains("Program Files"), std::cmp::Reverse(product_year(&path)), path)
        });
        found.dedup();
        folders.extend(found);
        folders
    })
}

/// The names of the plot style tables there are to choose from: AutoCAD's own and any in its folders.
pub fn available() -> Vec<String> {
    let mut names: Vec<String> = BUILT_IN.iter().map(|n| n.to_string()).collect();
    for folder in search_folders() {
        for entry in std::fs::read_dir(folder).into_iter().flatten().flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let table = name.to_lowercase().ends_with(".ctb") || name.to_lowercase().ends_with(".stb");
            if table && !names.iter().any(|n| n.eq_ignore_ascii_case(&name)) {
                names.push(name);
            }
        }
    }
    names.sort_by_key(|n| n.to_lowercase());
    names
}

fn cache() -> &'static Mutex<HashMap<String, Option<Arc<PlotStyleTable>>>> {
    static CACHE: OnceLock<Mutex<HashMap<String, Option<Arc<PlotStyleTable>>>>> = OnceLock::new();
    CACHE.get_or_init(Default::default)
}

/// A plot style table by file name, from beside the drawing, AutoCAD's folders, or the built-in ones. None when
/// none of those has it or it cannot be read.
pub fn load(name: &str, beside: Option<&Path>) -> Option<Arc<PlotStyleTable>> {
    let key = name.trim().to_lowercase();
    if key.is_empty() {
        return None;
    }
    if let Some(known) = cache().lock().ok()?.get(&key) {
        return known.clone();
    }
    let file_name = Path::new(name.trim()).file_name()?.to_string_lossy().to_lowercase();
    let found = beside.into_iter().chain(search_folders().iter().map(PathBuf::as_path)).find_map(|folder| {
        let path = std::fs::read_dir(folder)
            .ok()?
            .flatten()
            .map(|e| e.path())
            .find(|p| p.file_name().is_some_and(|n| n.to_string_lossy().to_lowercase() == file_name))?;
        parse(&std::fs::read(path).ok()?)
    });
    let table = found.or_else(|| built_in(&file_name)).map(Arc::new);
    cache().lock().ok()?.insert(key, table.clone());
    table
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// A table as the Plot Style Table Editor writes one, around a body.
    fn ctb(body: &str) -> Vec<u8> {
        let mut compressed = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        compressed.write_all(body.as_bytes()).unwrap();
        let compressed = compressed.finish().unwrap();
        let mut bytes = b"PIAFILEVERSION_2.0,CTBVER1,compress\r\npmzlibcodec".to_vec();
        bytes.extend(0u32.to_le_bytes());
        bytes.extend((body.len() as u32).to_le_bytes());
        bytes.extend((compressed.len() as u32).to_le_bytes());
        bytes.extend(compressed);
        bytes
    }

    #[test]
    fn a_table_s_pens_give_color_screening_and_lineweight() {
        let body = "description=\"Test\naci_table_available=TRUE\nplot_style{\n 0{\n  name=\"Color_1\n  color=-1023410176\n  color_policy=5\n  screen=100\n  lineweight=5\n }\n 1{\n  name=\"Color_2\n  color=-1\n  color_policy=3\n  screen=50\n  lineweight=0\n }\n}\ncustom_lineweight_table{\n 0=0.0\n 5=0.35\n}\n";
        let table = parse(&ctb(body)).unwrap();
        assert_eq!(table.description, "Test");
        assert_eq!(table.pens.len(), 2);
        let red = table.pen(1);
        assert_eq!((red.color, red.lineweight), (Some((0, 0, 0)), Some(0.35)), "a custom lineweight table entry");
        assert_eq!(red.apply((255, 0, 0)), (0, 0, 0));
        let yellow = table.pen(2);
        assert_eq!(yellow.lineweight, None, "lineweight 0 is the object's own");
        assert_eq!(yellow.apply((0, 0, 0)), (128, 128, 128), "black at half strength");
        assert_eq!(table.pen(200), PenStyle::default());
    }

    #[test]
    fn a_named_table_s_pens_go_by_style_name() {
        let body = "description=\"Named\naci_table_available=FALSE\nplot_style{\n 0{\n  name=\"Normal\n  color=-1\n  screen=100\n  lineweight=0\n }\n 1{\n  name=\"Hidden\n  color=-1006632961\n  color_policy=3\n  screen=40\n  lineweight=0\n }\n}\n";
        let table = parse(&ctb(body)).unwrap();
        assert!(table.is_named());
        assert_eq!(table.names, ["Normal", "Hidden"]);
        assert_eq!(table.named_pen("hidden").apply((0, 0, 0)), (153, 153, 153), "found whatever the case: gray at 40%");
        assert_eq!(table.named_pen("Normal"), PenStyle::default());
        assert_eq!(table.named_pen("Nowhere"), PenStyle::default(), "a style the table lacks prints as Normal");
        assert_eq!(built_in("monochrome.stb").unwrap().named_pen("Normal").apply((255, 0, 0)), (0, 0, 0));
    }

    #[test]
    fn standard_tables_work_without_their_files() {
        let mono = built_in("Monochrome.ctb").unwrap();
        assert_eq!(mono.pen(1).apply((255, 0, 0)), (0, 0, 0));
        let gray = built_in("grayscale.ctb").unwrap();
        assert_eq!(gray.pen(3).apply((0, 255, 0)), (150, 150, 150));
        assert!(built_in("nope.ctb").is_none());
        assert_eq!(product_year("C:/Users/a/AppData/Roaming/Autodesk/AutoCAD 2021/R24.0/enu/Plotters/Plot Styles"), 2021);
    }
}
