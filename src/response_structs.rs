use serde::Deserialize;


#[derive(Deserialize)]
pub struct ModOrgAsset {
    pub browser_download_url: String,
}

#[derive(Deserialize)]
pub struct ModOrgResponse {
    pub assets: Vec<ModOrgAsset>,
}

#[derive(Deserialize)]
pub struct ModdedExesFile {
    pub name: String,
    pub download_url: Option<String>,
}
