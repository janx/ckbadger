//! The bundled `docs/metadata/object-tiers.toml` is where an identity
//! collection's composition tier comes from: identity aggregates carry no
//! per-item tier counts, so the asset list and the collection pages show the
//! standard's override, and a standard without one is shown as "unknown".

use ckbadger_api::utils::resolve_object_collection_composition_tier_override;
use ckbadger_store::types::IdentityStandard;

/// Every identity standard, by exhaustive match: a new standard does not
/// compile until it is listed here, and then this test asks for its tier.
fn identity_standards() -> Vec<IdentityStandard> {
    let all = [
        IdentityStandard::DotBit,
        IdentityStandard::BitCell,
        IdentityStandard::DidCkb,
        IdentityStandard::DotCell,
    ];
    for standard in all {
        match standard {
            IdentityStandard::DotBit
            | IdentityStandard::BitCell
            | IdentityStandard::DidCkb
            | IdentityStandard::DotCell => {}
        }
    }
    all.to_vec()
}

/// A name's label, owner and records all live in CKB cells and witnesses — no
/// off-chain content and no second chain — so every identity standard is
/// `pure_ckb`, looked up by the wire value its aggregate reports.
#[test]
fn every_identity_standard_has_a_bundled_composition_tier() {
    for standard in identity_standards() {
        assert_eq!(
            resolve_object_collection_composition_tier_override(standard.asset_standard()),
            Some("pure_ckb"),
            "no composition tier for identity standard `{}` in docs/metadata/object-tiers.toml",
            standard.asset_standard()
        );
    }
}

/// `.cell` is listed under its display name too, as `.bit` is.
#[test]
fn the_dotcell_display_name_resolves_like_dotbit_s() {
    assert_eq!(
        resolve_object_collection_composition_tier_override(".cell"),
        resolve_object_collection_composition_tier_override(".bit")
    );
}
