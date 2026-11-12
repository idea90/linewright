//! What Windows and Linux each do their own way: where programs go, menus and shortcuts, which program opens a
//! kind of file, and the list of installed programs.

use crate::install::System;
#[cfg(not(windows))]
use crate::install::{Choices, Installed, RECORD};
#[cfg(not(windows))]
use std::{
    fs,
    path::{Path, PathBuf},
};

/// The system this setup runs on.
pub fn current() -> Box<dyn System> {
    #[cfg(windows)]
    return Box::new(windows::Windows);
    #[cfg(not(windows))]
    Box::new(Linux::from_env())
}

#[cfg(not(windows))]
pub use linux::Linux;

#[cfg(not(windows))]
mod linux {
    use super::*;

    /// Linux, and other systems that follow the freedesktop.org conventions: a program goes in the person's own data
    /// folder, a command for it in ~/.local/bin, and its menu entry and icon where menus look for them.
    pub struct Linux {
        /// Where menus look for entries and icons: $XDG_DATA_HOME, or ~/.local/share.
        pub data: PathBuf,
        /// Where the `linewright` command goes: ~/.local/bin.
        pub bin: PathBuf,
        /// Whether menus are told of the change (not in tests, which have no menus).
        pub refresh: bool,
    }

    /// The kinds of file Linewright opens, as Linux names them.
    const MIME_TYPES: &str = "image/vnd.dxf;image/vnd.dwg;image/x-dwg;application/dxf;application/acad;";

    const ICON: &[u8] = include_bytes!("../../assets/linewright.png");

    impl Linux {
        pub fn from_env() -> Linux {
            let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
            let data = std::env::var_os("XDG_DATA_HOME")
                .map(PathBuf::from)
                .filter(|p| p.is_absolute())
                .unwrap_or_else(|| home.join(".local/share"));
            Linux { data, bin: home.join(".local/bin"), refresh: true }
        }

        fn desktop_file(&self) -> PathBuf {
            self.data.join("applications/linewright.desktop")
        }

        fn command(&self) -> PathBuf {
            self.bin.join("linewright")
        }

        /// Menus and file managers read caches of entries and icons; they are brought up to date where the tools are.
        fn refresh_menus(&self) {
            if !self.refresh {
                return;
            }
            let run = |program: &str, args: &[&std::ffi::OsStr]| {
                let _ = std::process::Command::new(program)
                    .args(args)
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::null())
                    .status();
            };
            run("update-desktop-database", &[self.data.join("applications").as_os_str()]);
            run("gtk-update-icon-cache", &["-q".as_ref(), "-t".as_ref(), self.data.join("icons/hicolor").as_os_str()]);
        }
    }

    impl System for Linux {
        fn default_folder(&self) -> PathBuf {
            self.data.join("linewright")
        }

        fn installed_folder(&self) -> Option<PathBuf> {
            let program = fs::read_link(self.command()).ok()?;
            let folder = program.parent()?;
            folder.join(RECORD).is_file().then(|| folder.to_path_buf())
        }

        fn integrate(&self, choices: &Choices, program: &Path) -> Result<Vec<PathBuf>, String> {
            let failed = |path: &Path, error: std::io::Error| format!("{} could not be written: {error}", path.display());
            let mut files = vec![];
            // The command, so `linewright` can be typed in a terminal.
            let command = self.command();
            fs::create_dir_all(&self.bin).map_err(|e| failed(&self.bin, e))?;
            if command.symlink_metadata().is_ok() {
                fs::remove_file(&command).map_err(|e| failed(&command, e))?;
            }
            std::os::unix::fs::symlink(program, &command).map_err(|e| failed(&command, e))?;
            files.push(command);
            // The menu entry, which also says what it opens; kept out of the menu when only the files are wanted.
            if choices.menu || choices.associate {
                let icon = self.data.join("icons/hicolor/256x256/apps/linewright.png");
                let desktop = self.desktop_file();
                for file in [&icon, &desktop] {
                    let folder = file.parent().expect("a file in a folder");
                    fs::create_dir_all(folder).map_err(|e| failed(folder, e))?;
                }
                fs::write(&icon, ICON).map_err(|e| failed(&icon, e))?;
                let mut entry = format!(
                    "[Desktop Entry]\nType=Application\nName=Linewright\nGenericName=2D CAD\nComment=Draw, edit, dimension and plot DXF and DWG \
                 drawings\nExec=\"{}\" %f\nIcon=linewright\nTerminal=false\nCategories=Graphics;Engineering;2DGraphics;\n\
                 StartupWMClass=linewright\n",
                    program.display()
                );
                if choices.associate {
                    entry.push_str(&format!("MimeType={MIME_TYPES}\n"));
                }
                if !choices.menu {
                    entry.push_str("NoDisplay=true\n");
                }
                fs::write(&desktop, entry).map_err(|e| failed(&desktop, e))?;
                files.extend([icon, desktop]);
            }
            self.refresh_menus();
            if choices.associate && self.refresh {
                let mut command = std::process::Command::new("xdg-mime");
                command.args(["default", "linewright.desktop"]).args(MIME_TYPES.split(';').filter(|t| !t.is_empty()));
                let _ = command.stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status();
            }
            Ok(files)
        }

        fn register(&self, _installed: &Installed, _uninstaller: &Path) -> Result<(), String> {
            Ok(())
        }

        fn unintegrate(&self) {
            // The entry and icon are files, removed with the rest; the menus are told once they are gone.
            self.refresh_menus();
        }

        fn remove_after_exit(&self, _uninstaller: &Path, _folder: &Path) -> bool {
            false
        }
    }
}

#[cfg(windows)]
mod windows {
    use super::*;
    use crate::install::{Choices, Installed, PROGRAM, RECORD};
    use std::fs;
    use std::os::windows::process::CommandExt;
    use std::path::{Path, PathBuf};
    use winreg::RegKey;
    use winreg::enums::HKEY_CURRENT_USER;

    /// Runs a console program without a console window showing.
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    const DETACHED_PROCESS: u32 = 0x0000_0008;

    /// The kinds of file Linewright opens, by suffix.
    const SUFFIXES: [&str; 2] = [".dxf", ".dwg"];

    const UNINSTALL_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\Linewright";
    const CLASS: &str = "Linewright.Drawing";

    #[link(name = "shell32")]
    unsafe extern "system" {
        fn SHChangeNotify(event: i32, flags: u32, first: *const std::ffi::c_void, second: *const std::ffi::c_void);
    }

    /// Tell Explorer which programs open which files has changed, so icons and Open with show it at once.
    fn associations_changed() {
        const SHCNE_ASSOCCHANGED: i32 = 0x0800_0000;
        // SAFETY: the documented call for this event takes no items, and null pointers say so.
        unsafe { SHChangeNotify(SHCNE_ASSOCCHANGED, 0, std::ptr::null(), std::ptr::null()) };
    }

    /// A path as PowerShell reads it between single quotes.
    fn quoted(path: &Path) -> String {
        format!("'{}'", path.display().to_string().replace('\'', "''"))
    }

    fn user() -> RegKey {
        RegKey::predef(HKEY_CURRENT_USER)
    }

    fn set(path: &str, values: &[(&str, &str)]) -> Result<(), String> {
        let failed = |e: std::io::Error| format!("Windows would not record {path}: {e}");
        let (key, _) = user().create_subkey(path).map_err(failed)?;
        for (name, value) in values {
            key.set_value(name, &value.to_string()).map_err(failed)?;
        }
        Ok(())
    }

    pub struct Windows;

    impl System for Windows {
        fn default_folder(&self) -> PathBuf {
            std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_default().join("Programs").join("Linewright")
        }

        fn installed_folder(&self) -> Option<PathBuf> {
            let key = user().open_subkey(UNINSTALL_KEY).ok()?;
            let folder = PathBuf::from(key.get_value::<String, _>("InstallLocation").ok()?);
            folder.join(RECORD).is_file().then_some(folder)
        }

        fn integrate(&self, choices: &Choices, program: &Path) -> Result<Vec<PathBuf>, String> {
            let mut places = vec![];
            if choices.menu {
                places.push("Programs");
            }
            if choices.desktop {
                places.push("Desktop");
            }
            let mut files = vec![];
            if !places.is_empty() {
                // Windows makes shortcuts through its shell, which PowerShell reaches most simply; it says where it
                // put each, since the desktop can be anywhere (in OneDrive, say).
                let mut script = String::from("$shell = New-Object -ComObject WScript.Shell\n");
                for place in places {
                    script.push_str(&format!(
                        "$at = Join-Path ([Environment]::GetFolderPath('{place}')) 'Linewright.lnk'\n$link = $shell.CreateShortcut($at)\n\
                         $link.TargetPath = {}\n$link.WorkingDirectory = {}\n$link.Save()\nWrite-Output $at\n",
                        quoted(program),
                        quoted(program.parent().unwrap_or(program))
                    ));
                }
                let output = std::process::Command::new("powershell.exe")
                    .args(["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-Command", &script])
                    .creation_flags(CREATE_NO_WINDOW)
                    .output()
                    .map_err(|e| format!("The shortcuts could not be made: {e}"))?;
                if !output.status.success() {
                    return Err(format!("The shortcuts could not be made: {}", String::from_utf8_lossy(&output.stderr).trim()));
                }
                files.extend(String::from_utf8_lossy(&output.stdout).lines().map(str::trim).filter(|l| !l.is_empty()).map(PathBuf::from));
            }
            let open = format!("\"{}\" \"%1\"", program.display());
            // Linewright is in every DXF and DWG file's Open with list, and their program when chosen.
            let application = r"Software\Classes\Applications\linewright.exe";
            set(&format!(r"{application}\shell\open\command"), &[("", &open)])?;
            set(&format!(r"{application}\SupportedTypes"), &SUFFIXES.map(|suffix| (suffix, "")))?;
            if choices.associate {
                set(&format!(r"Software\Classes\{CLASS}"), &[("", "Drawing")])?;
                set(&format!(r"Software\Classes\{CLASS}\DefaultIcon"), &[("", &format!("\"{}\",0", program.display()))])?;
                set(&format!(r"Software\Classes\{CLASS}\shell\open\command"), &[("", &open)])?;
                for suffix in SUFFIXES {
                    set(&format!(r"Software\Classes\{suffix}\OpenWithProgids"), &[(CLASS, "")])?;
                }
            }
            associations_changed();
            Ok(files)
        }

        fn register(&self, installed: &Installed, uninstaller: &Path) -> Result<(), String> {
            let program = installed.folder.join(PROGRAM);
            set(
                UNINSTALL_KEY,
                &[
                    ("DisplayName", "Linewright"),
                    ("DisplayVersion", &installed.version),
                    ("Publisher", "Linewright"),
                    ("DisplayIcon", &program.display().to_string()),
                    ("InstallLocation", &installed.folder.display().to_string()),
                    ("UninstallString", &format!("\"{}\" --uninstall", uninstaller.display())),
                    ("URLInfoAbout", "https://github.com/idea90/linewright"),
                ],
            )?;
            let size: u64 = installed.files.iter().filter_map(|file| fs::metadata(file).ok()).map(|m| m.len()).sum();
            let (key, _) = user().create_subkey(UNINSTALL_KEY).map_err(|e| e.to_string())?;
            for (name, value) in [("EstimatedSize", (size / 1024) as u32), ("NoModify", 1), ("NoRepair", 1)] {
                key.set_value(name, &value).map_err(|e| e.to_string())?;
            }
            Ok(())
        }

        fn unintegrate(&self) {
            let classes = user().open_subkey_with_flags(r"Software\Classes", winreg::enums::KEY_ALL_ACCESS);
            if let Ok(classes) = classes {
                let _ = classes.delete_subkey_all(CLASS);
                let _ = classes.delete_subkey_all(r"Applications\linewright.exe");
                for suffix in SUFFIXES {
                    if let Ok(list) = classes.open_subkey_with_flags(format!(r"{suffix}\OpenWithProgids"), winreg::enums::KEY_ALL_ACCESS) {
                        let _ = list.delete_value(CLASS);
                    }
                }
            }
            let _ = user().delete_subkey_all(UNINSTALL_KEY);
            associations_changed();
        }

        fn remove_after_exit(&self, uninstaller: &Path, folder: &Path) -> bool {
            // A running program cannot remove itself on Windows: a command waits a moment for it to close, then does.
            let script = format!("ping -n 3 127.0.0.1 >nul & del /f /q \"{}\" & rmdir \"{}\"", uninstaller.display(), folder.display());
            std::process::Command::new("cmd.exe")
                .arg("/C")
                .raw_arg(&script)
                .creation_flags(CREATE_NO_WINDOW | DETACHED_PROCESS)
                .spawn()
                .is_ok()
        }
    }
}
