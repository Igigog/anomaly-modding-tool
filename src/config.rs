use std::path::Path;

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use crate::{
    addonlist::{Addons, FolderEntry, Modpack},
    app::AppContext,
};

#[derive(Debug, Serialize, Deserialize)]
pub struct ModpackConfig {
    metadata: Metadata,
    pub mods: IndexMap<String, FolderEntry>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Metadata {
    config_version: u8,
    name: String,
}

#[derive(Serialize, Deserialize)]
enum AddonEntry {
    Modpack(String),
    Addon(String),
}

#[derive(Serialize, Deserialize)]
pub struct Profile {
    name: String,
    load_order: Vec<AddonEntry>,
}

impl Default for Profile {
    fn default() -> Self {
        Self {
            name: "Default".to_owned(),
            load_order: Vec::new(),
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct InstanceConfigData {
    mo_dir: String,
    current_profile: String,
    addons: Addons,
    profiles: Vec<Profile>,
}

impl InstanceConfigData {
    pub fn new() -> Self {
        Self {
            addons: Addons::default(),
            profiles: vec![Profile {
                name: "Default".to_owned(),
                load_order: Vec::new(),
            }],
            mo_dir: "mo2".to_owned(),
            current_profile: "Default".to_owned(),
        }
    }

    pub fn mo_dir(&self) -> &Path {
        Path::new(&self.mo_dir)
    }

    pub fn missing_addons(&self) -> Vec<&str> {
        let mods_dir = self.mo_dir().join("mods");
        self.addons
            .iter()
            .map(|(k, _)| k)
            .filter(|m| !mods_dir.join(m).is_dir())
            .map(|m| m.as_str())
            .collect()
    }

    pub fn missing_modpack_addons<'a>(&self, modpack: &'a Modpack) -> Vec<&'a str> {
        let mut missing = Vec::new();
        for (name, addon) in modpack.addons() {
            if !self.addons.has(addon) {
                missing.push(name);
            }
        }
        missing
    }

    pub fn unknown_addons(&self) -> Vec<String> {
        let mut unknown = Vec::new();
        for dir in std::fs::read_dir(self.mo_dir().join("mods")).unwrap() {
            let name = dir.unwrap().file_name();
            let s = name.to_str().unwrap();
            if self.addons.get(s).is_none() {
                unknown.push(s.to_owned());
            }
        }
        unknown
    }
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use crate::{
        addonlist::{AddonKey, Addons, FolderEntry, UrlLink},
        config::ModpackConfig,
    };

    use super::{InstanceConfigData, Profile};

    static TEST_CONFIG: &str = include_str!("../resources/config.json");

    #[test]
    fn serialization() {
        let config: ModpackConfig = serde_json::from_str(TEST_CONFIG).unwrap();
        assert_eq!(
            serde_json::to_string_pretty(&config).unwrap(),
            TEST_CONFIG.replace("\r\n", "\n")
        );
    }

    #[test]
    fn unknown_addons() {
        let tmp = tempdir().unwrap();
        let mods_dir = tmp.path().join("mods");
        std::fs::create_dir_all(mods_dir.join("ara")).unwrap();
        std::fs::create_dir_all(mods_dir.join("abb")).unwrap();
        std::fs::create_dir_all(mods_dir.join("bba")).unwrap();
        std::fs::create_dir_all(mods_dir.join("hehe/haa/hbb")).unwrap();

        let entry = FolderEntry::new(AddonKey::Url(UrlLink::new("".to_owned())), None);
        let mut addons = Addons::default();
        addons.insert("ara".to_owned(), entry.clone());
        addons.insert("bba".to_owned(), entry.clone());

        let config = InstanceConfigData {
            mo_dir: tmp.path().to_str().unwrap().to_owned(),
            current_profile: "Default".to_owned(),
            addons,
            profiles: vec![Profile::default()],
        };

        let expected = vec!["abb", "hehe"];
        let missing = config.unknown_addons();

        assert_eq!(missing.len(), expected.len());
        for s in expected {
            assert!(missing.contains(&s.to_owned()));
        }
    }
    #[test]
    fn missing_addons() {
        let tmp = tempdir().unwrap();
        let mods_dir = tmp.path().join("mods");
        std::fs::create_dir_all(mods_dir.join("ara")).unwrap();
        std::fs::create_dir_all(mods_dir.join("bba")).unwrap();

        let entry = FolderEntry::new(AddonKey::Url(UrlLink::new("".to_owned())), None);
        let mut addons = Addons::default();
        addons.insert("ara".to_owned(), entry.clone());
        addons.insert("abb".to_owned(), entry.clone());
        addons.insert("bba".to_owned(), entry.clone());
        addons.insert("hehe".to_owned(), entry.clone());

        let config = InstanceConfigData {
            mo_dir: tmp.path().to_str().unwrap().to_owned(),
            current_profile: "Default".to_owned(),
            addons,
            profiles: vec![Profile::default()],
        };

        let expected = vec!["abb", "hehe"];
        let missing = config.missing_addons();

        assert_eq!(missing.len(), expected.len());
        for s in expected {
            assert!(missing.contains(&s));
        }
    }
}
