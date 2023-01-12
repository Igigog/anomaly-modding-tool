use anyhow::Result;
use egui::output::OpenUrl;
use parking_lot::Mutex;
use std::{path::PathBuf, sync::Arc, thread::JoinHandle};

use crate::actions::{
    download_7zip, AppAction, InstallMo2, InstallMo2Progress, InstallModdedExes, Unpacker7Zip,
};

enum AppState {
    NoAnomaly,
    GameNotInitialized,
    Normal,
    InstallMo2(Operation<InstallMo2>),
    InstallModdedExes(Operation<InstallModdedExes>),
}

trait Gui {
    fn paint(
        &self,
        ctx: &egui::Context,
        frame: &mut eframe::Frame,
        app_ctx: Arc<AppContext>,
    ) -> Option<AppState>;
}

struct Operation<T: AppAction> {
    handle: JoinHandle<Result<T::Output>>,
    progress: Arc<Mutex<T::Progress>>,
}

impl Gui for Operation<InstallMo2> {
    fn paint(
        &self,
        ctx: &egui::Context,
        frame: &mut eframe::Frame,
        app_ctx: Arc<AppContext>,
    ) -> Option<AppState> {
        TemplateApp::paint_secondary_panels(ctx, false, app_ctx);

        let lock = &self.progress.lock();

        let download_progress = |ui: &mut egui::Ui| {
            if let Some(dl) = &lock.download {
                let mo = "Mod Organizer".to_owned();
                ui.label(format!(
                    "Downloading {}: {:.2} mb / {} mb",
                    dl.file_name.as_ref().unwrap_or(&mo),
                    dl.downloaded as f64 / 1024.0 / 1024.0,
                    dl.size
                        .map(|s| format!("{:.2}", s as f64 / 1024.0 / 1024.0))
                        .unwrap_or_else(|| "Unknown".to_owned())
                ));
            }
        };

        let unzip_progress = |ui: &mut egui::Ui| {
            if let Some(x) = lock.unpacking_done {
                ui.label(if x {
                    "Unpacking... Done."
                } else {
                    "Unpacking..."
                });
            }
        };

        let configure_progress = |ui: &mut egui::Ui| {
            if let Some(x) = lock.configuring_done {
                ui.label(if x {
                    "Configuring... Done."
                } else {
                    "Configuring..."
                });
            }
        };

        egui::CentralPanel::default().show(ctx, |ui| {
            download_progress(ui);
            unzip_progress(ui);
            configure_progress(ui);
            egui::warn_if_debug_build(ui);
        });
        if lock.finished {
            return egui::Window::new("Install Mo2")
                .auto_sized()
                .show(ctx, |ui| {
                    ui.heading("All good! Done.");
                    if ui.button("Okay ^^").clicked() {
                        return Some(AppState::Normal);
                    }
                    None
                })
                .unwrap()
                .inner
                .unwrap();
        }
        None
    }
}

impl Gui for Operation<InstallModdedExes> {
    fn paint(
        &self,
        ctx: &egui::Context,
        frame: &mut eframe::Frame,
        app_ctx: Arc<AppContext>,
    ) -> Option<AppState> {
        TemplateApp::paint_secondary_panels(ctx, false, app_ctx);
        egui::CentralPanel::default().show(ctx, |ui| {
            egui::warn_if_debug_build(ui);
        });
        None
    }
}

pub struct AppContext {
    pub anomaly_dir: PathBuf,
    pub mo_dir: Option<PathBuf>,
    pub unpacker_7zip: Option<Unpacker7Zip<tempfile::TempPath>>,
}

pub struct TemplateApp {
    state: AppState,
    context: Arc<AppContext>,
}

impl Default for TemplateApp {
    fn default() -> Self {
        let anomaly_dir = std::env::current_dir().unwrap();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let unpacker_7zip = runtime.block_on(download_7zip()).ok();
        let mo_dir = std::path::Path::new("mo2");

        let anomaly_exists = anomaly_dir.join("AnomalyLauncher.exe").is_file();
        let game_initialized = anomaly_dir.join("appdata\\user.ltx").is_file();
        Self {
            context: Arc::new(AppContext {
                mo_dir: if mo_dir.exists() {
                    Some(mo_dir.to_path_buf())
                } else {
                    None
                },
                anomaly_dir,
                unpacker_7zip,
            }),
            state: if !anomaly_exists {
                AppState::NoAnomaly
            } else if !game_initialized {
                AppState::GameNotInitialized
            } else {
                AppState::Normal
            },
        }
    }
}

impl TemplateApp {
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        Default::default()
    }

    fn paint_game_not_initialized(
        &self,
        ctx: &egui::Context,
        _frame: &mut eframe::Frame,
    ) -> Option<AppState> {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.centered_and_justified(|ui| {
                ui.heading("Anomaly not initialized. Run the game and load into a new world once to avoid crashes.");
            });
        });
        None
    }

    fn paint_no_game(&self, ctx: &egui::Context, _frame: &mut eframe::Frame) -> Option<AppState> {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.centered_and_justified(|ui| {
                ui.heading("Anomaly not found! Run this program from Anomaly's root folder.");
            });
        });
        None
    }

    fn paint_secondary_panels(
        ctx: &egui::Context,
        input_enabled: bool,
        app_ctx: Arc<AppContext>,
    ) -> Option<AppState> {
        let book_button = |ui: &mut egui::Ui| -> Option<AppState> {
            if ui
                .add_enabled(input_enabled, egui::Button::new("Modding Book"))
                .clicked()
            {
                ctx.output().open_url = Some(OpenUrl::new_tab(
                    "https://igigog.github.io/anomaly-modding-book/",
                ));
            };
            None
        };

        let mo2_button = |ui: &mut egui::Ui| -> Option<AppState> {
            if !ui
                .add_enabled(input_enabled, egui::Button::new("Install MO2"))
                .clicked()
            {
                return None;
            };

            let app_ctx = app_ctx.clone();
            let gui_ctx = ctx.clone();
            let progress = Arc::new(Mutex::new(InstallMo2Progress::default()));
            let progress_cl = progress.clone();
            let handle = std::thread::spawn(move || {
                InstallMo2::run((), app_ctx, |p| {
                    *progress_cl.lock() = p.clone();
                    gui_ctx.request_repaint();
                })
            });
            Some(AppState::InstallMo2(Operation::<InstallMo2> {
                handle,
                progress,
            }))
        };

        let modded_exes_button = |ui: &mut egui::Ui| {
            if !ui
                .add_enabled(input_enabled, egui::Button::new("Install Modded Exes"))
                .clicked()
            {
                return None;
            };

            let app_ctx = app_ctx.clone();
            let gui_ctx = ctx.clone();
            let handle = std::thread::spawn(move || {
                InstallModdedExes::run((), app_ctx, |p| {
                    gui_ctx.request_repaint();
                })
            });
            Some(AppState::InstallModdedExes(
                Operation::<InstallModdedExes> {
                    handle,
                    progress: Arc::new(Mutex::new(())),
                },
            ))
        };

        egui::SidePanel::left("side_panel")
            .show(ctx, |ui| {
                ui.with_layout(egui::Layout::top_down_justified(egui::Align::TOP), |ui| {
                    book_button(ui);
                    let mo_state = mo2_button(ui);
                    let exes_state = modded_exes_button(ui);
                    mo_state.or(exes_state)
                })
                .inner
            })
            .inner
    }

    fn paint_normal(ctx: &egui::Context, app_ctx: Arc<AppContext>) -> Option<AppState> {
        if let Some(s) = Self::paint_secondary_panels(ctx, true, app_ctx) {
            return Some(s);
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            // The central panel the region left after adding TopPanel's and SidePanel's
            ui.heading("Привет мой гуй! ^^");
            egui::warn_if_debug_build(ui);
        });
        None
    }
}

impl eframe::App for TemplateApp {
    fn save(&mut self, _storage: &mut dyn eframe::Storage) {}

    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        use AppState::*;

        let next_state = match &self.state {
            NoAnomaly => self.paint_no_game(ctx, frame),
            GameNotInitialized => self.paint_game_not_initialized(ctx, frame),
            Normal => Self::paint_normal(ctx, self.context.clone()),
            InstallMo2(op) => op.paint(ctx, frame, self.context.clone()),
            InstallModdedExes(op) => op.paint(ctx, frame, self.context.clone()),
        };
        if let Some(s) = next_state {
            self.state = s;
        }
    }
}
