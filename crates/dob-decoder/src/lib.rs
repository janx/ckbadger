pub mod cache;
pub mod fetch;
pub mod types;
pub mod vm;

use anyhow::bail;

use crate::types::{DobDecodedResult, DobTrait, DobTraitGroup};
use crate::vm::execute_riscv_binary;

/// Decode a DOB0 spore by running a single decoder binary.
///
/// The decoder binary receives `argv = [dna_hex, pattern_json]` and emits
/// output via debug syscall 2177. Output is typically a JSON array of
/// `DobTraitGroup`, but decoders may return any text format (SVG, HTML, …).
/// When the output is not valid JSON traits, `traits` will be empty and
/// `raw_output` still holds the verbatim decoder output for media storage.
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
        traits,
        raw_output,
        output_step: 0,
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
/// Traits are extracted from the latest step whose output is valid JSON
/// `DobTraitGroup[]`. The final step's raw output is returned as-is — it
/// may be rendered media (SVG, PNG, …) rather than JSON when the chain
/// ends with a renderer decoder.
pub fn decode_dob1_chain(
    decoders: &[(&[u8], &str)],
    dna_hex: &str,
) -> anyhow::Result<DobDecodedResult> {
    if decoders.is_empty() {
        bail!("decoder chain is empty");
    }

    let mut previous_output: Option<String> = None;
    let mut last_raw_output = String::new();
    let mut traits: Vec<DobTrait> = Vec::new();

    for (i, (binary, pattern_json)) in decoders.iter().enumerate() {
        let (exit_code, output) = if let Some(prev) = &previous_output {
            // Step 1+: pass original DNA, pattern, and previous step output
            execute_riscv_binary(binary, &[dna_hex, pattern_json, prev])?
        } else {
            // Step 0: pass DNA and pattern only
            execute_riscv_binary(binary, &[dna_hex, pattern_json])?
        };

        if exit_code != 0 {
            bail!("decoder {i} exited with non-zero code: {exit_code}");
        }

        last_raw_output = output
            .last()
            .cloned()
            .filter(|s| !s.is_empty())
            .ok_or_else(|| anyhow::anyhow!("decoder {i} produced no output"))?;

        // Capture traits from each step that produces valid JSON trait groups.
        // For chains ending with a renderer (SVG output), traits come from an
        // earlier step; for chains where every step outputs traits, the latest
        // step's traits win.
        if let Ok(groups) = serde_json::from_str::<Vec<DobTraitGroup>>(&last_raw_output) {
            traits = flatten_trait_groups(&groups);
        }

        previous_output = Some(last_raw_output.clone());
    }

    Ok(DobDecodedResult {
        traits,
        raw_output: last_raw_output,
        output_step: (decoders.len() - 1) as u32,
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
