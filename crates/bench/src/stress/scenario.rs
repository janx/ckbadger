use anyhow::{bail, Result};
use rand::Rng;

use crate::registry::{EndpointEntry, ReadPattern, RiskTier};

/// Which stress scenario to run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Scenario {
    /// Realistic traffic mix weighted by page popularity.
    Mixed,
    /// Hammer the heaviest (High-risk) endpoints only.
    Heavy,
}

impl Scenario {
    /// Parse a comma-separated list of scenario names.
    ///
    /// Example: `"mixed,heavy"` -> `[Mixed, Heavy]`.
    pub fn parse(s: &str) -> Result<Vec<Scenario>> {
        let mut out = Vec::new();
        for part in s.split(',') {
            let trimmed = part.trim();
            match trimmed.to_lowercase().as_str() {
                "mixed" => out.push(Scenario::Mixed),
                "heavy" => out.push(Scenario::Heavy),
                other => bail!("unknown scenario: {other:?}; expected \"mixed\" or \"heavy\""),
            }
        }
        if out.is_empty() {
            bail!("no scenarios specified");
        }
        Ok(out)
    }
}

/// A group of related endpoints with a selection weight.
pub struct EndpointGroup {
    #[allow(dead_code)] // used by report module (Task 6)
    pub name: &'static str,
    pub weight: u32,
    pub endpoint_indices: Vec<usize>,
}

/// Build endpoint groups for the `Mixed` scenario.
///
/// Groups endpoints by module into page-like categories with weights
/// reflecting realistic traffic distribution.
pub fn build_mixed_groups(entries: &[EndpointEntry]) -> Vec<EndpointGroup> {
    let group_defs: &[(&str, u32, &[&str])] = &[
        ("homepage", 25, &["statistics", "mempool", "hardforks"]),
        ("blocks", 20, &["blocks"]),
        ("transactions", 20, &["transactions"]),
        ("addresses", 15, &["activities", "cells"]),
        ("assets", 10, &["tokens", "spore", "assets"]),
        (
            "other",
            10,
            &[
                "search",
                "dao",
                "scripts",
                "identities",
                "fiber",
                "forks",
                "graph",
            ],
        ),
    ];

    let mut groups = Vec::new();
    for &(name, weight, modules) in group_defs {
        let indices: Vec<usize> = entries
            .iter()
            .enumerate()
            .filter(|(_, e)| modules.contains(&e.module))
            .map(|(i, _)| i)
            .collect();

        if !indices.is_empty() {
            groups.push(EndpointGroup {
                name,
                weight,
                endpoint_indices: indices,
            });
        }
    }
    groups
}

/// Build endpoint groups for the `Heavy` scenario.
///
/// Includes only `RiskTier::High` endpoints, grouped by their
/// `ReadPattern` with weights reflecting relative cost.
pub fn build_heavy_groups(entries: &[EndpointEntry]) -> Vec<EndpointGroup> {
    let pattern_defs: &[(&str, u32, ReadPattern)] = &[
        ("full_cf_scan", 4, ReadPattern::FullCfScan),
        ("cross_store", 4, ReadPattern::CrossStore),
        ("range_scan", 3, ReadPattern::RangeScan),
        ("prefix_scan", 2, ReadPattern::PrefixScan),
    ];

    let mut groups = Vec::new();

    // Named pattern groups
    for &(name, weight, pattern) in pattern_defs {
        let indices: Vec<usize> = entries
            .iter()
            .enumerate()
            .filter(|(_, e)| e.risk_tier == RiskTier::High && e.read_pattern == pattern)
            .map(|(i, _)| i)
            .collect();

        if !indices.is_empty() {
            groups.push(EndpointGroup {
                name,
                weight,
                endpoint_indices: indices,
            });
        }
    }

    // Catch-all "other" group for High-risk endpoints with patterns not
    // listed above (KeyLookup, BatchLookup, RpcDependent, Cached, Aggregation).
    let named_patterns: Vec<ReadPattern> = pattern_defs.iter().map(|&(_, _, p)| p).collect();
    let other_indices: Vec<usize> = entries
        .iter()
        .enumerate()
        .filter(|(_, e)| e.risk_tier == RiskTier::High && !named_patterns.contains(&e.read_pattern))
        .map(|(i, _)| i)
        .collect();

    if !other_indices.is_empty() {
        groups.push(EndpointGroup {
            name: "other",
            weight: 1,
            endpoint_indices: other_indices,
        });
    }

    groups
}

/// Weighted random selection of a single endpoint index.
///
/// Rolls a random value in `0..total_weight`, walks the groups to find which
/// group the roll falls in, then picks a random endpoint within that group.
///
/// # Panics
///
/// Panics if `groups` is empty or all weights are zero.
pub fn pick_endpoint(groups: &[EndpointGroup]) -> usize {
    assert!(!groups.is_empty(), "pick_endpoint called with no groups");

    let total_weight: u32 = groups.iter().map(|g| g.weight).sum();
    assert!(total_weight > 0, "pick_endpoint: total weight is zero");

    let mut rng = rand::rng();
    let roll = rng.random_range(0..total_weight);

    let mut accum = 0u32;
    for group in groups {
        accum += group.weight;
        if roll < accum {
            let idx = rng.random_range(0..group.endpoint_indices.len());
            return group.endpoint_indices[idx];
        }
    }

    // Should be unreachable if weights are correct, but pick from last group.
    let last = groups.last().unwrap();
    last.endpoint_indices[0]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::{get, EndpointEntry, Method, ReadPattern, RiskTier};

    fn sample_entry(
        module: &'static str,
        risk_tier: RiskTier,
        read_pattern: ReadPattern,
    ) -> EndpointEntry {
        EndpointEntry {
            module,
            method: Method::Get,
            path_template: "/test",
            description: "test endpoint",
            resolve: Box::new(|base, _| Some(get(&format!("{base}/test")))),
            expect_status: 200,
            risk_tier,
            read_pattern,
        }
    }

    #[test]
    fn test_parse_scenario() {
        // Single
        let mixed = Scenario::parse("mixed").unwrap();
        assert_eq!(mixed, vec![Scenario::Mixed]);

        let heavy = Scenario::parse("heavy").unwrap();
        assert_eq!(heavy, vec![Scenario::Heavy]);

        // Multiple
        let both = Scenario::parse("mixed,heavy").unwrap();
        assert_eq!(both, vec![Scenario::Mixed, Scenario::Heavy]);

        // Case insensitive
        let upper = Scenario::parse("MIXED").unwrap();
        assert_eq!(upper, vec![Scenario::Mixed]);

        // Error on unknown
        let err = Scenario::parse("bogus");
        assert!(err.is_err());
        let msg = err.unwrap_err().to_string();
        assert!(msg.contains("bogus"), "error should mention the bad input");
    }

    #[test]
    fn test_mixed_scenario_has_groups() {
        let entries = vec![
            sample_entry("statistics", RiskTier::Low, ReadPattern::Cached),
            sample_entry("blocks", RiskTier::Medium, ReadPattern::KeyLookup),
            sample_entry("transactions", RiskTier::Medium, ReadPattern::KeyLookup),
            sample_entry("activities", RiskTier::High, ReadPattern::RangeScan),
            sample_entry("tokens", RiskTier::Medium, ReadPattern::PrefixScan),
            sample_entry("dao", RiskTier::Medium, ReadPattern::Aggregation),
        ];

        let groups = build_mixed_groups(&entries);
        assert!(!groups.is_empty(), "should produce at least one group");

        // Every entry's index should appear in exactly one group
        let mut all_indices: Vec<usize> = groups
            .iter()
            .flat_map(|g| g.endpoint_indices.iter().copied())
            .collect();
        all_indices.sort();
        all_indices.dedup();
        assert_eq!(all_indices.len(), entries.len());

        // Verify homepage group exists and contains index 0 (statistics)
        let homepage = groups.iter().find(|g| g.name == "homepage").unwrap();
        assert!(homepage.endpoint_indices.contains(&0));
        assert_eq!(homepage.weight, 25);
    }

    #[test]
    fn test_heavy_scenario_filters_high_risk() {
        let entries = vec![
            sample_entry("blocks", RiskTier::Low, ReadPattern::KeyLookup),
            sample_entry("activities", RiskTier::High, ReadPattern::RangeScan),
            sample_entry("cells", RiskTier::High, ReadPattern::CrossStore),
            sample_entry("tokens", RiskTier::Medium, ReadPattern::PrefixScan),
            sample_entry("scripts", RiskTier::High, ReadPattern::FullCfScan),
        ];

        let groups = build_heavy_groups(&entries);

        // Only High-risk endpoints included
        let all_indices: Vec<usize> = groups
            .iter()
            .flat_map(|g| g.endpoint_indices.iter().copied())
            .collect();

        for &idx in &all_indices {
            assert_eq!(
                entries[idx].risk_tier,
                RiskTier::High,
                "index {idx} should be High risk"
            );
        }

        // Index 0 (Low) and 3 (Medium) should NOT appear
        assert!(!all_indices.contains(&0));
        assert!(!all_indices.contains(&3));

        // Indices 1, 2, 4 (all High) should appear
        assert!(all_indices.contains(&1));
        assert!(all_indices.contains(&2));
        assert!(all_indices.contains(&4));
    }

    #[test]
    fn test_pick_endpoint_respects_weights() {
        // Group A has weight 0, group B has weight 10.
        // pick_endpoint should never select from group A.
        let groups = vec![
            EndpointGroup {
                name: "zero_weight",
                weight: 0,
                endpoint_indices: vec![99],
            },
            EndpointGroup {
                name: "all_weight",
                weight: 10,
                endpoint_indices: vec![1, 2, 3],
            },
        ];

        for _ in 0..200 {
            let picked = pick_endpoint(&groups);
            assert_ne!(picked, 99, "should never pick from zero-weight group");
            assert!(
                [1, 2, 3].contains(&picked),
                "picked {picked} not in expected set"
            );
        }
    }
}
