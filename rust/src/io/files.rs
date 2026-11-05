use std::sync::Arc;

use crate::model::entities::{BYLAYER, BlockDef, BlockRef, Entity};
use crate::model::styles::BYLAYER_LINETYPE;
use std::collections::hash_map::RandomState;
use std::fs;
use std::hash::{BuildHasher, Hasher};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::mpsc::{self, Sender};
use std::thread::JoinHandle;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

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

// ------------------------------------------------------------------------------------------- autosave

/// The small text file beside an autosave copy: the drawing's name, the file it was saved to, and when it was written.
const AUTOSAVE_NOTE: &str = "txt";

/// An autosave copy a session left behind: its DXF, the drawing's name and file, and when the copy was written.
#[derive(Clone, Debug, PartialEq)]
pub struct Autosave {
    pub dxf: PathBuf,
    /// The drawing's file name, or the name it had before it was saved, as in "plan.dxf" or "Drawing1".
    pub title: String,
    /// The file the drawing was saved to, if it ever was.
    pub original: Option<PathBuf>,
    /// When the copy was written, in seconds since 1970.
    pub written: u64,
    /// The process of the Linewright that wrote it, if the note says.
    pub process: Option<u32>,
}

impl Autosave {
    /// How long ago the copy was written, as the recovery dialog says it: "just now", "5 minutes ago".
    pub fn age_text(&self, now: u64) -> String {
        let seconds = now.saturating_sub(self.written);
        let (count, unit) = match seconds {
            0..60 => return "just now".to_string(),
            60..3600 => (seconds / 60, "minute"),
            3600..86400 => (seconds / 3600, "hour"),
            _ => (seconds / 86400, "day"),
        };
        format!("{count} {unit}{} ago", if count == 1 { "" } else { "s" })
    }
}

/// Whether a drawing is due an autosave: it has unsaved changes, autosave is on, and `minutes` have passed since it
/// was last saved or autosaved.
pub fn autosave_due(modified: bool, minutes: u32, since_saved: Duration) -> bool {
    modified && minutes > 0 && since_saved >= Duration::from_secs(u64::from(minutes) * 60)
}

/// The file name of an autosave copy: the drawing's name and a unique id, as in plan_3f2a.dxf.
pub fn autosave_name(stem: &str, id: &str) -> String {
    format!("{stem}_{id}.{DXF_SUFFIX}")
}

/// A new autosave file for a drawing called `stem` in `folder`, with an id that no file there has.
pub fn new_autosave_path(folder: &Path, stem: &str) -> PathBuf {
    loop {
        let id = format!("{:06x}", RandomState::new().build_hasher().finish() & 0xff_ffff);
        let path = folder.join(autosave_name(stem, &id));
        if !path.exists() {
            return path;
        }
    }
}

/// The note written beside an autosave copy: the drawing's name, the file it was saved to (empty if none), when the
/// copy was written, and the process writing it, one item per line.
pub fn autosave_note(title: &str, original: Option<&Path>, written: u64) -> String {
    let original = original.map_or(String::new(), |p| p.display().to_string());
    format!("{}\n{original}\n{written}\n{}\n", title.replace(['\n', '\r'], " "), std::process::id())
}

/// Whether a process is still running, so its autosave copies belong to a Linewright that is still open rather than
/// one that crashed.
fn process_running(process: u32) -> bool {
    #[cfg(target_os = "linux")]
    {
        Path::new("/proc").join(process.to_string()).exists()
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // Without a console window flashing up.
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        std::process::Command::new("tasklist")
            .args(["/FI", &format!("PID eq {process}"), "/NH", "/FO", "CSV"])
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .is_ok_and(|out| String::from_utf8_lossy(&out.stdout).contains(&format!("\"{process}\"")))
    }
    #[cfg(not(any(target_os = "linux", windows)))]
    {
        let _ = process;
        false
    }
}

/// The current time in seconds since 1970.
pub fn unix_now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// Write an autosave copy and its note. The copy is written under another name first, so a crash in the middle never
/// leaves a damaged copy under its real name.
fn write_autosave(dxf: &Path, bytes: &[u8], note: &str) -> std::io::Result<()> {
    if let Some(folder) = dxf.parent() {
        fs::create_dir_all(folder)?;
    }
    let staged = dxf.with_extension("tmp");
    fs::write(&staged, bytes)?;
    fs::rename(&staged, dxf)?;
    fs::write(dxf.with_extension(AUTOSAVE_NOTE), note)
}

/// Remove an autosave copy and its note.
pub fn remove_autosave(dxf: &Path) {
    let _ = fs::remove_file(dxf);
    let _ = fs::remove_file(dxf.with_extension(AUTOSAVE_NOTE));
}

/// The autosave copies in `folder` left by a Linewright that is no longer running, newest first. A copy whose note
/// is missing or unreadable is still listed; one written by another open Linewright is not.
pub fn leftover_autosaves(folder: &Path) -> Vec<Autosave> {
    let Ok(entries) = fs::read_dir(folder) else { return vec![] };
    let mut copies: Vec<Autosave> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| suffix_of(path) == DXF_SUFFIX && path.is_file())
        .map(read_autosave)
        .filter(|copy| copy.process.is_none_or(|p| p == std::process::id() || !process_running(p)))
        .collect();
    copies.sort_by_key(|copy| std::cmp::Reverse(copy.written));
    copies
}

fn read_autosave(dxf: PathBuf) -> Autosave {
    let note = fs::read_to_string(dxf.with_extension(AUTOSAVE_NOTE)).unwrap_or_default();
    let mut lines = note.lines();
    let title = lines.next().filter(|t| !t.is_empty()).map_or_else(|| name_of(&dxf), str::to_string);
    let original = lines.next().filter(|l| !l.is_empty()).map(PathBuf::from);
    let written = lines.next().and_then(|l| l.trim().parse().ok()).or_else(|| modified_secs(&dxf)).unwrap_or(0);
    let process = lines.next().and_then(|l| l.trim().parse().ok());
    Autosave { dxf, title, original, written, process }
}

fn modified_secs(path: &Path) -> Option<u64> {
    path.metadata().ok()?.modified().ok()?.duration_since(UNIX_EPOCH).ok().map(|d| d.as_secs())
}

enum Job {
    Write { dxf: PathBuf, bytes: Vec<u8>, note: String },
    Remove(PathBuf),
}

/// Writes and removes autosave copies on a background thread, in the order they are asked for, so the window never
/// waits on the disk. Dropping it waits until every copy is written or removed.
pub struct Autosaver {
    sender: Option<Sender<Job>>,
    worker: Option<JoinHandle<()>>,
}

impl Autosaver {
    /// Start the background thread that does the writing.
    pub fn start() -> Autosaver {
        let (sender, receiver) = mpsc::channel::<Job>();
        let worker = std::thread::spawn(move || {
            for job in receiver {
                match job {
                    Job::Write { dxf, bytes, note } => {
                        let _ = write_autosave(&dxf, &bytes, &note);
                    }
                    Job::Remove(dxf) => remove_autosave(&dxf),
                }
            }
        });
        Autosaver { sender: Some(sender), worker: Some(worker) }
    }

    /// Write the DXF bytes of a drawing as an autosave copy, with its note.
    pub fn write(&self, dxf: PathBuf, bytes: Vec<u8>, note: String) {
        self.send(Job::Write { dxf, bytes, note });
    }

    /// Remove an autosave copy once its drawing is saved or discarded.
    pub fn remove(&self, dxf: PathBuf) {
        self.send(Job::Remove(dxf));
    }

    fn send(&self, job: Job) {
        if let Some(sender) = &self.sender {
            let _ = sender.send(job);
        }
    }
}

impl Drop for Autosaver {
    fn drop(&mut self) {
        self.sender.take();
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[cfg(test)]
mod autosave_tests {
    use super::*;

    #[test]
    fn an_autosave_is_due_only_with_changes_and_after_savetime() {
        assert!(!autosave_due(false, 10, Duration::from_secs(9999)), "nothing unsaved");
        assert!(!autosave_due(true, 0, Duration::from_secs(9999)), "autosave is off");
        assert!(!autosave_due(true, 10, Duration::from_secs(599)), "not yet");
        assert!(autosave_due(true, 10, Duration::from_secs(600)));
    }

    #[test]
    fn autosave_names_carry_the_drawing_and_a_new_id() {
        assert_eq!(autosave_name("plan", "3f2a"), "plan_3f2a.dxf");
        let folder = tempfile::tempdir().unwrap();
        let first = new_autosave_path(folder.path(), "plan");
        assert_eq!(first.parent(), Some(folder.path()));
        assert!(first.file_name().unwrap().to_string_lossy().starts_with("plan_"));
        fs::write(&first, b"").unwrap();
        assert_ne!(new_autosave_path(folder.path(), "plan"), first);
    }

    #[test]
    fn leftover_copies_are_read_back_newest_first_with_their_notes() {
        let folder = tempfile::tempdir().unwrap();
        let original = Path::new("/work/plan.dxf");
        let older = folder.path().join("plan_aaaaaa.dxf");
        write_autosave(&older, b"0", &autosave_note("plan.dxf", Some(original), 1000)).unwrap();
        let untitled = folder.path().join("Drawing1_bbbbbb.dxf");
        write_autosave(&untitled, b"0", &autosave_note("Drawing1", None, 2000)).unwrap();
        fs::write(folder.path().join("stray.txt"), "x").unwrap();
        fs::write(folder.path().join("half_cccccc.tmp"), "x").unwrap();
        let found = leftover_autosaves(folder.path());
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].dxf, untitled);
        assert_eq!(found[0].original, None);
        let expected = Autosave {
            dxf: older.clone(),
            title: "plan.dxf".into(),
            original: Some(original.to_path_buf()),
            written: 1000,
            process: Some(std::process::id()),
        };
        assert_eq!(found[1], expected);
        remove_autosave(&older);
        assert_eq!(leftover_autosaves(folder.path()).len(), 1);
        assert!(leftover_autosaves(&folder.path().join("missing")).is_empty());
    }

    #[test]
    fn a_copy_without_a_note_is_still_listed() {
        let folder = tempfile::tempdir().unwrap();
        fs::write(folder.path().join("plan_dddddd.dxf"), b"0").unwrap();
        let found = leftover_autosaves(folder.path());
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].title, "plan_dddddd.dxf");
        assert!(found[0].original.is_none());
    }

    #[test]
    fn the_background_writer_finishes_its_queue_before_it_stops() {
        let folder = tempfile::tempdir().unwrap();
        let dxf = folder.path().join("plan_eeeeee.dxf");
        let writer = Autosaver::start();
        writer.write(dxf.clone(), b"DXF".to_vec(), autosave_note("plan.dxf", None, 5));
        writer.remove(dxf.clone());
        drop(writer);
        assert!(!dxf.exists());
        assert!(!dxf.with_extension("txt").exists());
    }

    #[test]
    fn ages_read_as_the_dialog_says_them() {
        let copy = Autosave { dxf: PathBuf::new(), title: String::new(), original: None, written: 1000, process: None };
        assert_eq!(copy.age_text(1030), "just now");
        assert_eq!(copy.age_text(1060), "1 minute ago");
        assert_eq!(copy.age_text(1000 + 3 * 3600), "3 hours ago");
        assert_eq!(copy.age_text(1000 + 86400), "1 day ago");
    }
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

    #[cfg(target_os = "linux")]
    #[test]
    fn copies_another_running_linewright_is_writing_are_not_offered() {
        let folder = tempfile::tempdir().unwrap();
        let live = folder.path().join("live_000001.dxf");
        let crashed = folder.path().join("crashed_000002.dxf");
        write_autosave(&live, b"0", "live.dxf\n\n1000\n1\n").unwrap();
        write_autosave(&crashed, b"0", &format!("crashed.dxf\n\n2000\n{}\n", u32::MAX - 1)).unwrap();
        // Process 1 is always running on Linux; the other number never is.
        let left: Vec<PathBuf> = leftover_autosaves(folder.path()).into_iter().map(|c| c.dxf).collect();
        assert_eq!(left, vec![crashed]);
    }
}
