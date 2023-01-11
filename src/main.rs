#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // hide console window on Windows in release
mod actions;
mod app;
mod config;
mod response_structs;
mod backup;

use std::{ffi::OsString, io::Read};

use app::TemplateApp;

use crate::actions::{download_7zip, download_file, unpack_temporary};

#[tokio::main]
async fn main() {
    /* eframe::run_native(
        "Anomaly modding tool",
        eframe::NativeOptions::default(),
        Box::new(|cc| Box::new(TemplateApp::new(cc))),
    ); */

    let mut file = std::fs::File::open("config.json").unwrap();
    let mut data = Vec::new();
    file.read_to_end(&mut data).unwrap();
    let text = String::from_utf8(data).unwrap();
    let config: config::ModpackConfig = serde_json::from_str(&text).unwrap();
    dbg!(&config);

    let l = actions::scrape_moddb_url(
        &config
            .mods
            .get("Anomaly-Mod-Configuration-Menu")
            .unwrap()
            .moddb
            .as_ref()
            .unwrap()
            .url,
    )
    .await
    .unwrap();

    let tmp = tempfile::NamedTempFile::new().unwrap();
    let f = download_file(l, tmp, |p| {
        println!(
            "Downloading {} {}/{}",
            &p.file_name
                .as_ref()
                .map(|s| s.to_owned())
                .unwrap_or_else(||"idk".to_owned()),
            p.downloaded,
            p.size.unwrap_or(0)
        );
    })
    .await
    .unwrap();

    let unpacker = download_7zip().await.unwrap();
    let up = unpack_temporary(&unpacker, f, |_| {}).unwrap();
    let dirs: Vec<std::path::PathBuf> = walkdir::WalkDir::new(up.path())
        .into_iter()
        .filter_map(|x| x.ok())
        .filter(|e| e.file_name() == "gamedata")
        .map(|e| e.path().parent().unwrap().to_path_buf())
        .collect();
    dbg!(&dirs);
    dbg!(dirs.get(0).unwrap().as_path().parent().unwrap() == up.path());
}
