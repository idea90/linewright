//! Linewright's setup: a small window that installs Linewright, or updates or removes it. The program itself is
//! inside the setup (see `build.rs`). Installed, the setup stays beside the program as its uninstaller, which
//! `--uninstall` opens at the question of removing it.
//!
//! `--quiet` does it all with no window, as chosen by default, for scripts: `--quiet` installs (in the folder
//! `--folder=PATH` gives, if given), and `--uninstall --quiet` removes. It ends with a nonzero status on failure.

// A release build on Windows opens no console window next to the setup.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod install;
mod system;

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};

use eframe::egui::{self, Align, Layout, RichText, Vec2};
use install::{Choices, Installed, PROGRAM, VERSION};
use linewright::ui::{logo, theme};

const TITLE: &str = "Linewright Setup";

fn main() -> eframe::Result {
    let uninstalling = std::env::args().any(|arg| arg == "--uninstall");
    if std::env::args().any(|arg| arg == "--quiet") {
        let folder = std::env::args().find_map(|arg| arg.strip_prefix("--folder=").map(PathBuf::from));
        if let Err(message) = quietly(uninstalling, folder) {
            eprintln!("{message}");
            std::process::exit(1);
        }
        return Ok(());
    }
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title(TITLE)
            .with_app_id("linewright-setup")
            .with_inner_size([600.0, 430.0])
            .with_resizable(false)
            .with_maximize_button(false)
            .with_icon(logo::window_icon(64)),
        centered: true,
        ..Default::default()
    };
    eframe::run_native(
        TITLE,
        options,
        Box::new(move |cc| {
            theme::apply(&cc.egui_ctx);
            Ok(Box::new(Setup::new(uninstalling)))
        }),
    )
}

/// Install or remove without a window, with what would be chosen by default.
fn quietly(uninstalling: bool, folder: Option<PathBuf>) -> Result<(), String> {
    let system = system::current();
    let mut step = |text: &str| eprintln!("{text}…");
    let installed = system.installed_folder();
    if uninstalling {
        let folder = folder.or(installed).ok_or("Linewright is not installed.")?;
        install::uninstall(&folder, system.as_ref(), &mut step)?;
        eprintln!("Linewright is removed.");
        return Ok(());
    }
    let folder = folder.or(installed).unwrap_or_else(|| system.default_folder());
    // An update keeps what was chosen when Linewright was installed.
    let (menu, desktop, associate) = Installed::read(&folder).and_then(|i| i.chosen).unwrap_or((true, cfg!(windows), true));
    let choices = Choices { folder, menu, desktop, associate };
    let program = install::payload().ok_or("This setup was built without the Linewright program in it.")?;
    let setup = std::env::current_exe().and_then(std::fs::read).map_err(|e| format!("The setup cannot read itself: {e}"))?;
    let installed = install::install(&choices, &program, &setup, system.as_ref(), &mut step)?;
    eprintln!("Linewright {VERSION} is installed in {}.", installed.folder.display());
    Ok(())
}

/// What the work going on in the background says.
enum Progress {
    Step(String),
    /// Done: the program installed, if installing; or what went wrong.
    Finished(Result<Option<PathBuf>, String>),
}

enum Page {
    /// Where to install and what to add; or, where it is installed already, the choice to remove it.
    Choose,
    /// The question of removing it.
    Remove,
    Working {
        title: &'static str,
        step: String,
        progress: Receiver<Progress>,
    },
    Installed {
        program: PathBuf,
        open: bool,
    },
    Removed,
    Failed {
        message: String,
    },
}

struct Setup {
    page: Page,
    choices: Choices,
    folder_text: String,
    /// Linewright as installed already, if it is.
    installed: Option<Installed>,
    /// The program, unless the setup was built without it.
    program_ready: bool,
    /// LINEWRIGHT_CAPTURE=out.png saves a screenshot of the window and closes it (debug builds).
    #[cfg(debug_assertions)]
    capture: Option<linewright::ui::debug_capture::Capture>,
}

impl Setup {
    fn new(uninstalling: bool) -> Setup {
        let system = system::current();
        let installed = system.installed_folder().and_then(|folder| Installed::read(&folder));
        let folder = installed.as_ref().map_or_else(|| system.default_folder(), |i| i.folder.clone());
        let page = if uninstalling && installed.is_some() { Page::Remove } else { Page::Choose };
        Setup {
            page,
            folder_text: folder.display().to_string(),
            choices: {
                let (menu, desktop, associate) = installed.as_ref().and_then(|i| i.chosen).unwrap_or((true, cfg!(windows), true));
                Choices { folder, menu, desktop, associate }
            },
            installed,
            program_ready: install::payload().is_some(),
            #[cfg(debug_assertions)]
            capture: linewright::ui::debug_capture::Capture::from_env(),
        }
    }

    /// Start installing or removing on another thread, so the window keeps answering.
    fn start(&mut self, ctx: &egui::Context, removing: bool) {
        let (sender, progress) = mpsc::channel();
        let (choices, folder) = (self.choices.clone(), self.installed.as_ref().map(|i| i.folder.clone()));
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let system = system::current();
            let mut step = |text: &str| {
                let _ = sender.send(Progress::Step(text.to_string()));
                ctx.request_repaint();
            };
            let result = if removing {
                let folder = folder.unwrap_or_else(|| choices.folder.clone());
                install::uninstall(&folder, system.as_ref(), &mut step).map(|_| None)
            } else {
                let program = install::payload().ok_or("This setup was built without the Linewright program in it.".to_string());
                let setup = std::env::current_exe().and_then(std::fs::read).map_err(|e| format!("The setup cannot read itself: {e}"));
                program.and_then(|program| {
                    let setup = setup?;
                    install::install(&choices, &program, &setup, system.as_ref(), &mut step).map(|i| Some(i.folder.join(PROGRAM)))
                })
            };
            let _ = sender.send(Progress::Finished(result));
            ctx.request_repaint();
        });
        let title = if removing { "Removing Linewright" } else { "Installing Linewright" };
        self.page = Page::Working { title, step: "Starting".into(), progress };
    }
}

/// A button that is the page's main way on.
fn main_button(ui: &mut egui::Ui, text: &str, enabled: bool) -> egui::Response {
    let label = RichText::new(text).strong().color(if enabled { theme::CANVAS } else { theme::TEXT_DIM });
    let button = egui::Button::new(label).fill(if enabled { theme::ACCENT } else { theme::PANEL_RAISED }).min_size(Vec2::new(96.0, 30.0));
    ui.add_enabled(enabled, button)
}

fn plain_button(ui: &mut egui::Ui, text: &str) -> egui::Response {
    ui.add(egui::Button::new(text).min_size(Vec2::new(96.0, 30.0)))
}

fn heading(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).size(20.0).color(theme::TEXT).strong());
    ui.add_space(6.0);
}

fn dim(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).color(theme::TEXT_DIM));
}

impl eframe::App for Setup {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        #[cfg(debug_assertions)]
        if let Some(capture) = self.capture.as_mut() {
            capture.step(ctx);
        }
        // News from the work going on.
        let mut next = None;
        if let Page::Working { step, progress, .. } = &mut self.page {
            while let Ok(news) = progress.try_recv() {
                match news {
                    Progress::Step(text) => *step = text,
                    Progress::Finished(Ok(Some(program))) => next = Some(Page::Installed { program, open: true }),
                    Progress::Finished(Ok(None)) => next = Some(Page::Removed),
                    Progress::Finished(Err(message)) => next = Some(Page::Failed { message }),
                }
            }
        }
        if let Some(page) = next {
            self.page = page;
        }

        egui::TopBottomPanel::top("banner").exact_height(76.0).frame(egui::Frame::new().fill(theme::PANEL_DARK)).show(ctx, |ui| {
            ui.horizontal_centered(|ui| {
                ui.add_space(22.0);
                let (square, _) = ui.allocate_exact_size(Vec2::splat(40.0), egui::Sense::hover());
                logo::paint(ui.painter(), square);
                ui.add_space(12.0);
                ui.vertical(|ui| {
                    ui.add_space(16.0);
                    ui.label(RichText::new("Linewright").size(22.0).strong().color(theme::TEXT));
                    ui.label(RichText::new(format!("Version {VERSION}")).color(theme::TEXT_DIM));
                });
            });
        });

        egui::TopBottomPanel::bottom("buttons")
            .exact_height(58.0)
            .frame(egui::Frame::new().fill(theme::PANEL_DARK).inner_margin(egui::Margin::symmetric(22, 0)))
            .show(ctx, |ui| {
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| self.buttons(ui, ctx));
            });

        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(theme::PANEL).inner_margin(egui::Margin::symmetric(26, 20)))
            .show(ctx, |ui| self.body(ui));
    }
}

impl Setup {
    fn body(&mut self, ui: &mut egui::Ui) {
        match &mut self.page {
            Page::Choose => {
                match &self.installed {
                    Some(installed) if installed.version == VERSION => heading(ui, "Reinstall Linewright"),
                    Some(_) => heading(ui, "Update Linewright"),
                    None => heading(ui, "Install Linewright"),
                }
                if let Some(installed) = &self.installed {
                    let version = if installed.version.is_empty() { String::new() } else { format!(" {}", installed.version) };
                    dim(ui, &format!("Linewright{version} is installed in {}.", installed.folder.display()));
                } else {
                    dim(ui, "A 2D CAD program for DXF and DWG drawings.");
                }
                ui.add_space(14.0);
                ui.label("Install in");
                ui.horizontal(|ui| {
                    let field = egui::TextEdit::singleline(&mut self.folder_text).desired_width(ui.available_width() - 104.0);
                    if ui.add_enabled(self.installed.is_none(), field).changed() {
                        self.choices.folder = PathBuf::from(self.folder_text.trim());
                    }
                    if ui.add_enabled(self.installed.is_none(), egui::Button::new("Browse…").min_size(Vec2::new(96.0, 0.0))).clicked()
                        && let Some(mut folder) = rfd::FileDialog::new().set_directory(&self.choices.folder).pick_folder()
                    {
                        // A folder of its own inside the one picked, as other programs' setups do.
                        if !folder.file_name().is_some_and(|name| name.eq_ignore_ascii_case("linewright")) {
                            folder.push("Linewright");
                        }
                        self.folder_text = folder.display().to_string();
                        self.choices.folder = folder;
                    }
                });
                ui.add_space(12.0);
                let menu = if cfg!(windows) { "Add Linewright to the Start menu" } else { "Add Linewright to the applications menu" };
                ui.checkbox(&mut self.choices.menu, menu);
                if cfg!(windows) {
                    ui.checkbox(&mut self.choices.desktop, "Put a shortcut on the desktop");
                }
                ui.checkbox(&mut self.choices.associate, "Open DXF and DWG drawings with Linewright");
                ui.add_space(14.0);
                if self.program_ready {
                    dim(ui, "Linewright is free software under the MIT licence. Your drawings and settings are never changed.");
                } else {
                    ui.label(RichText::new("This setup was built without the Linewright program in it.").color(theme::ERROR));
                }
            }
            Page::Remove => {
                heading(ui, "Remove Linewright");
                let folder = self.installed.as_ref().map(|i| i.folder.clone()).unwrap_or_default();
                ui.label(format!("Linewright will be removed from {}, with its menu entry, shortcuts and file types.", folder.display()));
                ui.add_space(8.0);
                dim(ui, "Your drawings and settings are left as they are.");
            }
            Page::Working { title, step, .. } => {
                heading(ui, title);
                ui.add_space(10.0);
                ui.add(egui::Spinner::new().size(22.0).color(theme::ACCENT));
                ui.add_space(6.0);
                dim(ui, &format!("{step}…"));
            }
            Page::Installed { open, .. } => {
                heading(ui, "Linewright is installed");
                let place = if cfg!(windows) { "the Start menu" } else { "your applications menu" };
                ui.label(if self.choices.menu {
                    format!("Find it in {place}.")
                } else {
                    format!("It is in {}.", self.choices.folder.display())
                });
                ui.add_space(14.0);
                ui.checkbox(open, "Open Linewright now");
            }
            Page::Removed => {
                heading(ui, "Linewright is removed");
                dim(ui, "Your drawings and settings are left as they were.");
            }
            Page::Failed { message } => {
                heading(ui, "Something went wrong");
                ui.label(RichText::new(message.as_str()).color(theme::ERROR));
            }
        }
    }

    fn buttons(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let close = |ctx: &egui::Context| ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        match &self.page {
            Page::Choose => {
                let label = match &self.installed {
                    Some(installed) if installed.version == VERSION => "Reinstall",
                    Some(_) => "Update",
                    None => "Install",
                };
                let ready = self.program_ready && !self.folder_text.trim().is_empty();
                if main_button(ui, label, ready).clicked() {
                    self.start(ctx, false);
                }
                if plain_button(ui, "Cancel").clicked() {
                    close(ctx);
                }
                if self.installed.is_some() {
                    ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                        if plain_button(ui, "Uninstall").clicked() {
                            self.page = Page::Remove;
                        }
                    });
                }
            }
            Page::Remove => {
                if main_button(ui, "Remove", true).clicked() {
                    self.start(ctx, true);
                }
                if plain_button(ui, "Cancel").clicked() {
                    close(ctx);
                }
            }
            Page::Working { .. } => {
                main_button(ui, "Finish", false);
            }
            Page::Installed { program, open } => {
                if main_button(ui, "Finish", true).clicked() {
                    if *open {
                        let _ = std::process::Command::new(program).current_dir(program.parent().unwrap_or(program)).spawn();
                    }
                    close(ctx);
                }
            }
            Page::Removed => {
                if main_button(ui, "Close", true).clicked() {
                    close(ctx);
                }
            }
            Page::Failed { .. } => {
                if plain_button(ui, "Close").clicked() {
                    close(ctx);
                }
                if main_button(ui, "Back", true).clicked() {
                    self.page = Page::Choose;
                }
            }
        }
    }
}
