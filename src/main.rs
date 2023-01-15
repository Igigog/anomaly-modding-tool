// #![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release
mod actions;
mod app;
mod config;
mod backup;
mod addonlist;

use std::{io::Read, path::Path};
use anyhow::Result;

use addonlist::Modpack;
use app::TemplateApp;
use config::ModpackConfig;

use crate::actions::{download_7zip, download_file, unpack_temporary};

/* fn main() {
    eframe::run_native(
        "Anomaly modding tool",
        eframe::NativeOptions::default(),
        Box::new(|cc| Box::new(TemplateApp::new(cc))),
    );
} */

#[tokio::main]
async fn main() -> Result<()> {
    let config_str = include_str!("../resources/config.json");
    let config: ModpackConfig = serde_json::from_str(config_str).unwrap();
    let pack: Modpack = config.into();
    let unpacker = download_7zip().await?;
    let mo_dir = Path::new("mo2");
    pack.install(mo_dir, &unpacker).await?;
    pack.enable(mo_dir).unwrap();

    Ok(())
}
