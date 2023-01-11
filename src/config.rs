use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;


#[derive(Debug, Serialize, Deserialize)]
pub struct ModpackConfig {
    metadata: Metadata,
    pub mods: IndexMap<String, FolderEntry>
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Metadata {
    config_version: u8,
    name: String,
}

#[skip_serializing_none]
#[derive(Debug, Serialize, Deserialize)]
pub struct FolderEntry {
    pub moddb: Option<ModdbLink>,
    pub github: Option<GithubLink>,
    pub url: Option<UrlLink>,
    pub renamed_from: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Hash)]
pub struct ModdbLink {
    pub url: String,
    pub updated: String,
}

#[derive(Debug, Serialize, Deserialize, Hash)]
pub struct GithubLink {
    pub url: String,
    pub tag: String,
    pub filename: String,
}

#[derive(Debug, Serialize, Deserialize, Hash)]
pub struct UrlLink {
    url: String,
}
