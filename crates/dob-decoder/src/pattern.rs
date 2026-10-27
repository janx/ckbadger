//! Byte provenance for reviewed DOB/0 decoder binaries.
//!
//! This module describes the pattern, never computes expressed traits. The VM
//! remains their only calculation path. A Type ID must be resolved to the actual
//! decoder binary's CKB data hash before calling `describe_pattern`.
//!
//! Sources and deployment hashes: sporeprotocol/spore-dob-0 (legacy schema at
//! 5648032, JSON schema at a592680c) and nervape/nervape-dob-decoder (68a43dc5).

use anyhow::{anyhow, bail, ensure, Context, Result};
use serde::Serialize;
use serde_json::{json, Value};

const LEGACY: &str = "b82abd59ade361a014f0abb692f71b0feb880693c3ccb95b9137b73551d872ce";
const JSON_DOB0: &str = "13cac78ad8482202f18f9df4ea707611c35f994375fa03ae79121312dda9925c";
const NERVAPE: [&str; 3] = [
    "9c3d6729aac887b2501064ab50ad89a1a27cf27abf34f2ea4f2847302156a422",
    "279af749dddf2f3357ad9d84487ec64672e4d19088f78665b5fcf86821eceeba",
    // Mainnet 3X: fixed chain binary, directly verified in nervape_pattern_vm.
    "98b7af5e071c0d4af030447b25bb436ee4e9dd687daac7bca4bee7a0393b78e1",
];

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(
    tag = "status",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum PatternDescription {
    Supported {
        format: PatternFormat,
        range_policy: RangePolicy,
        genes: Vec<GeneDefinition>,
    },
    Unsupported {
        reason: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PatternFormat {
    MoleculeDob0,
    JsonDob0,
    NervapeJsonDob0,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum RangePolicy {
    Strict,
    ClipToDna,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GeneDefinition {
    pub index: usize,
    pub trait_index: usize,
    pub value_index: usize,
    pub name: String,
    pub type_tag: String,
    pub byte_offset: u64,
    pub byte_length: u64,
    pub rule: String,
    pub rule_arguments: Value,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GeneBytes {
    pub gene_index: usize,
    pub declared_byte_offset: u64,
    pub declared_byte_length: u64,
    pub byte_offset: usize,
    pub byte_length: usize,
    pub truncated: bool,
    pub raw_hex: String,
}

/// Describe only a known binary's pattern dialect, never guess from JSON shape.
pub fn describe_pattern(
    decoder_code_hash: &[u8; 32],
    pattern: &Value,
) -> Result<PatternDescription> {
    let hash = hex::encode(decoder_code_hash);
    let (format, range_policy, genes) = if hash == LEGACY {
        (
            PatternFormat::MoleculeDob0,
            RangePolicy::Strict,
            describe_molecule(pattern)?,
        )
    } else if hash == JSON_DOB0 || NERVAPE.contains(&hash.as_str()) {
        let nervape = hash != JSON_DOB0;
        (
            if nervape {
                PatternFormat::NervapeJsonDob0
            } else {
                PatternFormat::JsonDob0
            },
            RangePolicy::ClipToDna,
            describe_json(pattern, nervape)?,
        )
    } else {
        return Ok(PatternDescription::Unsupported {
            reason: format!("byte mapping is not defined for decoder binary 0x{hash}"),
        });
    };
    Ok(PatternDescription::Supported {
        format,
        range_policy,
        genes,
    })
}

/// Map literal DNA bytes; VM output must be used for the expressed value.
/// Modern decoders explicitly clip ranges to DNA length. Report both ranges so
/// a truncated declaration is never silently presented as a complete read.
pub fn map_dna(description: &PatternDescription, dna: &[u8]) -> Result<Vec<GeneBytes>> {
    let PatternDescription::Supported {
        range_policy,
        genes,
        ..
    } = description
    else {
        bail!("cannot map DNA for an unsupported decoder binary");
    };
    genes
        .iter()
        .map(|gene| {
            let declared_end = gene
                .byte_offset
                .checked_add(gene.byte_length)
                .ok_or_else(|| {
                    anyhow!("gene {} ({}) byte range overflows", gene.index, gene.name)
                })?;
            let (start, end) = match range_policy {
                RangePolicy::Strict => {
                    ensure!(
                        declared_end <= dna.len() as u64,
                        "gene {} ({}) needs DNA bytes {}..{}, only {} bytes available",
                        gene.index,
                        gene.name,
                        gene.byte_offset,
                        declared_end,
                        dna.len()
                    );
                    (gene.byte_offset as usize, declared_end as usize)
                }
                RangePolicy::ClipToDna => {
                    // This is the reviewed decoder's range semantics, not a
                    // repair of invalid data. The truncation is returned below.
                    let start = gene.byte_offset.min(dna.len() as u64) as usize;
                    let end = declared_end.min(dna.len() as u64) as usize;
                    (start, end)
                }
            };
            Ok(GeneBytes {
                gene_index: gene.index,
                declared_byte_offset: gene.byte_offset,
                declared_byte_length: gene.byte_length,
                byte_offset: start,
                byte_length: end - start,
                truncated: start as u64 != gene.byte_offset || end as u64 != declared_end,
                raw_hex: hex::encode(&dna[start..end]),
            })
        })
        .collect()
}

fn describe_json(pattern: &Value, nervape: bool) -> Result<Vec<GeneDefinition>> {
    let rows = pattern
        .as_array()
        .context("DOB JSON pattern must be an array")?;
    rows.iter()
        .enumerate()
        .map(|(index, row)| {
            let parse = || -> Result<GeneDefinition> {
                let fields = row.as_array().context("gene must be an array")?;
                ensure!(fields.len() >= 5, "gene requires at least five fields");
                let name = fields[0].as_str().context("name must be a string")?;
                let type_tag = fields[1].as_str().context("type must be a string")?;
                let byte_offset = fields[2].as_u64().context("offset must be a u64")?;
                let byte_length = fields[3].as_u64().context("length must be a u64")?;
                byte_offset
                    .checked_add(byte_length)
                    .context("byte range overflows u64")?;
                let rule = fields[4].as_str().context("rule must be a string")?;
                let standard_rule = matches!(
                    rule,
                    "options" | "range" | "rawNumber" | "rawString" | "utf8"
                );
                let nervape_rule = matches!(
                    rule,
                    "ckbfs"
                        | "btcfs"
                        | "btcfs2"
                        | "nervapeSerial"
                        | "nervapeSerialNumber"
                        | "nervapeColor"
                        | "nervapeNote"
                        | "nervapeSeries"
                        | "nervapeString"
                        | "nervapeInvolved"
                        | "nervapeDescription"
                        | "nervapeJob"
                        | "nervapeExternal"
                        | "nervapeOrigin"
                        | "nervapeParts"
                        | "nervapeItemType"
                        | "nervapeEvent"
                        | "nervapeItem"
                        | "nervapeSpecial"
                        | "nervapeCharacter"
                        | "nervapeScene"
                        | "nervapeStory"
                );
                ensure!(
                    standard_rule || (nervape && nervape_rule),
                    "unknown decoder rule {rule}"
                );
                let rule_arguments = match fields.get(5) {
                    Some(args) => {
                        ensure!(args.is_array(), "rule arguments must be an array");
                        args.clone()
                    }
                    None => Value::Null,
                };
                Ok(GeneDefinition {
                    index,
                    trait_index: index,
                    value_index: 0,
                    name: name.into(),
                    type_tag: type_tag.into(),
                    byte_offset,
                    byte_length,
                    rule: rule.into(),
                    rule_arguments,
                })
            };
            parse().with_context(|| format!("DOB JSON gene {index}"))
        })
        .collect()
}

fn describe_molecule(pattern: &Value) -> Result<Vec<GeneDefinition>> {
    let encoded = pattern
        .as_str()
        .context("legacy DOB pattern must be a hex string")?;
    let bytes = hex::decode(encoded).context("invalid legacy DOB pattern hex")?;
    let mut genes = Vec::new();
    let mut byte_offset = 0u64;
    for (trait_index, group) in dynamic_fields(&bytes)?.into_iter().enumerate() {
        let parse = || -> Result<Vec<GeneDefinition>> {
            let fields = table_fields(group, 2)?;
            let name = molecule_string(fields[0])?;
            let mut result = Vec::new();
            for (value_index, schema) in dynamic_fields(fields[1])?.into_iter().enumerate() {
                let schema = table_fields(schema, 2)?;
                ensure!(schema[0].len() == 1, "byte_length must be one byte");
                let byte_length = u64::from(schema[0][0]);
                ensure!(
                    matches!(byte_length, 1 | 2 | 4 | 8),
                    "invalid DNA segment length {byte_length}"
                );
                let (type_tag, rule, rule_arguments) = describe_pool(schema[1])?;
                let preceding: u64 = result.iter().map(|g: &GeneDefinition| g.byte_length).sum();
                result.push(GeneDefinition {
                    index: genes.len() + value_index,
                    trait_index,
                    value_index,
                    name: name.clone(),
                    type_tag: type_tag.into(),
                    byte_offset: byte_offset
                        .checked_add(preceding)
                        .context("DNA offset overflows")?,
                    byte_length,
                    rule: rule.into(),
                    rule_arguments,
                });
            }
            Ok(result)
        };
        let group_genes = parse().with_context(|| format!("legacy DOB trait {trait_index}"))?;
        for gene in &group_genes {
            byte_offset = byte_offset
                .checked_add(gene.byte_length)
                .context("DNA offset overflows")?;
        }
        genes.extend(group_genes);
    }
    Ok(genes)
}

fn describe_pool(bytes: &[u8]) -> Result<(&'static str, &'static str, Value)> {
    if bytes.is_empty() {
        return Ok(("Number", "rawNumber", Value::Null));
    }
    let tag = word(bytes)?;
    let data = &bytes[4..];
    match tag {
        0 => {
            let values = dynamic_fields(data)?
                .into_iter()
                .map(molecule_string)
                .collect::<Result<Vec<_>>>()?;
            Ok(("String", "options", json!(values)))
        }
        1 => Ok(("Number", "options", json!(number_vector(data)?))),
        2 => {
            let fields = table_fields(data, 2)?;
            Ok((
                "Float",
                "options",
                json!({"numerators": number_vector(fields[0])?, "denominator": number(fields[1])?}),
            ))
        }
        3 => Ok(("Number", "range", json!(number_range(data)?))),
        4 => {
            let fields = table_fields(data, 2)?;
            Ok((
                "Float",
                "range",
                json!({"numeratorRange": number_range(fields[0])?, "denominator": number(fields[1])?}),
            ))
        }
        _ => bail!("unknown legacy DOB trait pool union tag {tag}"),
    }
}

fn word(bytes: &[u8]) -> Result<usize> {
    let raw: [u8; 4] = bytes.get(..4).context("missing Molecule u32")?.try_into()?;
    Ok(u32::from_le_bytes(raw) as usize)
}

fn dynamic_fields(bytes: &[u8]) -> Result<Vec<&[u8]>> {
    ensure!(
        word(bytes)? == bytes.len(),
        "Molecule total size does not match payload length {}",
        bytes.len()
    );
    if bytes.len() == 4 {
        return Ok(Vec::new());
    }
    let first = word(&bytes[4..])?;
    ensure!(
        first >= 8 && first % 4 == 0 && first <= bytes.len(),
        "invalid Molecule first offset {first}"
    );
    let count = first / 4 - 1;
    let mut offsets = Vec::with_capacity(count + 1);
    for i in 0..count {
        let offset = word(&bytes[4 + i * 4..])?;
        ensure!(
            offset >= first && offset <= bytes.len(),
            "Molecule offset {i} is out of bounds: {offset}"
        );
        if let Some(previous) = offsets.last() {
            ensure!(
                offset >= *previous,
                "Molecule offsets are not monotone at {i}"
            );
        }
        offsets.push(offset);
    }
    offsets.push(bytes.len());
    Ok(offsets
        .windows(2)
        .map(|pair| &bytes[pair[0]..pair[1]])
        .collect())
}

fn table_fields(bytes: &[u8], minimum: usize) -> Result<Vec<&[u8]>> {
    let fields = dynamic_fields(bytes)?;
    // The deployed legacy decoder uses from_compatible_slice, which permits
    // later table fields while preserving the known field prefix.
    ensure!(
        fields.len() >= minimum,
        "Molecule table needs {minimum} fields, got {}",
        fields.len()
    );
    Ok(fields)
}

fn fixed_items(bytes: &[u8], width: usize) -> Result<&[u8]> {
    let count = word(bytes)?;
    let length = count
        .checked_mul(width)
        .and_then(|n| n.checked_add(4))
        .context("Molecule fixed vector size overflows")?;
    ensure!(
        bytes.len() == length,
        "Molecule fixed vector count/width do not match {} bytes",
        bytes.len()
    );
    Ok(&bytes[4..])
}

fn molecule_string(bytes: &[u8]) -> Result<String> {
    Ok(std::str::from_utf8(fixed_items(bytes, 1)?)
        .context("Molecule string is not UTF-8")?
        .into())
}

fn number(bytes: &[u8]) -> Result<String> {
    let raw: [u8; 8] = bytes
        .try_into()
        .context("Molecule Number must be eight bytes")?;
    // Preserve exact integers in the JSON-facing rule description as well.
    Ok(u64::from_le_bytes(raw).to_string())
}

fn number_vector(bytes: &[u8]) -> Result<Vec<String>> {
    fixed_items(bytes, 8)?
        .as_chunks::<8>()
        .0
        .iter()
        .map(|item| number(item))
        .collect()
}

fn number_range(bytes: &[u8]) -> Result<[String; 2]> {
    ensure!(
        bytes.len() == 16,
        "Molecule NumberRange must be sixteen bytes"
    );
    Ok([number(&bytes[..8])?, number(&bytes[8..])?])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hash(value: &str) -> [u8; 32] {
        hex::decode(value).unwrap().try_into().unwrap()
    }

    fn genes(description: &PatternDescription) -> &[GeneDefinition] {
        match description {
            PatternDescription::Supported { genes, .. } => genes,
            other => panic!("expected supported pattern: {other:?}"),
        }
    }

    fn table(fields: &[Vec<u8>]) -> Vec<u8> {
        let header = 4 + fields.len() * 4;
        let size = header + fields.iter().map(Vec::len).sum::<usize>();
        let mut bytes = (size as u32).to_le_bytes().to_vec();
        let mut offset = header;
        for field in fields {
            bytes.extend_from_slice(&(offset as u32).to_le_bytes());
            offset += field.len();
        }
        for field in fields {
            bytes.extend_from_slice(field);
        }
        bytes
    }

    fn string(value: &str) -> Vec<u8> {
        let mut bytes = (value.len() as u32).to_le_bytes().to_vec();
        bytes.extend_from_slice(value.as_bytes());
        bytes
    }

    fn schema(length: u8, pool: Vec<u8>) -> Vec<u8> {
        table(&[vec![length], pool])
    }

    fn legacy(schemas: &[Vec<u8>]) -> Value {
        json!(hex::encode(table(&[table(&[
            string("Example"),
            table(schemas)
        ])])))
    }

    #[test]
    fn unknown_binary_is_unsupported_even_with_a_familiar_pattern() {
        let description =
            describe_pattern(&[7; 32], &json!([["Color", "String", 0, 1, "utf8"]])).unwrap();
        assert!(matches!(
            description,
            PatternDescription::Unsupported { .. }
        ));
        assert!(map_dna(&description, &[1])
            .unwrap_err()
            .to_string()
            .contains("unsupported"));
    }

    #[test]
    fn type_id_is_not_treated_as_decoder_code_hash() {
        let type_id = hash("f9e6e61b29ef7356cdacce217065bc1ec7ec95cf33f62d2c800b913a66688e3d");
        assert!(matches!(
            describe_pattern(&type_id, &json!([])).unwrap(),
            PatternDescription::Unsupported { .. }
        ));
    }

    #[test]
    fn nervape_json_keeps_overlapping_ranges_and_maps_exact_dna_bytes() {
        let pattern = json!([
            ["prev.type", "String", 0, 1, "options", ["image"]],
            ["relief", "String", 0, 1, "options", ["NERVAPE 3X"]],
            ["prev.bg", "String", 0, 32, "btcfs"],
            ["Color", "String", 32, 1, "nervapeColor"],
            ["Serial", "String", 33, 18, "utf8"],
            ["Note", "String", 51, 2, "nervapeNote"]
        ]);
        let dna = hex::decode("a94617953341c95ac6d88b3fd6f5ed2a98bc76a5ecb70de14211d7d4a91879e0033358303132354e4d4e5641503033333230320000").unwrap();
        for code_hash in NERVAPE {
            let description = describe_pattern(&hash(code_hash), &pattern).unwrap();
            let definitions = genes(&description);
            assert_eq!(definitions.len(), 6);
            assert_eq!(definitions[3].name, "Color");
            assert_eq!(definitions[5].byte_offset, 51);
            let mapped = map_dna(&description, &dna).unwrap();
            assert_eq!(mapped[0].raw_hex, mapped[1].raw_hex);
            assert_eq!(mapped[2].byte_length, 32);
            assert_eq!(mapped[3].raw_hex, "03");
            assert_eq!(
                hex::decode(&mapped[4].raw_hex).unwrap(),
                b"3X0125NMNVAP033202"
            );
            assert_eq!(mapped[5].raw_hex, "0000");
            assert!(mapped.iter().all(|gene| !gene.truncated));
        }
    }

    #[test]
    fn modern_clipping_preserves_both_declared_and_actual_ranges() {
        let description = describe_pattern(
            &hash(JSON_DOB0),
            &json!([
                ["suffix", "String", 2, 6, "rawString"],
                ["outside", "String", 9, 2, "rawString"],
                ["empty", "String", 3, 0, "rawString"]
            ]),
        )
        .unwrap();
        let mapped = map_dna(&description, &[0xaa, 0xbb, 0xcc]).unwrap();
        assert_eq!(
            mapped[0],
            GeneBytes {
                gene_index: 0,
                declared_byte_offset: 2,
                declared_byte_length: 6,
                byte_offset: 2,
                byte_length: 1,
                truncated: true,
                raw_hex: "cc".into(),
            }
        );
        assert_eq!(mapped[1].byte_offset, 3);
        assert_eq!(mapped[1].byte_length, 0);
        assert!(mapped[1].truncated);
        assert!(!mapped[2].truncated);
    }

    #[test]
    fn json_rejects_structural_errors_with_gene_context() {
        for pattern in [
            json!([["A", "String", 0, 1]]),
            json!([["A", "String", -1, 1, "utf8"]]),
            json!([["A", "String", 0, 1, "imaginary"]]),
            json!([["A", "String", 0, 1, "options", "blue"]]),
            json!([["A", "String", u64::MAX, 1, "utf8"]]),
        ] {
            let error = describe_pattern(&hash(JSON_DOB0), &pattern).unwrap_err();
            assert!(format!("{error:#}").contains("DOB JSON gene 0"));
        }
        assert!(describe_pattern(&hash(JSON_DOB0), &json!({})).is_err());
    }

    #[test]
    fn standard_decoder_does_not_accept_nervape_rules() {
        assert!(describe_pattern(
            &hash(JSON_DOB0),
            &json!([["Color", "String", 0, 1, "nervapeColor"]])
        )
        .is_err());
    }

    #[test]
    fn real_legacy_cluster_has_sequential_gene_ranges() {
        let pattern: Value = serde_json::from_str(include_str!(
            "../tests/fixtures/nervape-legacy-pattern.json"
        ))
        .unwrap();
        let description = describe_pattern(&hash(LEGACY), &pattern).unwrap();
        let definitions = genes(&description);
        assert_eq!(definitions.len(), 16);
        assert_eq!(definitions[0].name, "prev.type");
        assert_eq!(definitions[0].rule_arguments, json!(["image"]));
        for pair in definitions.windows(2) {
            assert_eq!(
                pair[0].byte_offset + pair[0].byte_length,
                pair[1].byte_offset
            );
        }
        let last = definitions.last().unwrap();
        let size = (last.byte_offset + last.byte_length) as usize;
        let dna: Vec<u8> = (0..size).map(|i| i as u8).collect();
        let mapped = map_dna(&description, &dna).unwrap();
        assert_eq!(
            mapped
                .iter()
                .map(|gene| gene.raw_hex.as_str())
                .collect::<String>(),
            hex::encode(dna)
        );
    }

    #[test]
    fn legacy_multiple_values_keep_group_indices_and_exact_integers() {
        let mut pool = 1u32.to_le_bytes().to_vec();
        pool.extend_from_slice(&1u32.to_le_bytes());
        pool.extend_from_slice(&u64::MAX.to_le_bytes());
        let description = describe_pattern(
            &hash(LEGACY),
            &legacy(&[schema(1, Vec::new()), schema(8, pool)]),
        )
        .unwrap();
        let definitions = genes(&description);
        assert_eq!(definitions[0].trait_index, 0);
        assert_eq!(definitions[1].trait_index, 0);
        assert_eq!(definitions[1].value_index, 1);
        assert_eq!(definitions[1].index, 1);
        assert_eq!(definitions[1].byte_offset, 1);
        assert_eq!(
            definitions[1].rule_arguments,
            json!(["18446744073709551615"])
        );
        let mapped = map_dna(&description, &[1, 2, 3, 4, 5, 6, 7, 8, 9]).unwrap();
        assert_eq!(mapped[1].raw_hex, "0203040506070809");
    }

    #[test]
    fn legacy_short_dna_is_an_error_not_a_clip() {
        let description =
            describe_pattern(&hash(LEGACY), &legacy(&[schema(2, Vec::new())])).unwrap();
        let error = map_dna(&description, &[1]).unwrap_err().to_string();
        assert!(error.contains("Example") && error.contains("0..2") && error.contains("only 1"));
    }

    #[test]
    fn legacy_float_and_range_arguments_remain_exact_without_computing_traits() {
        let range = [10u64.to_le_bytes(), 20u64.to_le_bytes()].concat();
        let mut numbers = 1u32.to_le_bytes().to_vec();
        numbers.extend_from_slice(&u64::MAX.to_le_bytes());
        for (tag, body, expected) in [
            (
                2u32,
                table(&[numbers, 3u64.to_le_bytes().to_vec()]),
                json!({"numerators":["18446744073709551615"],"denominator":"3"}),
            ),
            (3, range.clone(), json!(["10", "20"])),
            (
                4,
                table(&[range, 7u64.to_le_bytes().to_vec()]),
                json!({"numeratorRange":["10","20"],"denominator":"7"}),
            ),
        ] {
            let pool = [tag.to_le_bytes().to_vec(), body].concat();
            let description = describe_pattern(&hash(LEGACY), &legacy(&[schema(1, pool)])).unwrap();
            assert_eq!(genes(&description)[0].rule_arguments, expected);
        }
    }

    #[test]
    fn molecule_rejects_bad_sizes_offsets_strings_union_and_segments() {
        for bytes in [
            vec![],
            vec![4, 0, 0],
            vec![5, 0, 0, 0],
            vec![8, 0, 0, 0, 7, 0, 0, 0],
            vec![8, 0, 0, 0, 12, 0, 0, 0],
        ] {
            assert!(dynamic_fields(&bytes).is_err());
        }
        let mut descending = table(&[vec![1], vec![2]]);
        descending[8..12].copy_from_slice(&11u32.to_le_bytes());
        assert!(dynamic_fields(&descending).is_err());
        assert!(molecule_string(&[2, 0, 0, 0, b'a']).is_err());
        assert!(molecule_string(&[1, 0, 0, 0, 0xff]).is_err());
        assert!(describe_pattern(&hash(LEGACY), &legacy(&[schema(3, Vec::new())])).is_err());
        assert!(describe_pattern(
            &hash(LEGACY),
            &legacy(&[schema(1, 5u32.to_le_bytes().to_vec())])
        )
        .is_err());
    }

    #[test]
    fn compatible_legacy_tables_allow_extra_fields() {
        let group = table(&[
            string("Versioned"),
            table(&[table(&[vec![1], vec![], vec![42]])]),
            vec![43],
        ]);
        let description =
            describe_pattern(&hash(LEGACY), &json!(hex::encode(table(&[group])))).unwrap();
        assert_eq!(genes(&description)[0].name, "Versioned");
    }

    #[test]
    fn public_contract_uses_camel_case_and_does_not_fabricate_expressed_values() {
        let description = describe_pattern(
            &hash(JSON_DOB0),
            &json!([["Color", "String", 0, 1, "options", ["blue"]]]),
        )
        .unwrap();
        let value = serde_json::to_value(&description).unwrap();
        assert_eq!(value["status"], "supported");
        assert_eq!(value["rangePolicy"], "clipToDna");
        assert_eq!(value["genes"][0]["typeTag"], "String");
        assert_eq!(value["genes"][0]["ruleArguments"], json!(["blue"]));
        assert!(value["genes"][0].get("value").is_none());
        let mapped = serde_json::to_value(map_dna(&description, &[0]).unwrap()).unwrap();
        assert_eq!(mapped[0]["declaredByteLength"], 1);
        assert_eq!(mapped[0]["rawHex"], "00");
    }
}
