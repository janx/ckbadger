//! Direct evidence from the fixed mainnet decoder binary, independent of the
//! pattern helper. No node, network, wall clock or mutable Type ID is required.

use ckbadger_dob_decoder::{
    decode_dob0,
    pattern::{describe_pattern, map_dna, PatternDescription},
};
use serde_json::{json, Value};

const BINARY: &[u8] = include_bytes!("fixtures/nervape3x-98b7af5e.elf");
const HASH: &str = "98b7af5e071c0d4af030447b25bb436ee4e9dd687daac7bca4bee7a0393b78e1";
const DNA: &str = "a94617953341c95ac6d88b3fd6f5ed2a98bc76a5ecb70de14211d7d4a91879e0033358303132354e4d4e5641503033333230320000";

fn binary_hash() -> [u8; 32] {
    let actual = ckb_hash::blake2b_256(BINARY);
    assert_eq!(hex::encode(actual), HASH);
    actual
}

#[test]
fn fixed_mainnet_binary_reads_exact_declared_ranges_in_the_real_3x_pattern() {
    let hash = binary_hash();
    let pattern: Value =
        serde_json::from_str(include_str!("fixtures/nervape3x-pattern.json")).unwrap();
    let result = decode_dob0(BINARY, DNA, &pattern.to_string()).unwrap();
    let traits = &result.step_outputs[0].traits;
    assert_eq!(traits.len(), 6);
    assert_eq!(traits[0].name, "prev.type");
    assert_eq!(traits[0].value, "image");
    assert_eq!(traits[1].value, "NERVAPE 3X");
    assert_eq!(traits[2].value, format!("btcfs://{}i0", &DNA[..64]));
    assert_eq!(traits[4].value, "3X0125NMNVAP033202");

    let description = describe_pattern(&hash, &pattern).unwrap();
    assert!(matches!(description, PatternDescription::Supported { .. }));
    let mapped = map_dna(&description, &hex::decode(DNA).unwrap()).unwrap();
    assert_eq!(mapped[2].raw_hex, DNA[..64]);
    assert_eq!(mapped[3].raw_hex, "03");
    assert_eq!(mapped[4].byte_offset, 33);
    assert_eq!(mapped[4].byte_length, 18);

    // This fixture's actual code is the oracle for byte provenance. Reading each
    // rule's range alone must produce the same expressed value as that range in
    // the complete DNA, even for overlapping or nonzero-offset declarations.
    for (index, row) in pattern.as_array().unwrap().iter().enumerate() {
        let mut row = row.clone();
        row[2] = json!(0);
        let isolated =
            decode_dob0(BINARY, &mapped[index].raw_hex, &json!([row]).to_string()).unwrap();
        assert_eq!(
            isolated.step_outputs[0].traits[0].value,
            traits[index].value
        );
    }
}

#[test]
fn binary_range_oracle_covers_overlap_empty_and_truncated_declarations() {
    let hash = binary_hash();
    let dna = [0x61, 0x62, 0x63, 0x64];
    for offset in 0..=6 {
        let pattern = Value::Array(
            (0..=6)
                .map(|length| {
                    json!([
                        format!("range-{offset}-{length}"),
                        "String",
                        offset,
                        length,
                        "rawString"
                    ])
                })
                .collect(),
        );
        let result = decode_dob0(BINARY, &hex::encode(dna), &pattern.to_string()).unwrap();
        let description = describe_pattern(&hash, &pattern).unwrap();
        let mapped = map_dna(&description, &dna).unwrap();
        assert_eq!(mapped.len(), 7);
        for (index, gene) in mapped.iter().enumerate() {
            assert_eq!(result.step_outputs[0].traits[index].value, gene.raw_hex);
            assert_eq!(
                gene.truncated,
                offset + index > dna.len() || offset > dna.len()
            );
        }
    }
}

#[test]
fn binary_uses_the_same_range_for_each_supported_rule() {
    let hash = binary_hash();
    let rules = [
        "rawNumber",
        "rawString",
        "utf8",
        "options",
        "range",
        "btcfs",
        "btcfs2",
        "ckbfs",
        "nervapeColor",
        "nervapeNote",
        "nervapeSeries",
        "nervapeString",
        "nervapeInvolved",
        "nervapeDescription",
        "nervapeJob",
        "nervapeExternal",
        "nervapeOrigin",
        "nervapeParts",
        "nervapeItemType",
        "nervapeEvent",
        "nervapeItem",
        "nervapeSpecial",
        "nervapeCharacter",
        "nervapeScene",
        "nervapeStory",
    ];
    for rule in rules {
        let mut row = json!([rule, "String", 1, 1, rule]);
        if rule == "options" {
            row.as_array_mut().unwrap().push(json!(["one", "two"]));
        } else if rule == "range" {
            row.as_array_mut().unwrap().push(json!([10, 100]));
        }
        let pattern = json!([row.clone()]);
        let result = decode_dob0(BINARY, "aa01bb", &pattern.to_string())
            .unwrap_or_else(|e| panic!("{rule}: {e}"));
        let description = describe_pattern(&hash, &pattern).unwrap();
        let mapped = map_dna(&description, &[0xaa, 1, 0xbb]).unwrap();
        assert_eq!(mapped[0].raw_hex, "01");
        row[2] = json!(0);
        let isolated = decode_dob0(BINARY, "01", &json!([row]).to_string()).unwrap();
        assert_eq!(
            serde_json::to_value(&result.step_outputs[0].traits).unwrap(),
            serde_json::to_value(&isolated.step_outputs[0].traits).unwrap(),
            "{rule}"
        );
    }
}

#[test]
fn parsed_but_unimplemented_rules_do_not_claim_successful_vm_traits() {
    // Both rule names are in the official parser; the deployed decoder's match
    // reaches todo!(). A byte description is not a claim that decoding succeeds.
    for rule in ["nervapeSerial", "nervapeSerialNumber"] {
        let pattern = json!([[rule, "String", 0, 1, rule]]);
        let description = describe_pattern(&binary_hash(), &pattern).unwrap();
        assert_eq!(map_dna(&description, &[1]).unwrap()[0].raw_hex, "01");
        let error = decode_dob0(BINARY, "01", &pattern.to_string()).unwrap_err();
        assert!(error.to_string().contains("non-zero code: 101"));
    }
}
