use serde::{Deserialize, Serialize};

/// Configuration for label import.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LabelImportConfig {
    #[serde(default = "default_token_labels_path")]
    pub token_labels_path: String,
    #[serde(default = "default_network")]
    pub network: String,
    #[serde(default = "default_true")]
    pub import_udt: bool,
    #[serde(default = "default_true")]
    pub import_scripts: bool,
}

impl Default for LabelImportConfig {
    fn default() -> Self {
        Self {
            token_labels_path: default_token_labels_path(),
            network: default_network(),
            import_udt: true,
            import_scripts: true,
        }
    }
}

fn default_token_labels_path() -> String {
    "docs/token-labels".to_string()
}

fn default_network() -> String {
    "mainnet".to_string()
}

fn default_true() -> bool {
    true
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
        assert_eq!(cfg.token_labels_path, "docs/token-labels");
        assert_eq!(cfg.network, "mainnet");
        assert!(cfg.import_udt);
        assert!(cfg.import_scripts);
    }
}
