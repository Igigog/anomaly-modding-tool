use std::{
    borrow::Cow,
    collections::{hash_map::Entry, HashMap, HashSet},
    fs::File,
    io::stdin,
    path::{Path, PathBuf},
};

use anyhow::{anyhow, bail, Result};
use once_cell::sync::Lazy;
use regex::Regex;
use reqwest::IntoUrl;
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;
use tempfile::{tempdir, TempDir};

use crate::{
    actions::{download_and_unpack, Unpack7Zip},
    backup::{BasicTransaction, ComplexTransaction},
    config::ModpackConfig,
};

static LOADORDER_HEADER: &str =
    "# This file was automatically generated by Anomaly Modding Tool. Sorry if it broke lol.\n";

#[derive(Default)]
pub struct Modpack {
    addons: Addons,
    order: LoadOrder,
}

impl Modpack {
    pub async fn install(&self, mo_dir: &Path, unpacker: &impl Unpack7Zip) -> Result<()> {
        let modpack = tempdir()?;
        let mut cache = DownloadCache::new();
        let mut tr = ComplexTransaction::new();
        for addon in self.addons.missing_addons(mo_dir) {
            let entry = self.addons.get(addon).unwrap();
            let dl_dir = cache.get_or_download(&entry.download, unpacker).await?;
            let tr = Addons::install(entry, unpacker, &dl_dir).await?;
            let addon_dir = modpack.path().join(addon);
            std::fs::create_dir(&addon_dir).unwrap_or(());
            tr.run(&addon_dir)?;
        }

        let tr = BasicTransaction::new(modpack)?;
        let backup = tempdir()?;
        tr.backup(&mo_dir.join("mods"), backup.path())?.run()?;
        Ok(())
    }
}

impl From<ModpackConfig> for Modpack {
    fn from(value: ModpackConfig) -> Self {
        let mut pack = Modpack::default();
        for (folder, entry) in value.mods {
            pack.order.push(folder.clone());
            pack.addons.insert(folder, entry);
        }
        pack
    }
}

#[derive(Default)]
pub struct DownloadCache<'a>(HashMap<&'a AddonKey, TempDir>);

impl<'a> DownloadCache<'a> {
    async fn get_or_download(
        &mut self,
        key: &'a AddonKey,
        unpacker: &impl Unpack7Zip,
    ) -> Result<PathBuf> {
        dbg!(serde_json::to_string(key).unwrap());
        let entry = self.0.entry(key);
        let dir = match &entry {
            Entry::Occupied(_) => None,
            Entry::Vacant(_) => {
                let url = key.download_link().await?;
                let dl_dir = download_and_unpack(url, unpacker).await?;
                Some(dl_dir)
            }
        };
        dbg!(&dir);
        Ok(entry.or_insert_with(|| dir.unwrap()).path().to_owned())
    }

    fn new() -> Self {
        Self::default()
    }
}

#[derive(Default)]
struct Addons(HashMap<String, FolderEntry>);

#[derive(Default)]
struct LoadOrder(Vec<String>);

impl AsRef<[String]> for LoadOrder {
    fn as_ref(&self) -> &[String] {
        &self.0
    }
}

impl LoadOrder {
    fn new() -> Self {
        Self::default()
    }

    fn push(&mut self, addon: String) {
        self.0.push(addon)
    }

    fn change_position(&mut self, addon: &str, pos: usize) -> Result<()> {
        debug_assert!(pos <= self.0.len());
        let ix = self
            .0
            .iter()
            .position(|s| s.as_str() == addon)
            .ok_or_else(|| anyhow!("No such element"))?;
        let a = self.0.remove(ix);
        self.0.insert(pos, a);
        Ok(())
    }

    fn to_modorg_modlist(&self, all: &Addons) -> String {
        let mut s = LOADORDER_HEADER.to_owned();
        let enabled_mods = self.0.iter().map(|s| "+".to_owned() + s + "\n");
        let disabled_mods = all
            .0
            .keys()
            .filter(|k| !self.0.contains(k))
            .map(|s| "-".to_owned() + s + "\n");
        s.extend(enabled_mods);
        s.extend(disabled_mods);
        s
    }
}

impl Addons {
    fn new() -> Self {
        Self::default()
    }

    fn entry(&mut self, key: String) -> Entry<String, FolderEntry> {
        self.0.entry(key)
    }

    fn missing_addons(&self, mo_dir: &Path) -> Vec<&str> {
        let mods_dir = mo_dir.join("mods");
        self.0
            .keys()
            .filter(|m| !mods_dir.join(m).is_dir())
            .map(|m| m.as_str())
            .collect()
    }

    fn insert(&mut self, name: String, entry: FolderEntry) -> Option<FolderEntry> {
        self.0.insert(name, entry)
    }

    pub fn get(&self, folder: &str) -> Option<&FolderEntry> {
        self.0.get(folder)
    }

    fn find_addons<'a>(folders: impl Iterator<Item = impl AsRef<Path>>) -> HashSet<PathBuf> {
        folders
            .filter(|d| d.as_ref().file_name().unwrap() == "gamedata")
            .map(|d| d.as_ref().parent().unwrap().to_path_buf())
            .collect()
    }

    fn with_entries<'a>(
        addons: impl Iterator<Item = &'a Path>,
        key: &AddonKey,
        root_folder: &'a Path,
    ) -> HashMap<&'a Path, FolderEntry> {
        addons
            .into_iter()
            .map(|p| {
                (
                    p,
                    FolderEntry::new(
                        key.clone(),
                        if p == root_folder {
                            None
                        } else {
                            Some(p.file_name().unwrap().to_str().unwrap().to_owned())
                        },
                    ),
                )
            })
            .collect()
    }

    async fn install<'a>(
        entry: &'a FolderEntry,
        unpacker: &impl Unpack7Zip,
        dl_dir: &Path,
    ) -> Result<BasicTransaction> {
        let folders = walkdir::WalkDir::new(&dl_dir)
            .into_iter()
            .filter_map(|d| d.ok())
            .map(|d| d.into_path());
        let addon_folders = Self::find_addons(folders);

        let dir_name = entry
            .addon_folder
            .as_ref()
            .and_then(|s| {
                addon_folders
                    .iter()
                    .find(|p| p.file_name().unwrap() == s.as_str())
            })
            .or_else(|| addon_folders.get(dl_dir))
            .map(|p| (*p).to_owned())
            .ok_or_else(|| anyhow!("Can't find addon folder"))?;

        let tr = BasicTransaction::new(dir_name)?;
        Ok(tr)
    }
}

#[skip_serializing_none]
#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
pub struct FolderEntry {
    pub download: AddonKey,
    pub addon_folder: Option<String>,
}

impl FolderEntry {
    fn new(key: AddonKey, folder: Option<String>) -> Self {
        Self {
            download: key,
            addon_folder: folder,
        }
    }
}

#[derive(Debug, Serialize, Deserialize, Hash, PartialEq, Clone, Eq)]
pub struct ModdbLink {
    pub addon_link: String,
    pub updated: String,
}

#[derive(Debug, Serialize, Deserialize, Hash, PartialEq, Clone, Eq)]
pub struct GithubLink {
    pub repo: String,
    pub tag: String,
    pub filename: String,
}

#[derive(Debug, Serialize, Deserialize, Hash, PartialEq, Clone, Eq)]
pub struct UrlLink {
    url: String,
}

impl UrlLink {
    fn get_download_url(&self) -> String {
        self.url.clone()
    }
}

#[derive(Debug, Hash, Serialize, Deserialize, PartialEq, Clone, Eq)]
#[non_exhaustive]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum AddonKey {
    Moddb(ModdbLink),
    Github(GithubLink),
    Url(UrlLink),
}

static CLIENT: Lazy<reqwest::Client> = Lazy::new(|| {
    reqwest::Client::builder()
        .user_agent("Anomaly-Modder-Tool")
        .build()
        .unwrap()
});

static LINKS_REGEX: Lazy<Regex> = Lazy::new(|| regex::Regex::new("href=\"([^\"]*)\"").unwrap());
const URL_MODDB: &str = "https://www.moddb.com/mods/stalker-anomaly/addons/";

impl ModdbLink {
    pub async fn get_download_url(&self) -> Result<String> {
        let url = format!("{url}{addon}", url = URL_MODDB, addon = self.addon_link);
        let resp = CLIENT.get(url).send().await?.text().await?;

        let link = LINKS_REGEX
            .captures_iter(&resp)
            .map(|s| s.get(1).unwrap())
            .find(|s| s.as_str().contains("addons/start"))
            .map(|m| m.as_str().to_owned())
            .ok_or_else(|| anyhow!("Couldn't find moddb download button"))?;

        let resp = CLIENT
            .get(format!("https://www.moddb.com{}", link))
            .send()
            .await?
            .text()
            .await?;
        let link = LINKS_REGEX
            .captures_iter(&resp)
            .map(|s| s.get(1).unwrap())
            .find(|s| s.as_str().contains("moddb.com/downloads/mirror"))
            .map(|m| m.as_str().to_owned())
            .ok_or_else(|| anyhow!("Couldn't find moddb mirror link"))?;
        Ok(link)
    }
}

impl GithubLink {
    async fn fetch_tag(&self) -> Result<Cow<str>> {
        if self.tag != "latest" {
            return Ok(Cow::Borrowed(&self.tag));
        }

        let resp = CLIENT
            .head(format!(
                r"https://github.com/{repo}/releases/latest",
                repo = self.repo
            ))
            .send()
            .await?;

        if !resp.status().is_success() {
            bail!("No such repo");
        }

        let last_segment = resp.url().path_segments().unwrap().last().unwrap();
        let tag = match last_segment {
            "releases" => bail!("No releases in repo"),
            x => x,
        };
        Ok(Cow::Owned(tag.to_owned()))
    }

    pub async fn get_download_url(&self) -> Result<String> {
        let tag = self.fetch_tag().await?;
        let version = match tag.strip_prefix('v') {
            // if tag starts with v (v3.2 for example), strips v
            Some(v) => v,
            None => &tag,
        };
        let filename = self.filename.replace("$VERSION", version);
        Ok(format!(
            "https://github.com/{repo}/releases/download/{tag}/{filename}",
            repo = self.repo
        ))
    }
}

impl AddonKey {
    async fn download_link(&self) -> Result<impl IntoUrl> {
        use AddonKey::*;

        match self {
            Moddb(link) => link.get_download_url().await,
            Url(link) => Ok(link.get_download_url()),
            Github(link) => link.get_download_url().await,
        }
    }

    fn from_moddb(link: ModdbLink) -> Self {
        Self::Moddb(link)
    }

    fn from_url(link: UrlLink) -> Self {
        Self::Url(link)
    }

    fn from_github(link: GithubLink) -> Self {
        Self::Github(link)
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::path::PathBuf;

    use tempfile::tempdir;

    use crate::addonlist::GithubLink;

    use super::AddonKey;
    use super::Addons;
    use super::CLIENT;

    use super::FolderEntry;
    use super::LoadOrder;
    use super::ModdbLink;
    use super::UrlLink;

    #[tokio::test]
    async fn moddb_link() {
        let key = ModdbLink {
            addon_link: "anomaly-mod-configuration-menu".to_owned(),
            updated: "Aug 8th, 2022".to_owned(),
        };
        let url = key.get_download_url().await.unwrap();

        let response = CLIENT.head(url).send().await.unwrap();
        assert!(response.content_length().is_some()); // just check if it has size
    }

    #[test]
    fn find_addons() {
        let paths = [
            r"D:\Tasks\Weird_Tasks_Framework",
            r"D:\Tasks\Weird_Tasks_Framework\gamedata",
            r"D:\Tasks\BaseGame_TP\gamedata",
            r"D:\Tasks\BaseGame_TP\gamedata\abc",
            r"D:\Tasks\BaseGame_TP",
            r"D:\Tasks\GhenTuong_TP\gamedata",
            r"D:\Tasks\GhenTuong_TP",
            r"D:\Tasks\GhenTuong_TP\gamedata",
            r"D:\Tasks\GhenTuong_TP\gamedata\abs",
            r"D:\Tasks\GhenTuong_TP\gamedata\abs\bca",
        ]
        .iter()
        .map(Path::new);

        let expected = [
            r"D:\Tasks\Weird_Tasks_Framework",
            r"D:\Tasks\BaseGame_TP",
            r"D:\Tasks\GhenTuong_TP",
        ]
        .map(PathBuf::from)
        .into_iter()
        .collect();

        assert_eq!(Addons::find_addons(paths), expected)
    }

    #[test]
    fn addons_with_entries() {
        let paths = [
            r"D:\Tasks",
            r"D:\Tasks\Weird_Tasks_Framework",
            r"D:\Tasks\BaseGame_TP",
            r"D:\Tasks\GhenTuong_TP",
        ]
        .map(Path::new)
        .into_iter();

        let key = AddonKey::Url(UrlLink { url: "".to_owned() });

        let expected = [
            (
                Path::new(r"D:\Tasks\Weird_Tasks_Framework"),
                FolderEntry::new(key.clone(), Some("Weird_Tasks_Framework".to_owned())),
            ),
            (
                Path::new(r"D:\Tasks\BaseGame_TP"),
                FolderEntry::new(key.clone(), Some("BaseGame_TP".to_owned())),
            ),
            (
                Path::new(r"D:\Tasks\GhenTuong_TP"),
                FolderEntry::new(key.clone(), Some("GhenTuong_TP".to_owned())),
            ),
            (Path::new(r"D:\Tasks"), FolderEntry::new(key.clone(), None)),
        ]
        .into_iter()
        .collect();

        assert_eq!(
            Addons::with_entries(paths, &key, Path::new(r"D:\Tasks")),
            expected
        );
    }

    #[tokio::test]
    async fn github_download_link_tagged() {
        let key = GithubLink {
            repo: "ModOrganizer2/modorganizer".to_owned(),
            tag: "v2.4.3".to_owned(),
            filename: "Mod.Organizer-$VERSION.7z".to_owned(),
        };

        let expected = "https://github.com/ModOrganizer2/modorganizer/releases/download/v2.4.3/Mod.Organizer-2.4.3.7z";
        assert_eq!(key.get_download_url().await.unwrap(), expected);
    }

    #[tokio::test]
    async fn github_download_link_latest() {
        let key = GithubLink {
            repo: "ModOrganizer2/modorganizer".to_owned(),
            tag: "latest".to_owned(),
            filename: "Mod.Organizer-$VERSION.7z".to_owned(),
        };

        let not_expected = "https://github.com/ModOrganizer2/modorganizer/releases/download/latest/Mod.Organizer-latest.7z";
        let url = key.get_download_url().await.unwrap();
        assert_ne!(url, not_expected);
        assert!(CLIENT
            .head(url)
            .send()
            .await
            .unwrap()
            .content_length()
            .is_some());
    }

    #[test]
    fn modorg_modlist() {
        let entry = FolderEntry::new(AddonKey::from_url(UrlLink { url: "".to_owned() }), None);
        let mut addons = Addons::new();
        addons.insert("community-task-pack".to_owned(), entry.clone());
        addons.insert("BaseGame_Task_Pack".to_owned(), entry.clone());
        addons.insert("GhenTuong_Task_Pack".to_owned(), entry.clone());
        addons.insert("Arszi_Task_Pack".to_owned(), entry.clone());
        addons.insert("Weird_Tasks_Framework".to_owned(), entry.clone());
        addons.insert("Anomaly-Mod-Configuration-Menu".to_owned(), entry.clone());
        addons.insert("Interactive_PDA".to_owned(), entry.clone());
        addons.insert("Igigui".to_owned(), entry);

        let mut modlist = LoadOrder::new();
        modlist.push("community-task-pack".to_owned());
        modlist.push("BaseGame_Task_Pack".to_owned());
        modlist.push("GhenTuong_Task_Pack".to_owned());
        modlist.push("Arszi_Task_Pack".to_owned());
        modlist.push("Weird_Tasks_Framework".to_owned());
        modlist.push("Anomaly-Mod-Configuration-Menu".to_owned());

        let prefix = [
            "# This file was automatically generated by Anomaly Modding Tool. Sorry if it broke lol.\n",
            "+community-task-pack\n",
            "+BaseGame_Task_Pack\n",
            "+GhenTuong_Task_Pack\n",
            "+Arszi_Task_Pack\n",
            "+Weird_Tasks_Framework\n",
            "+Anomaly-Mod-Configuration-Menu\n",
        ].join("");

        let repr = modlist.to_modorg_modlist(&addons);

        assert!(repr.starts_with(&prefix));
        assert!(repr.lines().count() == addons.0.len() + 1);
        for k in addons.0.keys().filter(|k| !modlist.0.contains(k)) {
            assert!(repr.contains(&("-".to_owned() + k)));
            assert!(!repr.contains(&("+".to_owned() + k)))
        }
    }

    #[test]
    fn missing_addons() {
        let dir = tempdir().unwrap();
        let mods_path = dir.path().join("mods");
        std::fs::create_dir(&mods_path).unwrap();

        let mut addons = Addons::new();
        let entry = FolderEntry::new(AddonKey::from_url(UrlLink { url: "".to_owned() }), None);

        let addons_found = [
            "community-task-pack".to_owned(),
            "BaseGame_Task_Pack".to_owned(),
            "GhenTuong_Task_Pack".to_owned(),
        ];

        for addon in addons_found {
            std::fs::create_dir(mods_path.join(&addon)).unwrap();
            addons.insert(addon, entry.clone());
        }

        let addons_missing = ["Igigui".to_owned(), "Arszi_Task_Pack".to_owned()];

        for addon in &addons_missing {
            addons.insert(addon.clone(), entry.clone());
        }

        let missing = addons.missing_addons(dir.path());
        assert_eq!(missing.len(), addons_missing.len());
        for addon in &addons_missing {
            assert!(missing.contains(&addon.as_str()));
        }
    }
}
