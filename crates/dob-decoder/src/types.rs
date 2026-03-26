use serde::{Deserialize, Serialize};
use serde_json::Value;

use std::collections::BTreeMap;

/// Matches the `StandardDOBOutput` format from dob-decoder-standalone-server.
///
/// Each group has a name and a list of trait key-value pairs produced by the
/// decoder binary.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DobTraitGroup {
    pub name: String,
    pub traits: Vec<DobTraitValue>,
}

/// A single trait entry output by a decoder binary.
///
/// Uses `BTreeMap` with `#[serde(flatten)]` so iteration order is
/// deterministic (alphabetical by key). The map typically contains exactly
/// one entry where the key is the trait type tag and the value is the
/// display value.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DobTraitValue {
    #[serde(flatten)]
    pub inner: BTreeMap<String, Value>,
}

impl DobTraitValue {
    /// Returns the display value (first map value) or `null` if empty.
    pub fn display_value(&self) -> &Value {
        self.inner.values().next().unwrap_or(&Value::Null)
    }

    /// Returns the type tag (first map key) or empty string if empty.
    pub fn type_tag(&self) -> &str {
        self.inner.keys().next().map(String::as_str).unwrap_or("")
    }
}

/// A flattened name+value pair suitable for storage and API responses.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DobTrait {
    pub name: String,
    pub value: Value,
    pub type_tag: String,
}

/// One decoder step's output.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct StepOutput {
    /// 0-indexed position in the decoder chain.
    pub step: u32,
    /// The raw output bytes (verbatim from decoder).
    pub raw_output: String,
    /// Traits parsed from this step's output, if it was valid `DobTraitGroup[]`
    /// JSON. Empty otherwise.
    pub traits: Vec<DobTrait>,
}

/// Result of decoding a DOB's DNA through one or more decoder binaries.
///
/// Each decoder in the chain produces a [`StepOutput`]. The raw output of each
/// step is preserved as-is — the caller decides what to store and how to
/// interpret the results.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DobDecodedResult {
    pub step_outputs: Vec<StepOutput>,
}

/// Reference to a decoder binary on-chain, identified either by code_hash
/// or type_id.
#[derive(Debug, Clone)]
pub enum DecoderRef {
    CodeHash(Vec<u8>),
    TypeId(Vec<u8>),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dob_trait_value_single_entry() {
        let json = r#"{"String": "Blue"}"#;
        let tv: DobTraitValue = serde_json::from_str(json).unwrap();
        assert_eq!(tv.type_tag(), "String");
        assert_eq!(tv.display_value(), &Value::String("Blue".to_string()));
    }

    #[test]
    fn test_dob_trait_value_empty() {
        let json = r#"{}"#;
        let tv: DobTraitValue = serde_json::from_str(json).unwrap();
        assert_eq!(tv.type_tag(), "");
        assert_eq!(tv.display_value(), &Value::Null);
    }

    #[test]
    fn test_dob_trait_group_roundtrip() {
        let json = r#"{"name":"Background","traits":[{"String":"Blue"},{"Number":42}]}"#;
        let group: DobTraitGroup = serde_json::from_str(json).unwrap();
        assert_eq!(group.name, "Background");
        assert_eq!(group.traits.len(), 2);
        assert_eq!(group.traits[0].type_tag(), "String");

        // Roundtrip
        let serialized = serde_json::to_string(&group).unwrap();
        let reparsed: DobTraitGroup = serde_json::from_str(&serialized).unwrap();
        assert_eq!(reparsed.name, "Background");
        assert_eq!(reparsed.traits.len(), 2);
    }

    #[test]
    fn test_dob_decoded_result_camel_case() {
        let result = DobDecodedResult {
            step_outputs: vec![StepOutput {
                step: 0,
                raw_output: r#"[{"name":"test","traits":[]}]"#.to_string(),
                traits: vec![DobTrait {
                    name: "Color".to_string(),
                    value: Value::String("Red".to_string()),
                    type_tag: "String".to_string(),
                }],
            }],
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("\"stepOutputs\""));
        assert!(json.contains("\"rawOutput\""));
        assert!(json.contains("\"typeTag\""));
    }
}
