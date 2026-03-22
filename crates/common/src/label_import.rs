use serde::{Deserialize, Serialize};

/// Configuration for label import.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LabelImportConfig {
    /// Path to workdir metadata override directory. None = use bundled data only.
    #[serde(default)]
    pub metadata_path: Option<String>,
    #[serde(default = "default_network")]
    pub network: String,
}

impl Default for LabelImportConfig {
    fn default() -> Self {
        Self {
            metadata_path: None,
            network: default_network(),
        }
    }
}

fn default_network() -> String {
    "mainnet".to_string()
}

/// Label import summary result.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct LabelImportResult {
    pub udt_labels_imported: i64,
    pub script_labels_imported: i64,
    pub errors: Vec<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_label_import_config() {
        let cfg = LabelImportConfig::default();
        assert!(cfg.metadata_path.is_none());
        assert_eq!(cfg.network, "mainnet");
    }
}
