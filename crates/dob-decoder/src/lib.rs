pub mod cache;
pub mod fetch;
pub mod types;
pub mod vm;

use anyhow::bail;

use crate::types::{DobDecodedResult, DobTrait, DobTraitGroup, StepOutput};
use crate::vm::execute_riscv_binary;

/// Decode a DOB0 spore by running a single decoder binary.
///
/// The decoder binary receives `argv = [dna_hex, pattern_json]` and emits
/// output via debug syscall 2177. Output is typically a JSON array of
/// `DobTraitGroup`, but decoders may return any text format (SVG, HTML, …).
pub fn decode_dob0(
    decoder_binary: &[u8],
    dna_hex: &str,
    pattern_json: &str,
) -> anyhow::Result<DobDecodedResult> {
    let (exit_code, output) = execute_riscv_binary(decoder_binary, &[dna_hex, pattern_json])?;

    if exit_code != 0 {
        bail!("decoder exited with non-zero code: {exit_code}");
    }

    let raw_output = output
        .last()
        .cloned()
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            anyhow::anyhow!("DOB0 decoder exited successfully but produced no output")
        })?;

    let traits = match serde_json::from_str::<Vec<DobTraitGroup>>(&raw_output) {
        Ok(groups) => flatten_trait_groups(&groups),
        Err(_) => Vec::new(),
    };

    Ok(DobDecodedResult {
        step_outputs: vec![StepOutput {
            step: 0,
            raw_output,
            traits,
        }],
    })
}

/// Decode a DOB1 spore by running a chain of decoders.
///
/// Each entry in `decoders` is `(binary, pattern_json)`. The first decoder
/// receives `argv = [dna_hex, pattern]`. Subsequent decoders receive
/// `argv = [dna_hex, pattern, previous_output_json]` — the original DNA
/// is always passed as the first argument, and the previous step's output
/// is appended as the third argument.
///
/// Each decoder's raw output is preserved independently in the result.
/// Trait parsing is attempted per step — steps whose output is valid
/// `DobTraitGroup[]` JSON will have their parsed traits recorded.
pub fn decode_dob1_chain(
    decoders: &[(&[u8], &str)],
    dna_hex: &str,
) -> anyhow::Result<DobDecodedResult> {
    if decoders.is_empty() {
        bail!("decoder chain is empty");
    }

    let mut step_outputs = Vec::with_capacity(decoders.len());
    let mut previous_output: Option<String> = None;

    for (i, (binary, pattern_json)) in decoders.iter().enumerate() {
        let (exit_code, output) = if let Some(prev) = &previous_output {
            execute_riscv_binary(binary, &[dna_hex, pattern_json, prev])?
        } else {
            execute_riscv_binary(binary, &[dna_hex, pattern_json])?
        };

        if exit_code != 0 {
            bail!("decoder {i} exited with non-zero code: {exit_code}");
        }

        let raw_output = output
            .last()
            .cloned()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| anyhow::anyhow!("decoder {i} produced no output"))?;

        let traits = match serde_json::from_str::<Vec<DobTraitGroup>>(&raw_output) {
            Ok(groups) => flatten_trait_groups(&groups),
            Err(_) => Vec::new(),
        };

        previous_output = Some(raw_output.clone());
        step_outputs.push(StepOutput {
            step: i as u32,
            raw_output,
            traits,
        });
    }

    Ok(DobDecodedResult { step_outputs })
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
