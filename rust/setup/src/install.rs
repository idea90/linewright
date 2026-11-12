//! Putting Linewright on the computer and taking it off again. What is done the same everywhere is here; what
//! each system does its own way (menus, shortcuts, file types, the list of installed programs) is a `System`.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

/// The version of Linewright this setup installs.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The program's file in the folder it is installed in.
pub const PROGRAM: &str = if cfg!(windows) { "linewright.exe" } else { "linewright" };

/// This setup's copy in the folder, which takes Linewright off again.
pub const UNINSTALLER: &str = if cfg!(windows) { "uninstall.exe" } else { "linewright-setup" };

/// The list of what was put where, kept in the folder for taking it all off again.
pub const RECORD: &str = "installed.txt";

/// The program, compressed when the setup was built; empty in a setup built without it.
const PAYLOAD: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/payload.gz"));

/// The program this setup installs, or none in a setup built without it.
pub fn payload() -> Option<Vec<u8>> {
    if PAYLOAD.is_empty() {
        return None;
    }
    let mut program = vec![];
    flate2::read::GzDecoder::new(PAYLOAD).read_to_end(&mut program).ok()?;
    Some(program)
}

/// What the person chose.
#[derive(Clone, Debug)]
pub struct Choices {
    pub folder: PathBuf,
    /// An entry in the Start menu, or in the applications menu.
    pub menu: bool,
    /// A shortcut on the desktop (Windows only).
    pub desktop: bool,
    /// DXF and DWG drawings open in Linewright.
    pub associate: bool,
}

/// Linewright as found installed on this computer.
#[derive(Clone, Debug, PartialEq)]
pub struct Installed {
    pub folder: PathBuf,
    pub version: String,
    /// Everything put on the computer, the folder's own files and those elsewhere.
    pub files: Vec<PathBuf>,
    /// What was chosen: menu entry, desktop shortcut, file types. An update keeps them.
    pub chosen: Option<(bool, bool, bool)>,
}

impl Installed {
    /// Read what an installation in `folder` left in its record.
    pub fn read(folder: &Path) -> Option<Installed> {
        let text = fs::read_to_string(folder.join(RECORD)).ok()?;
        let mut installed = Installed { folder: folder.to_path_buf(), version: String::new(), files: vec![], chosen: None };
        for line in text.lines() {
            match line.split_once(' ') {
                Some(("version", version)) => installed.version = version.to_string(),
                Some(("file", path)) => installed.files.push(PathBuf::from(path)),
                Some(("chosen", chosen)) => {
                    let has = |word: &str| chosen.split_whitespace().any(|w| w == word);
                    installed.chosen = Some((has("menu"), has("desktop"), has("associate")));
                }
                _ => {}
            }
        }
        Some(installed)
    }

    fn write(&self) -> std::io::Result<()> {
        let mut text = format!("# Linewright, as its setup installed it. Its uninstaller removes these files.\nversion {}\n", self.version);
        if let Some((menu, desktop, associate)) = self.chosen {
            let words = [(menu, "menu"), (desktop, "desktop"), (associate, "associate")];
            let chosen: Vec<&str> = words.iter().filter(|(on, _)| *on).map(|(_, word)| *word).collect();
            text.push_str(&format!("chosen {}\n", chosen.join(" ")));
        }
        for file in &self.files {
            text.push_str(&format!("file {}\n", file.display()));
        }
        fs::write(self.folder.join(RECORD), text)
    }
}

/// What each system does its own way.
pub trait System {
    /// Where Linewright goes unless the person chooses otherwise.
    fn default_folder(&self) -> PathBuf;

    /// The folder Linewright is installed in already, if it is.
    fn installed_folder(&self) -> Option<PathBuf>;

    /// Add the menu entry, shortcuts and file types chosen, for the program at `program`. Gives the files made for
    /// them, to be removed again with the program.
    fn integrate(&self, choices: &Choices, program: &Path) -> Result<Vec<PathBuf>, String>;

    /// Put Linewright in the system's list of installed programs, if it has one.
    fn register(&self, installed: &Installed, uninstaller: &Path) -> Result<(), String>;

    /// Take out what `integrate` and `register` put in beyond files: the file types and the entry in the list.
    fn unintegrate(&self);

    /// Remove the running uninstaller, and then its folder, once it has closed. Gives false where the system lets
    /// a running program be removed while it runs, so it was removed with the rest.
    fn remove_after_exit(&self, uninstaller: &Path, folder: &Path) -> bool;
}

/// The words for an error putting Linewright on the computer.
fn could_not(what: &str, path: &Path, error: &std::io::Error) -> String {
    match error.kind() {
        std::io::ErrorKind::PermissionDenied => format!(
            "Linewright cannot be {what} {}: permission was refused. Choose a folder of your own, or close Linewright if it is \
             open, then try again.",
            path.display()
        ),
        _ => format!("Linewright cannot be {what} {}: {error}", path.display()),
    }
}

/// Put a file in place over whatever was there. A program that is running cannot be written over on Windows, but it
/// can be renamed out of the way: the old copy is then removed when it next can be.
fn replace(path: &Path, contents: &[u8], executable: bool) -> Result<(), String> {
    if path.exists() {
        let old = path.with_extension("old");
        let _ = fs::remove_file(&old);
        if fs::remove_file(path).is_err() && fs::rename(path, &old).is_err() {
            return Err(format!("{} is in use. Close Linewright, then try again.", path.display()));
        }
    }
    fs::write(path, contents).map_err(|e| could_not("put in", path, &e))?;
    #[cfg(unix)]
    if executable {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).map_err(|e| could_not("put in", path, &e))?;
    }
    let _ = executable;
    Ok(())
}

/// Install Linewright as chosen. `program` is the program's bytes and `setup` this setup's own, copied in as the
/// uninstaller. `step` is told what is being done, for the person watching.
pub fn install(
    choices: &Choices,
    program: &[u8],
    setup: &[u8],
    system: &dyn System,
    step: &mut dyn FnMut(&str),
) -> Result<Installed, String> {
    let folder = &choices.folder;
    // What an earlier installation put elsewhere goes first, so nothing is left behind that was not chosen this time.
    if let Some(earlier) = Installed::read(folder) {
        step("Removing what the earlier installation added");
        system.unintegrate();
        for file in earlier.files.iter().filter(|file| !file.starts_with(folder)) {
            let _ = fs::remove_file(file);
        }
    }
    step("Copying Linewright");
    fs::create_dir_all(folder).map_err(|e| could_not("installed in", folder, &e))?;
    let program_path = folder.join(PROGRAM);
    replace(&program_path, program, true)?;
    let uninstaller = folder.join(UNINSTALLER);
    if std::env::current_exe().ok().and_then(|exe| exe.canonicalize().ok()) != uninstaller.canonicalize().ok() {
        replace(&uninstaller, setup, true)?;
    }
    step(if cfg!(windows) { "Adding shortcuts and file types" } else { "Adding the menu entry" });
    let mut files = vec![program_path.clone(), uninstaller.clone()];
    files.extend(system.integrate(choices, &program_path)?);
    let chosen = Some((choices.menu, choices.desktop, choices.associate));
    let installed = Installed { folder: folder.clone(), version: VERSION.to_string(), files, chosen };
    installed.write().map_err(|e| could_not("installed in", folder, &e))?;
    step("Registering Linewright");
    system.register(&installed, &uninstaller)?;
    Ok(installed)
}

/// Take Linewright off the computer: everything its record lists, then its folder if nothing else is left in it.
/// Drawings are never touched: only what the setup put in is removed.
pub fn uninstall(folder: &Path, system: &dyn System, step: &mut dyn FnMut(&str)) -> Result<(), String> {
    let installed = Installed::read(folder).unwrap_or_else(|| Installed {
        folder: folder.to_path_buf(),
        version: String::new(),
        files: vec![folder.join(PROGRAM), folder.join(UNINSTALLER)],
        chosen: None,
    });
    step("Removing shortcuts and file types");
    system.unintegrate();
    step("Removing Linewright");
    let running = std::env::current_exe().ok().and_then(|exe| exe.canonicalize().ok());
    let mut later = None;
    for file in &installed.files {
        if file.canonicalize().ok().is_some_and(|file| Some(&file) == running.as_ref()) && cfg!(windows) {
            later = Some(file.clone());
            continue;
        }
        match fs::remove_file(file) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
                return Err(format!("{} could not be removed: {e}. Close Linewright, then try again.", file.display()));
            }
            _ => {}
        }
    }
    let _ = fs::remove_file(folder.join(PROGRAM).with_extension("old"));
    let _ = fs::remove_file(folder.join(RECORD));
    if !later.is_some_and(|uninstaller| system.remove_after_exit(&uninstaller, folder)) {
        let _ = fs::remove_dir(folder);
    }
    Ok(())
}

// The tests install into a folder of their own, which Linux allows; on Windows they would change the registry.
#[cfg(all(test, not(windows)))]
mod tests {
    use super::*;
    use crate::system::Linux;

    fn system(root: &Path) -> Linux {
        Linux { data: root.join("share"), bin: root.join("bin"), refresh: false }
    }

    fn choices(root: &Path, menu: bool, associate: bool) -> Choices {
        Choices { folder: root.join("share/linewright"), menu, desktop: false, associate }
    }

    #[test]
    fn installing_puts_the_program_its_menu_entry_and_a_record_in_place() {
        let root = tempfile::tempdir().expect("a folder");
        let system = system(root.path());
        let installed = install(&choices(root.path(), true, true), b"program", b"setup", &system, &mut |_| {}).expect("installed");
        let folder = root.path().join("share/linewright");
        assert_eq!(fs::read(folder.join(PROGRAM)).expect("the program"), b"program");
        assert_eq!(fs::read(folder.join(UNINSTALLER)).expect("the uninstaller"), b"setup");
        let desktop = fs::read_to_string(root.path().join("share/applications/linewright.desktop")).expect("the menu entry");
        assert!(desktop.contains(&format!("Exec=\"{}\" %f", folder.join(PROGRAM).display())), "{desktop}");
        assert!(desktop.contains("MimeType=image/vnd.dxf;image/vnd.dwg;") && !desktop.contains("NoDisplay"), "{desktop}");
        assert!(root.path().join("share/icons/hicolor/256x256/apps/linewright.png").is_file());
        assert_eq!(fs::read_link(root.path().join("bin/linewright")).expect("the command"), folder.join(PROGRAM));
        assert_eq!(system.installed_folder(), Some(folder.clone()));
        assert_eq!(Installed::read(&folder), Some(installed.clone()));
        assert_eq!(installed.version, VERSION);
        assert_eq!(installed.files.len(), 5);
        assert_eq!(installed.chosen, Some((true, false, true)));
    }

    #[test]
    fn installing_again_leaves_nothing_that_was_not_chosen_the_second_time() {
        let root = tempfile::tempdir().expect("a folder");
        let system = system(root.path());
        install(&choices(root.path(), true, false), b"one", b"setup", &system, &mut |_| {}).expect("installed");
        install(&choices(root.path(), false, false), b"two", b"setup", &system, &mut |_| {}).expect("installed again");
        assert!(!root.path().join("share/applications/linewright.desktop").exists());
        assert_eq!(fs::read(root.path().join("share/linewright").join(PROGRAM)).expect("the program"), b"two");
        // Opening drawings without a menu entry still needs the entry, kept out of the menu.
        install(&choices(root.path(), false, true), b"three", b"setup", &system, &mut |_| {}).expect("installed again");
        let desktop = fs::read_to_string(root.path().join("share/applications/linewright.desktop")).expect("the entry");
        assert!(desktop.contains("NoDisplay=true") && desktop.contains("MimeType="), "{desktop}");
    }

    #[test]
    fn uninstalling_removes_everything_installed_and_nothing_else() {
        let root = tempfile::tempdir().expect("a folder");
        let system = system(root.path());
        let installed = install(&choices(root.path(), true, true), b"program", b"setup", &system, &mut |_| {}).expect("installed");
        let drawing = root.path().join("share/applications/plan.dxf");
        fs::write(&drawing, "a drawing").expect("a drawing");
        uninstall(&installed.folder, &system, &mut |_| {}).expect("removed");
        for file in &installed.files {
            assert!(!file.exists() && file.symlink_metadata().is_err(), "{} is left", file.display());
        }
        assert!(!installed.folder.exists());
        assert!(drawing.exists());
        assert_eq!(system.installed_folder(), None);
    }

    #[test]
    fn a_folder_that_cannot_be_written_is_told_plainly() {
        let root = tempfile::tempdir().expect("a folder");
        let blocked = root.path().join("file");
        fs::write(&blocked, "").expect("a file");
        let choices = Choices { folder: blocked.join("linewright"), menu: false, desktop: false, associate: false };
        let error = install(&choices, b"program", b"setup", &system(root.path()), &mut |_| {}).expect_err("a file is no folder");
        assert!(error.starts_with("Linewright cannot be installed in"), "{error}");
    }
}
