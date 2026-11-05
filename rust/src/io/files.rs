use std::sync::Arc;

use crate::model::entities::{BYLAYER, BlockDef, BlockRef, Entity};
use crate::model::styles::BYLAYER_LINETYPE;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::dxf::{DxfFile, document_from_dxf, dxf_from_document};
use crate::model::audit::{Finding, repair_all};
use crate::model::document::Document;

pub const DXF_SUFFIX: &str = "dxf";
pub const DWG_SUFFIX: &str = "dwg";
pub const ODA_MISSING: &str = "DWG files need the free ODA File Converter. Install it from opendesign.com, then try again.";

/// A drawing could not be opened or saved. The message is meant for the user.
#[derive(Debug, Clone, PartialEq)]
pub struct FileError(pub String);

impl std::fmt::Display for FileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

fn name_of(path: &Path) -> String {
    path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().into_owned())
}

fn suffix_of(path: &Path) -> String {
    path.extension().map_or(String::new(), |e| e.to_string_lossy().to_lowercase())
}

fn unsupported(path: &Path) -> FileError {
    let suffix = path.extension().map_or(String::new(), |e| format!(".{}", e.to_string_lossy()));
    FileError(format!("Unsupported file type: {suffix}"))
}

pub fn open_drawing(path: &Path) -> Result<Document, FileError> {
    open_nested(path, 0)
}

/// A damaged drawing read as far as it goes: what was skipped while reading it, and what the audit found in it. Every
/// finding with a fix has been put right in the document.
pub struct Recovery {
    pub document: Document,
    pub skipped: Vec<String>,
    pub findings: Vec<Finding>,
}

/// RECOVER: reads a DXF or DWG file that OPEN would refuse, skipping what cannot be read, and repairs what the audit
/// finds in it.
pub fn recover_drawing(path: &Path) -> Result<Recovery, FileError> {
    let could_not = |error: &dyn std::fmt::Display| FileError(format!("Could not recover {}: {error}", name_of(path)));
    let bytes = match suffix_of(path).as_str() {
        DXF_SUFFIX => fs::read(path).map_err(|e| could_not(&e))?,
        DWG_SUFFIX => dwg_to_dxf(path)?,
        _ => return Err(unsupported(path)),
    };
    let (file, skipped) = DxfFile::parse_recovering(&bytes).map_err(|e| could_not(&e))?;
    let mut document = document_from_dxf(file);
    document.path = Some(path.to_path_buf());
    resolve_xrefs(&mut document, 0);
    let findings = repair_all(&mut document);
    Ok(Recovery { document, skipped, findings })
}

/// How deep external references within external references are followed.
const MAX_XREF_DEPTH: usize = 4;

/// Where an external reference's drawing is: as written when that exists, else beside the drawing using it.
pub fn xref_path(host: Option<&Path>, written: &str) -> std::path::PathBuf {
    let path = Path::new(written);
    match host.and_then(Path::parent) {
        Some(folder) if !path.exists() => folder.join(path.file_name().map_or(path.as_os_str(), |n| n)),
        _ => path.to_path_buf(),
    }
}

/// An external reference's drawing as block content: its model space, each object's by-layer look fixed to its
/// layer's there, since the layers do not come along.
pub fn xref_entities(path: &Path) -> Result<Vec<Entity>, FileError> {
    xref_entities_nested(path, 0)
}

fn xref_entities_nested(path: &Path, depth: usize) -> Result<Vec<Entity>, FileError> {
    let referenced = open_nested(path, depth + 1)?;
    Ok(referenced
        .space_entities(None)
        .iter()
        .map(|e| {
            let layer = referenced.layer_of(e);
            let mut own = e.clone();
            if own.color == BYLAYER {
                own.color = layer.color;
            }
            if own.linetype == BYLAYER_LINETYPE {
                own.linetype = layer.linetype.clone();
            }
            own
        })
        .collect())
}

/// Fill each external reference with what its drawing holds. One that cannot be read stays empty.
fn resolve_xrefs(document: &mut Document, depth: usize) {
    if depth >= MAX_XREF_DEPTH {
        return;
    }
    for block in document.xrefs() {
        let written = block.0.xref.clone().unwrap_or_default();
        let path = xref_path(document.path.as_deref(), &written);
        match xref_entities_nested(&path, depth) {
            Ok(entities) => document.replace_block(BlockRef(Arc::new(BlockDef { entities, ..(*block.0).clone() }))),
            Err(_) => *document.skipped.entry(format!("missing external reference {written}")).or_default() += 1,
        }
    }
}

fn open_nested(path: &Path, depth: usize) -> Result<Document, FileError> {
    let could_not = |error: &dyn std::fmt::Display| FileError(format!("Could not open {}: {error}", name_of(path)));
    let bytes = match suffix_of(path).as_str() {
        DXF_SUFFIX => fs::read(path).map_err(|e| could_not(&e))?,
        DWG_SUFFIX => dwg_to_dxf(path)?,
        _ => return Err(unsupported(path)),
    };
    let file = DxfFile::parse(&bytes).map_err(|e| could_not(&e))?;
    let mut document = document_from_dxf(file);
    document.path = Some(path.to_path_buf());
    resolve_xrefs(&mut document, depth);
    Ok(document)
}

pub fn save_drawing(document: &mut Document, path: &Path) -> Result<(), FileError> {
    let suffix = suffix_of(path);
    if suffix != DXF_SUFFIX && suffix != DWG_SUFFIX {
        return Err(unsupported(path));
    }
    let converter = if suffix == DWG_SUFFIX { Some(oda_converter().ok_or_else(|| FileError(ODA_MISSING.into()))?) } else { None };
    let file = dxf_from_document(document);
    let could_not = |error: &dyn std::fmt::Display| FileError(format!("Could not save {}: {error}", name_of(path)));
    // Stage in a temp folder so a failed export never damages an existing file.
    let folder = tempfile::Builder::new().prefix("linewright_").tempdir().map_err(|e| could_not(&e))?;
    let staged = match converter {
        None => {
            let staged = folder.path().join(name_of(path));
            fs::write(&staged, file.to_bytes()).map_err(|e| could_not(&e))?;
            staged
        }
        Some(converter) => {
            let input = folder.path().join("in");
            let output = folder.path().join("out");
            fs::create_dir_all(&input).and_then(|_| fs::create_dir_all(&output)).map_err(|e| could_not(&e))?;
            let stem = path.file_stem().map_or("drawing".into(), |s| s.to_string_lossy().into_owned());
            fs::write(input.join(format!("{stem}.dxf")), file.to_bytes()).map_err(|e| could_not(&e))?;
            convert(&converter, &input, &output, "DWG")?;
            output.join(format!("{stem}.dwg"))
        }
    };
    move_into_place(&staged, path).map_err(|e| could_not(&e))?;
    // The file now holds what the document holds. Later saves write into the same source again.
    document.path = Some(path.to_path_buf());
    Ok(())
}

/// Rename when the temp folder is on the same disk, copy when it is not.
fn move_into_place(staged: &Path, target: &Path) -> std::io::Result<()> {
    if fs::rename(staged, target).is_ok() {
        return Ok(());
    }
    fs::copy(staged, target).map(|_| ())
}

fn dwg_to_dxf(path: &Path) -> Result<Vec<u8>, FileError> {
    let converter = oda_converter().ok_or_else(|| FileError(ODA_MISSING.into()))?;
    let could_not = |error: &dyn std::fmt::Display| FileError(format!("Could not open {}: {error}", name_of(path)));
    let folder = tempfile::Builder::new().prefix("linewright_").tempdir().map_err(|e| could_not(&e))?;
    let input = folder.path().join("in");
    let output = folder.path().join("out");
    fs::create_dir_all(&input).and_then(|_| fs::create_dir_all(&output)).map_err(|e| could_not(&e))?;
    fs::copy(path, input.join("drawing.dwg")).map_err(|e| could_not(&e))?;
    convert(&converter, &input, &output, "DXF")?;
    fs::read(output.join("drawing.dxf")).map_err(|_| could_not(&"the ODA File Converter could not read it"))
}

/// Run the ODA File Converter on every file in a folder.
fn convert(converter: &Path, input: &Path, output: &Path, kind: &str) -> Result<(), FileError> {
    let mut command = Command::new(converter);
    command.arg(input).arg(output).arg("ACAD2018").arg(kind).arg("0").arg("1");
    hide_window(&mut command);
    let status = command.status().map_err(|e| FileError(format!("Could not run the ODA File Converter: {e}")))?;
    if !status.success() {
        return Err(FileError(format!("The ODA File Converter failed ({status}).")));
    }
    Ok(())
}

/// Keep the converter from showing a window of its own. It is a Qt program: on Windows it only ships Qt's
/// "windows" platform plugin, so nothing may be overridden there, just the console window suppressed.
#[cfg(windows)]
fn hide_window(command: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    command.creation_flags(CREATE_NO_WINDOW);
}

/// On Linux without a display (a server, a terminal session) Qt needs the offscreen platform to start at all.
/// With a display it is left alone, since not every build of the converter includes that plugin.
#[cfg(not(windows))]
fn hide_window(command: &mut Command) {
    let has_display = std::env::var_os("DISPLAY").is_some() || std::env::var_os("WAYLAND_DISPLAY").is_some();
    if !has_display {
        command.env("QT_QPA_PLATFORM", "offscreen");
    }
}

/// Where the ODA File Converter is installed, if it is. LINEWRIGHT_ODA can point at it directly.
pub fn oda_converter() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("LINEWRIGHT_ODA").map(PathBuf::from).filter(|p| p.is_file()) {
        return Some(path);
    }
    let program = if cfg!(windows) { "ODAFileConverter.exe" } else { "ODAFileConverter" };
    if let Some(found) =
        std::env::var_os("PATH").and_then(|paths| std::env::split_paths(&paths).map(|dir| dir.join(program)).find(|p| p.is_file()))
    {
        return Some(found);
    }
    // The installers put it in a versioned folder: ODA/ODAFileConverter 25.4.0 on Windows, /usr/bin on Linux.
    let mut roots: Vec<PathBuf> = vec![];
    if cfg!(windows) {
        roots.extend(std::env::var_os("ProgramFiles").map(|p| PathBuf::from(p).join("ODA")));
        roots.push(PathBuf::from(r"C:\Program Files\ODA"));
    } else {
        roots.push(PathBuf::from("/usr/bin"));
        roots.push(PathBuf::from("/opt/ODAFileConverter"));
    }
    let mut found: Vec<PathBuf> = roots
        .iter()
        .filter_map(|root| fs::read_dir(root).ok())
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|dir| dir.file_name().is_some_and(|n| n.to_string_lossy().starts_with("ODAFileConverter")))
        .map(|dir| if dir.is_dir() { dir.join(program) } else { dir })
        .filter(|p| p.is_file())
        .collect();
    found.sort();
    found.pop()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::entities::{circle, line};
    use crate::model::geometry::pt;

    #[test]
    fn a_damaged_file_is_recovered_with_what_is_left() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("plan.dxf");
        let mut document = Document::with_entities(vec![line(pt(0.0, 0.0), pt(3.0, 4.0)), circle(pt(5.0, 5.0), 2.0)]);
        save_drawing(&mut document, &path).unwrap();
        // The circle's radius is garbled, which takes the circle out; the line is kept.
        let text = fs::read_to_string(&path).unwrap();
        let at = text.find("CIRCLE").unwrap();
        let garbled = format!("{}{}", &text[..at], text[at..].replacen(" 40\n2.0\n", " 40\nbig\n", 1));
        fs::write(&path, garbled).unwrap();
        assert!(open_drawing(&path).is_ok(), "the normal reader takes a bad number as it comes");
        let recovered = recover_drawing(&path).unwrap();
        assert_eq!(recovered.document.len(), 1);
        assert!(recovered.skipped.iter().any(|note| note.contains("needs a number")), "{:?}", recovered.skipped);
        assert!(recovered.findings.is_empty());
        assert_eq!(recovered.document.path.as_deref(), Some(path.as_path()));
    }

    #[test]
    fn a_truncated_file_opens_with_its_complete_objects() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("plan.dxf");
        let mut document = Document::with_entities(vec![line(pt(0.0, 0.0), pt(3.0, 4.0)), line(pt(1.0, 1.0), pt(2.0, 2.0))]);
        save_drawing(&mut document, &path).unwrap();
        // Cut after an odd number of lines, so the last group code has no value to go with it.
        let text = fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        fs::write(&path, lines[..(lines.len() * 2 / 3) | 1].join("\n")).unwrap();
        assert!(open_drawing(&path).is_err());
        let recovered = recover_drawing(&path).unwrap();
        assert!(recovered.skipped.iter().any(|note| note.contains("ends in the middle")), "{:?}", recovered.skipped);
    }

    #[test]
    fn save_and_open_a_dxf() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("plan.DXF");
        let mut document = Document::with_entities(vec![line(pt(0.0, 0.0), pt(3.0, 4.0))]);
        save_drawing(&mut document, &path).unwrap();
        assert_eq!(document.path.as_deref(), Some(path.as_path()));
        let opened = open_drawing(&path).unwrap();
        assert_eq!(opened.len(), 1);
        assert_eq!(opened.path.as_deref(), Some(path.as_path()));
    }

    #[test]
    fn unknown_types_and_missing_files_are_explained() {
        let mut document = Document::new();
        assert_eq!(save_drawing(&mut document, Path::new("plan.svg")).unwrap_err().0, "Unsupported file type: .svg");
        let error = open_drawing(Path::new("/nowhere/plan.dxf")).err().unwrap().0;
        assert!(error.starts_with("Could not open plan.dxf:"), "{error}");
    }

    #[test]
    fn a_failed_save_leaves_the_old_file_alone() {
        let folder = tempfile::tempdir().unwrap();
        let path = folder.path().join("missing_folder").join("plan.dxf");
        let mut document = Document::new();
        assert!(save_drawing(&mut document, &path).is_err());
        assert!(document.path.is_none());
    }
}
