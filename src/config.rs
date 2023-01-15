use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;

use crate::addonlist::FolderEntry;


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

#[cfg(test)]
mod tests {
    use crate::config::ModpackConfig;

    static TEST_CONFIG: &str = include_str!("../resources/config.json");

    #[test]
    fn serialization() {
        let config: ModpackConfig = serde_json::from_str(TEST_CONFIG).unwrap();
        assert_eq!(serde_json::to_string_pretty(&config).unwrap(), TEST_CONFIG.replace("\r\n", "\n"));
    }
}
