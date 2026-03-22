pub mod cache;
pub mod fetch;
pub mod types;
pub mod vm;

use anyhow::{bail, Context};

use crate::types::{DobDecodedResult, DobTrait, DobTraitGroup};
use crate::vm::execute_riscv_binary;

/// Decode a DOB0 spore by running a single decoder binary.
///
/// The decoder binary receives `argv = [dna_hex, pattern_json]` and emits
/// a JSON array of `DobTraitGroup` via debug syscall 2177.
pub fn decode_dob0(
    decoder_binary: &[u8],
    dna_hex: &str,
    pattern_json: &str,
) -> anyhow::Result<DobDecodedResult> {
    let (exit_code, output) = execute_riscv_binary(decoder_binary, &[dna_hex, pattern_json])?;

    if exit_code != 0 {
        bail!("decoder exited with non-zero code: {exit_code}");
    }

    let raw_output = output.last().cloned().unwrap_or_default();

    let groups: Vec<DobTraitGroup> = serde_json::from_str(&raw_output)
        .context("failed to parse decoder output as DobTraitGroup array")?;

    let traits = flatten_trait_groups(&groups);

    Ok(DobDecodedResult { traits, raw_output })
}

/// Decode a DOB1 spore by running a chain of decoders.
///
/// Each entry in `decoders` is `(binary, pattern_json)`. The first decoder
/// receives the original `dna_hex`; subsequent decoders receive the raw
/// output of the previous decoder as their DNA input.
pub fn decode_dob1_chain(
    decoders: &[(&[u8], &str)],
    dna_hex: &str,
) -> anyhow::Result<DobDecodedResult> {
    if decoders.is_empty() {
        bail!("decoder chain is empty");
    }

    let mut current_dna = dna_hex.to_string();
    let mut last_raw_output = String::new();

    for (i, (binary, pattern_json)) in decoders.iter().enumerate() {
        let (exit_code, output) = execute_riscv_binary(binary, &[&current_dna, pattern_json])?;

        if exit_code != 0 {
            bail!("decoder {i} exited with non-zero code: {exit_code}");
        }

        last_raw_output = output.last().cloned().unwrap_or_default();

        // For chained decoders, the raw output becomes the next decoder's DNA
        // input (hex-encoded).
        current_dna = hex::encode(last_raw_output.as_bytes());
    }

    let groups: Vec<DobTraitGroup> = serde_json::from_str(&last_raw_output)
        .context("failed to parse final decoder output as DobTraitGroup array")?;

    let traits = flatten_trait_groups(&groups);

    Ok(DobDecodedResult {
        traits,
        raw_output: last_raw_output,
    })
}

/// Flatten trait groups into a flat list of `DobTrait` for storage/API use.
fn flatten_trait_groups(groups: &[DobTraitGroup]) -> Vec<DobTrait> {
    let mut traits = Vec::new();
    for group in groups {
        for tv in &group.traits {
            traits.push(DobTrait {
                name: group.name.clone(),
                value: tv.display_value().clone(),
                type_tag: tv.type_tag().to_string(),
            });
        }
    }
    traits
}

#[cfg(test)]
mod tests {
    use super::*;

    use serde_json::Value;

    #[test]
    fn test_flatten_trait_groups() {
        let json = r#"[
            {
                "name": "Background",
                "traits": [
                    {"String": "Blue"},
                    {"Number": 42}
                ]
            },
            {
                "name": "Eyes",
                "traits": [
                    {"String": "Green"}
                ]
            }
        ]"#;

        let groups: Vec<DobTraitGroup> = serde_json::from_str(json).unwrap();
        let traits = flatten_trait_groups(&groups);

        assert_eq!(traits.len(), 3);

        assert_eq!(traits[0].name, "Background");
        assert_eq!(traits[0].type_tag, "String");
        assert_eq!(traits[0].value, Value::String("Blue".to_string()));

        assert_eq!(traits[1].name, "Background");
        assert_eq!(traits[1].type_tag, "Number");
        assert_eq!(traits[1].value, Value::Number(42.into()));

        assert_eq!(traits[2].name, "Eyes");
        assert_eq!(traits[2].type_tag, "String");
        assert_eq!(traits[2].value, Value::String("Green".to_string()));
    }

    #[test]
    fn test_flatten_empty_groups() {
        let traits = flatten_trait_groups(&[]);
        assert!(traits.is_empty());
    }

    #[test]
    fn test_decode_dob1_chain_empty_decoders() {
        let result = decode_dob1_chain(&[], "deadbeef");
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("decoder chain is empty"),);
    }
}
