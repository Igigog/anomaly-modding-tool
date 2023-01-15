use anyhow::{anyhow, bail, Result};
use futures_util::stream::StreamExt;
use once_cell::sync::Lazy;

use regex::Regex;
use reqwest::IntoUrl;
use std::{ffi::OsString, fs, os::windows::process::CommandExt, path::Path};
use tempfile::{NamedTempFile, TempDir, TempPath};
use tokio::runtime::Runtime;

use crate::{app::AppContext, backup::BasicTransaction};

static CLIENT: Lazy<reqwest::Client> = Lazy::new(|| {
    reqwest::Client::builder()
        .user_agent("Anomaly-Modder-Tool")
        .build()
        .unwrap()
});

static LINKS_REGEX: Lazy<Regex> = Lazy::new(|| regex::Regex::new("href=\"([^\"]*)\"").unwrap());

static MODORG_INI: &str = include_str!("../resources/ModOrganizer.ini");
static NXMHANDLER: &str = include_str!("../resources/nxmhandler.ini");

/* static VANILLA_EXES: &[u8] = include_bytes!("../resources/Vanilla_Exes.zip"); */

static URL_7ZIP: &str = "https://www.7-zip.org/a/7zr.exe";
static URL_MODORG: &str = "https://github.com/ModOrganizer2/modorganizer/releases";
static URL_MODDED_EXES: &str = "https://github.com/themrdemonized/STALKER-Anomaly-modded-exes";

pub struct Unpacker7Zip<P: AsRef<Path>> {
    path: P,
}

pub trait Unpack7Zip {
    fn unpack(&self, file_path: &Path, out_dir: &Path) -> Result<()>;
}

impl<P: AsRef<Path>> Unpacker7Zip<P> {
    pub fn new(path: P) -> Self {
        let path_str = path.as_ref().as_os_str();

        if cfg!(debug_assertions) {
            let successful = std::process::Command::new(path_str)
                .args(["i".to_owned()])
                .creation_flags(0x08000000) // Create no console window
                .status()
                .expect("7zip path is not executable")
                .success();
            assert!(successful, "Not 7zip or not executable")
        }

        Self { path }
    }
}

impl<P: AsRef<Path>> Unpack7Zip for &Unpacker7Zip<P> {
    fn unpack(&self, file_path: &Path, out_dir: &Path) -> Result<()> {
        debug_assert!(!out_dir.is_file(), "Output directory is a file");
        let cmd: OsString = "x".into();
        let out_arg = {
            let mut x = OsString::new();
            x.push("-o");
            x.push(out_dir.as_os_str());

            x
        };
        let status = std::process::Command::new(self.path.as_ref().as_os_str())
            .args([&cmd, &out_arg, file_path.as_os_str()])
            // .creation_flags(0x08000000) // Create no console window
            .status()?;

        if status.success() {
            Ok(())
        } else {
            bail!("7zip was not successful")
        }
    }
}

pub trait AppAction {
    type Output;
    type Progress;
    type Config;
    fn run(
        config: Self::Config,
        ctx: impl AsRef<AppContext>,
        progress: impl FnMut(&Self::Progress),
    ) -> Result<Self::Output>;
}

#[derive(Default, Clone)]
pub struct DownloadProgress {
    pub file_name: Option<String>,
    pub size: Option<u64>,
    pub downloaded: u64,
}

pub struct InstallMo2;

impl InstallMo2 {
    async fn scrape_mo2_url() -> Result<impl IntoUrl> {
        let resp = CLIENT.get(URL_MODORG).send().await?.text().await?;

        let tag = LINKS_REGEX
            .captures_iter(&resp)
            .map(|s| s.get(1).unwrap())
            .find(|s| s.as_str().contains("tag/") && !s.as_str().contains("rc"))
            .map(|s| {
                s.as_str()
                    .split('/')
                    .last()
                    .unwrap()
                    .to_owned()
                    .replace('v', "")
            })
            .ok_or_else(|| anyhow!("Couldn't find last version tag"))?;
        Ok(format!(
            "https://github.com/ModOrganizer2/modorganizer/releases/download/v{0}/Mod.Organizer-{0}.7z",
            tag
        ))
    }

    async fn download_mod_org(
        progress_callback: impl FnMut(&DownloadProgress),
    ) -> Result<tempfile::NamedTempFile> {
        let url = Self::scrape_mo2_url().await?;
        download_file(url, tempfile::NamedTempFile::new()?, progress_callback).await
    }

    fn configure_mo2(mo_path: &Path, anomaly_path: &Path) -> Result<()> {
        let anomaly_path_str = anomaly_path.to_str().unwrap();
        let content: Vec<u8> = MODORG_INI
            .lines()
            .map(|l| {
                l.replace("D:/Games/Anomaly", &anomaly_path_str.replace('\\', "/"))
                    .replace(
                        r"D:\\Games\\Anomaly",
                        &anomaly_path_str.replace('\\', r"\\"),
                    )
                    + "\n"
            })
            .flat_map(|s| s.bytes().collect::<Vec<_>>())
            .collect();

        let mut modorg_config = std::fs::File::create(mo_path.join("ModOrganizer.ini"))?;
        std::io::copy(&mut content.as_slice(), &mut modorg_config)?;

        let mut nxm = std::fs::File::create(mo_path.join("nxmhandler.ini"))?;
        std::io::copy(&mut NXMHANDLER.as_bytes(), &mut nxm)?;
        Ok(())
    }
}

#[derive(Default, Clone)]
pub struct InstallMo2Progress {
    pub download: Option<DownloadProgress>,
    pub unpacking_done: Option<bool>,
    pub configuring_done: Option<bool>,
    pub finished: bool,
}

impl AppAction for InstallMo2 {
    type Output = ();
    type Progress = InstallMo2Progress;
    type Config = ();

    fn run(
        _config: Self::Config,
        ctx: impl AsRef<AppContext>,
        mut progress_callback: impl FnMut(&Self::Progress),
    ) -> Result<Self::Output> {
        let ctx = ctx.as_ref();
        let unpacker_7zip = ctx.unpacker_7zip.as_ref().unwrap();
        let mut progress = Self::Progress::default();

        let mod_org = Runtime::new()
            .unwrap()
            .block_on(Self::download_mod_org(|p| {
                progress.download = Some(p.clone());
                progress_callback(&progress);
            }))
            .unwrap();

        progress.unpacking_done = Some(false);
        progress_callback(&progress);

        let modorg_tmp = unpack_temporary(&unpacker_7zip, mod_org, |_| {})?;

        progress.unpacking_done = Some(true);
        progress.configuring_done = Some(false);
        progress_callback(&progress);

        Self::configure_mo2(modorg_tmp.path(), &ctx.anomaly_dir)?;

        progress.configuring_done = Some(true);

        let tr = BasicTransaction::new(modorg_tmp)?;

        let mo_dir = ctx.anomaly_dir.join("mo2");
        let backup_dir = ctx.anomaly_dir.join("BACKUP");
        let safe_tr = tr.backup(&mo_dir, &backup_dir)?;

        let done = safe_tr.run();

        progress.finished = true;
        progress_callback(&progress);

        done
    }
}

pub struct InstallModdedExes;

impl InstallModdedExes {
    async fn download_modded_exes() -> Result<tempfile::NamedTempFile> {
        let resp = CLIENT.get(URL_MODDED_EXES).send().await?.text().await?;
        let url = format!(
            "https://github.com{}",
            LINKS_REGEX
                .captures_iter(&resp)
                .map(|c| c.get(1).unwrap())
                .find(|s| s.as_str().ends_with(".zip") && !s.as_str().ends_with("main.zip"))
                .map(|s| s.as_str().replace("blob", "raw"))
                .ok_or_else(|| anyhow!("Couldn't find the link for modded exes"))?
        );

        download_file(url, tempfile::NamedTempFile::new()?, |_p| {
            {};
        })
        .await
    }
}

impl AppAction for InstallModdedExes {
    type Output = ();
    type Progress = ();
    type Config = ();
    fn run(
        _config: Self::Config,
        ctx: impl AsRef<AppContext>,
        _progress: impl FnMut(&Self::Progress),
    ) -> Result<Self::Output> {
        let file = Runtime::new()?.block_on(Self::download_modded_exes())?;
        let tmp_dir = tempfile::tempdir()?;
        unpack_zip(file.as_file(), tmp_dir.path(), |_| {})?;
        let tr = BasicTransaction::new(tmp_dir)?;
        let backup_dir = ctx.as_ref().anomaly_dir.join("BACKUP_Vanilla_exes");
        let safe = tr.backup(&ctx.as_ref().anomaly_dir, &backup_dir)?;
        safe.run()?;
        Ok(())
    }
}

pub async fn download_and_unpack(url: impl IntoUrl, unpacker: &impl Unpack7Zip) -> Result<TempDir> {
    let file = download_file(url, tempfile::NamedTempFile::new()?, |p| {
        println!(
            "Downloading {:#?}: {}/{:#?}",
            p.file_name, p.downloaded, p.size
        );
    })
    .await?;
    unpack_temporary(unpacker, file, |_| {})
}

pub async fn download_file<W: std::io::Write>(
    url: impl IntoUrl,
    mut file: W,
    mut progress_callback: impl FnMut(&DownloadProgress),
) -> Result<W> {
    let regex = Regex::new("filename ?= ?\"?([[:^space:]]*)\"?").unwrap();
    let response = CLIENT.get(url).send().await?;
    let filename = response
        .headers()
        .get(http::header::CONTENT_DISPOSITION)
        .iter()
        .flat_map(|h| h.to_str())
        .flat_map(|s| regex.captures(s))
        .map(|c| c.get(1).unwrap().as_str().to_owned())
        .next();
    let mut progress = DownloadProgress {
        file_name: filename,
        size: response.content_length(),
        downloaded: 0,
    };
    progress_callback(&progress);

    let mut stream = response.bytes_stream();

    while let Some(item) = stream.next().await {
        let chunk = item?;
        file.write_all(&chunk)?;

        progress.downloaded += chunk.len() as u64;
        progress_callback(&progress);
    }

    Ok(file)
}

pub async fn download_7zip() -> Result<Unpacker7Zip<TempPath>> {
    let tmpfile = download_file(URL_7ZIP, tempfile::NamedTempFile::new()?, |_p| {
        {};
    })
    .await?;
    Ok(Unpacker7Zip::new(tmpfile.into_temp_path()))
}

pub struct UnpackZipProgress {
    unpacked: Vec<String>,
}

pub fn unpack_temporary(
    unpacker_7zip: &impl Unpack7Zip,
    file: NamedTempFile,
    progress_callback: impl FnMut(&UnpackZipProgress),
) -> Result<TempDir> {
    let tempdir = tempfile::Builder::new().tempdir()?;
    let (file, path) = file.into_parts();
    let unpacked_zip = unpack_zip(&file, tempdir.path(), progress_callback);
    if unpacked_zip.is_ok() {
        return Ok(tempdir);
    }

    drop(file);
    unpacker_7zip.unpack(&path, tempdir.path()).map(|_| tempdir)
}

fn unpack_zip<R>(
    file: R,
    out_dir: &Path,
    mut progress_callback: impl FnMut(&UnpackZipProgress),
) -> Result<()>
where
    R: std::io::Seek,
    R: std::io::Read,
{
    debug_assert!(!out_dir.is_file(), "Output directory is a file");
    let mut archive = zip::ZipArchive::new(file)?;
    let mut progress = UnpackZipProgress {
        unpacked: Vec::with_capacity(archive.len()),
    };
    progress_callback(&progress);

    for i in 0..archive.len() {
        let mut file = archive.by_index(i).unwrap();
        let outpath = match file.enclosed_name() {
            Some(path) => out_dir.join(path),
            None => bail!("Zip is ill-formed!"),
        };

        if (*file.name()).ends_with('/') {
            fs::create_dir_all(&outpath).unwrap();
        } else {
            if let Some(p) = outpath.parent() {
                if !p.exists() {
                    fs::create_dir_all(p).unwrap();
                }
            }
            let mut outfile = fs::File::create(&outpath).unwrap();
            std::io::copy(&mut file, &mut outfile).unwrap();
        }

        progress
            .unpacked
            .push(outpath.to_string_lossy().into_owned());
        progress_callback(&progress);
    }
    Ok(())
}
