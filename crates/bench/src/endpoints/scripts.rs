use crate::registry::*;

pub fn entries() -> Vec<EndpointEntry> {
    vec![
        EndpointEntry {
            module: "scripts",
            method: Method::Get,
            path_template: "/scripts",
            description: "List all known scripts",
            resolve: Box::new(|base, _p| Some(get(&format!("{base}/scripts")))),
            expect_status: 200,
            risk_tier: RiskTier::Low,
            read_pattern: ReadPattern::Cached,
        },
        EndpointEntry {
            module: "scripts",
            method: Method::Post,
            path_template: "/scripts/lookup",
            description: "Lookup scripts by code_hash",
            resolve: Box::new(|_base, _p| {
                // Requires code_hash values not available from list endpoint
                None
            }),
            expect_status: 200,
            risk_tier: RiskTier::Low,
            read_pattern: ReadPattern::KeyLookup,
        },
        EndpointEntry {
            module: "scripts",
            method: Method::Get,
            path_template: "/scripts/code-cell",
            description: "Get script code cell",
            resolve: Box::new(|_base, _p| {
                // Skip — needs specific code_hash query param
                None
            }),
            expect_status: 200,
            risk_tier: RiskTier::Medium,
            read_pattern: ReadPattern::CrossStore,
        },
        EndpointEntry {
            module: "scripts",
            method: Method::Get,
            path_template: "/scripts/code-cells",
            description: "List script code cells",
            resolve: Box::new(|_base, _p| {
                // Skip — needs specific query params
                None
            }),
            expect_status: 200,
            risk_tier: RiskTier::Medium,
            read_pattern: ReadPattern::CrossStore,
        },
        EndpointEntry {
            module: "scripts",
            method: Method::Get,
            path_template: "/scripts/charts/capacity-history",
            description: "Script capacity history chart (global)",
            resolve: Box::new(|_base, _p| {
                // Skip — needs code_hash query param
                None
            }),
            expect_status: 200,
            risk_tier: RiskTier::Medium,
            read_pattern: ReadPattern::Aggregation,
        },
        EndpointEntry {
            module: "scripts",
            method: Method::Get,
            path_template: "/scripts/{name}",
            description: "Get script by name",
            resolve: Box::new(|base, p| {
                let name = p.script_names.first()?;
                Some(get(&format!("{base}/scripts/{name}")))
            }),
            expect_status: 200,
            risk_tier: RiskTier::Low,
            read_pattern: ReadPattern::KeyLookup,
        },
        EndpointEntry {
            module: "scripts",
            method: Method::Get,
            path_template: "/scripts/{name}/usage",
            description: "Get script usage stats",
            resolve: Box::new(|base, p| {
                let name = p.script_names.first()?;
                Some(get(&format!("{base}/scripts/{name}/usage")))
            }),
            expect_status: 200,
            risk_tier: RiskTier::Low,
            read_pattern: ReadPattern::KeyLookup,
        },
        EndpointEntry {
            module: "scripts",
            method: Method::Get,
            path_template: "/scripts/{name}/charts/capacity-history",
            description: "Script capacity history chart by name",
            resolve: Box::new(|base, p| {
                let name = p.script_names.first()?;
                Some(get(&format!(
                    "{base}/scripts/{name}/charts/capacity-history"
                )))
            }),
            expect_status: 200,
            risk_tier: RiskTier::Medium,
            read_pattern: ReadPattern::Aggregation,
        },
    ]
}
