//! Updates in the window: looking for a newer release once a day (or when asked), offering it, downloading and
//! installing it on another thread, and the dialog and header button that show how it is going. `crate::update`
//! does the work; this decides when, and shows it.

use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};

use eframe::egui::{self, Align, Id, Layout, RichText};

use super::{settings, theme};
use crate::io::files::unix_now;
use crate::update::{self, RELEASES_PAGE, Release, VERSION};

/// The dialog's button that goes on: Update, Restart Now.
fn main_button(text: &str) -> egui::Button<'_> {
    egui::Button::new(RichText::new(text).strong().color(theme::CANVAS)).fill(theme::ACCENT)
}

/// How long after a check the next one is made by itself.
const CHECK_EVERY: u64 = 24 * 60 * 60;

/// Where updating has got to.
#[derive(Clone, Debug, PartialEq)]
pub enum Stage {
    Idle,
    Checking,
    UpToDate,
    Available(Release),
    Downloading(Release, f32),
    Installing(Release),
    /// Installed over the running program, which is at the path given: it takes effect when Linewright starts again.
    Installed(Release, PathBuf),
    /// This Linewright was not installed by the setup, so the new setup's window was opened to install it.
    SetupOpened(Release),
    Failed(String),
}

/// What the work on the other thread says.
enum News {
    Checked(Result<Option<Release>, String>),
    Downloaded(f32),
    Installing,
    /// The program installed over, or none where the setup was opened instead.
    Done(Result<Option<PathBuf>, String>),
}

/// What the dialog asks of the window when a button is pressed.
pub enum Request {
    /// Start the program at this path, and close this one.
    Restart(PathBuf),
}

pub struct Updates {
    pub stage: Stage,
    /// The dialog is open.
    pub shown: bool,
    /// The check was asked for: its answer is shown even when there is nothing new or it failed.
    asked: bool,
    news: Option<Receiver<News>>,
}

impl Default for Updates {
    fn default() -> Self {
        Updates { stage: Stage::Idle, shown: false, asked: false, news: None }
    }
}

impl Updates {
    /// At start: look for a newer release if it is time to, unless turned off. Nothing shows unless one is found.
    pub fn check_if_due(&mut self, ctx: &egui::Context) {
        // A screenshot is of the window as it is, without a check, unless a list of releases to try is given.
        let trying = std::env::var_os("LINEWRIGHT_UPDATE_URL").is_some();
        let turned_off = std::env::var_os("LINEWRIGHT_NO_UPDATE_CHECK").is_some() || std::env::var_os("LINEWRIGHT_CAPTURE").is_some();
        let due = settings::update_checks() && unix_now().saturating_sub(settings::last_update_check()) >= CHECK_EVERY;
        if trying || (!turned_off && due) {
            self.check(ctx, false);
        }
    }

    /// Look for a newer release now. `asked` shows the dialog with the answer, whatever it is.
    pub fn check(&mut self, ctx: &egui::Context, asked: bool) {
        if matches!(self.stage, Stage::Downloading(..) | Stage::Installing(_)) {
            self.shown = true;
            return;
        }
        let (sender, news) = mpsc::channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let _ = sender.send(News::Checked(update::check()));
            ctx.request_repaint();
        });
        self.news = Some(news);
        self.asked = asked;
        self.shown = asked;
        self.stage = Stage::Checking;
    }

    /// Download the release and install it.
    fn start(&mut self, ctx: &egui::Context, release: Release) {
        let (sender, news) = mpsc::channel();
        let ctx = ctx.clone();
        let chosen = release.clone();
        // Where this program is, found before it is replaced: afterwards the system may name it as removed.
        let installed = update::installed_folder().zip(std::env::current_exe().ok());
        std::thread::spawn(move || {
            let mut last = 0.0;
            let mut progress = |done: f32| {
                // A step at a time, so the window is not asked to redraw for every piece of the download.
                if done - last >= 0.01 || done >= 1.0 {
                    last = done;
                    let _ = sender.send(News::Downloaded(done));
                    ctx.request_repaint();
                }
            };
            let result = update::download(&chosen, &mut progress).and_then(|setup| {
                let _ = sender.send(News::Installing);
                ctx.request_repaint();
                match installed {
                    Some((folder, program)) => update::install_quietly(&setup, &folder).map(|_| Some(program)),
                    None => update::open_setup(&setup).map(|_| None),
                }
            });
            let _ = sender.send(News::Done(result));
            ctx.request_repaint();
        });
        self.news = Some(news);
        self.stage = Stage::Downloading(release, 0.0);
    }

    /// Each frame: take in what the work on the other thread has said.
    pub fn tick(&mut self) {
        let Some(news) = &self.news else { return };
        let mut finished = false;
        while let Ok(item) = news.try_recv() {
            match item {
                News::Checked(result) => {
                    finished = true;
                    if result.is_ok() {
                        settings::set_last_update_check(unix_now());
                    }
                    self.stage = match result {
                        Ok(Some(release)) => {
                            // A version skipped is offered again only when asked for.
                            let skipped = settings::skipped_version().is_some_and(|v| v == release.version);
                            self.shown |= self.asked || !skipped;
                            if skipped && !self.asked { Stage::Idle } else { Stage::Available(release) }
                        }
                        Ok(None) if self.asked => Stage::UpToDate,
                        Err(message) if self.asked => Stage::Failed(message),
                        _ => Stage::Idle,
                    };
                }
                News::Downloaded(done) => {
                    if let Stage::Downloading(_, share) = &mut self.stage {
                        *share = done;
                    }
                }
                News::Installing => {
                    if let Stage::Downloading(release, _) = &self.stage {
                        self.stage = Stage::Installing(release.clone());
                    }
                }
                News::Done(result) => {
                    finished = true;
                    let release = match &self.stage {
                        Stage::Downloading(release, _) | Stage::Installing(release) => release.clone(),
                        _ => continue,
                    };
                    self.stage = match result {
                        Ok(Some(program)) => Stage::Installed(release, program),
                        Ok(None) => Stage::SetupOpened(release),
                        Err(message) => Stage::Failed(message),
                    };
                    self.shown = true;
                }
            }
        }
        if finished {
            self.news = None;
        }
    }

    /// The header's button, when there is something to say and the dialog is closed: an update to install, or one
    /// installed that needs a restart.
    pub fn header_button(&mut self, ui: &mut egui::Ui) {
        let text = match &self.stage {
            Stage::Available(release) => format!("Update to {}", release.version),
            Stage::Downloading(_, share) => format!("Downloading update {:.0}%", share * 100.0),
            Stage::Installing(_) => "Installing update".to_string(),
            Stage::Installed(..) => "Restart to update".to_string(),
            _ => return,
        };
        let button =
            egui::Button::new(RichText::new(text).color(theme::CANVAS).strong().size(12.5)).fill(theme::ACCENT).corner_radius(10.0);
        ui.add_space(8.0);
        if ui.add(button).on_hover_text("Linewright updates").clicked() {
            self.shown = true;
        }
        ui.add_space(8.0);
    }

    /// The Linewright Update dialog, when open.
    pub fn dialog(&mut self, ctx: &egui::Context) -> Option<Request> {
        if !self.shown {
            return None;
        }
        let mut request = None;
        let mut close = false;
        let mut start = None;
        let modal = egui::Modal::new(Id::new("updates")).show(ctx, |ui| {
            ui.set_width(440.0);
            ui.label(RichText::new("Linewright Update").family(theme::heading()).size(16.0));
            ui.add_space(8.0);
            let dim = |ui: &mut egui::Ui, text: String| ui.label(RichText::new(text).color(theme::TEXT_DIM));
            match &self.stage {
                Stage::Idle | Stage::Checking => {
                    ui.horizontal(|ui| {
                        ui.add(egui::Spinner::new().color(theme::ACCENT));
                        ui.label("Looking for a newer version…");
                    });
                }
                Stage::UpToDate => {
                    ui.label(format!("Linewright {VERSION} is the newest version."));
                }
                Stage::Available(release) => {
                    ui.label(RichText::new(format!("Linewright {} is available.", release.version)).strong());
                    dim(ui, format!("You have {VERSION}."));
                    ui.add_space(4.0);
                    ui.hyperlink_to("What's new", &release.page);
                    ui.add_space(4.0);
                    dim(
                        ui,
                        if update::installed_folder().is_some() {
                            "It is downloaded and installed over this one, and used when Linewright starts again.".to_string()
                        } else {
                            "Its setup is downloaded and opened to install it.".to_string()
                        },
                    );
                }
                Stage::Downloading(release, share) => {
                    ui.label(format!("Downloading Linewright {}…", release.version));
                    ui.add_space(6.0);
                    ui.add(egui::ProgressBar::new(*share).show_percentage());
                }
                Stage::Installing(release) => {
                    ui.horizontal(|ui| {
                        ui.add(egui::Spinner::new().color(theme::ACCENT));
                        ui.label(format!("Installing Linewright {}…", release.version));
                    });
                }
                Stage::Installed(release, _) => {
                    ui.label(RichText::new(format!("Linewright {} is installed.", release.version)).strong());
                    dim(ui, "Restart Linewright to use it. You are asked to save your drawing first.".to_string());
                }
                Stage::SetupOpened(release) => {
                    ui.label(format!("The setup for Linewright {} is open: follow it to install the update.", release.version));
                }
                Stage::Failed(message) => {
                    ui.label(RichText::new(message.as_str()).color(theme::ERROR));
                    ui.add_space(4.0);
                    ui.hyperlink_to("Download it from the releases page", RELEASES_PAGE);
                }
            }
            ui.add_space(10.0);
            let mut automatic = settings::update_checks();
            if ui.checkbox(&mut automatic, "Check for updates automatically").changed() {
                settings::set_update_checks(automatic);
            }
            ui.add_space(10.0);
            ui.with_layout(Layout::right_to_left(Align::Min), |ui| match &self.stage {
                Stage::Available(release) => {
                    if ui.add(main_button("Update")).clicked() {
                        start = Some(release.clone());
                    }
                    if ui.button("Later").clicked() {
                        close = true;
                    }
                    if ui.button("Skip This Version").clicked() {
                        settings::set_skipped_version(&release.version);
                        self.stage = Stage::Idle;
                        close = true;
                    }
                }
                Stage::Installed(_, program) => {
                    if ui.add(main_button("Restart Now")).clicked() {
                        request = Some(Request::Restart(program.clone()));
                        close = true;
                    }
                    if ui.button("Later").clicked() {
                        close = true;
                    }
                }
                Stage::Failed(_) => {
                    if ui.button("Try Again").clicked() {
                        start = None;
                        request = None;
                        self.asked = true;
                        self.stage = Stage::Checking;
                    }
                    if ui.button("Close").clicked() {
                        close = true;
                    }
                }
                Stage::Downloading(..) | Stage::Installing(_) | Stage::Checking => {
                    if ui.button("Hide").clicked() {
                        close = true;
                    }
                }
                Stage::Idle | Stage::UpToDate | Stage::SetupOpened(_) => {
                    if ui.button("OK").clicked() {
                        close = true;
                    }
                }
            });
        });
        if self.stage == Stage::Checking && self.news.is_none() {
            self.check(ctx, true);
        }
        if let Some(release) = start {
            self.start(ctx, release);
        }
        if close || modal.should_close() {
            self.shown = false;
            if matches!(self.stage, Stage::UpToDate | Stage::Failed(_) | Stage::SetupOpened(_)) {
                self.stage = Stage::Idle;
            }
        }
        request
    }
}
