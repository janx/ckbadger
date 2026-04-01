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
            resolve: Box::new(|base, _p| {
                let dao = "0x82d76d1b75fe2fd9a27dfbaa65a039221a380d76c926f378d3f81cf3e7e13f2e";
                Some(post(
                    &format!("{base}/scripts/lookup"),
                    &format!(r#"{{"codeHashes":["{dao}"]}}"#),
                ))
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
            resolve: Box::new(|base, _p| {
                let dao = "0x82d76d1b75fe2fd9a27dfbaa65a039221a380d76c926f378d3f81cf3e7e13f2e";
                Some(get(&format!(
                    "{base}/scripts/code-cell?code_hash={dao}&hash_type=type"
                )))
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
            resolve: Box::new(|base, _p| {
                let dao = "0x82d76d1b75fe2fd9a27dfbaa65a039221a380d76c926f378d3f81cf3e7e13f2e";
                Some(get(&format!(
                    "{base}/scripts/code-cells?code_hash={dao}&hash_type=type"
                )))
            }),
            expect_status: 200,
            risk_tier: RiskTier::Medium,
            read_pattern: ReadPattern::CrossStore,
        },
        EndpointEntry {
            module: "scripts",
            method: Method::Get,
            path_template: "/scripts/charts/capacity-history",
            description: "Script capacity history chart (by code_hash)",
            resolve: Box::new(|base, _p| {
                let dao = "0x82d76d1b75fe2fd9a27dfbaa65a039221a380d76c926f378d3f81cf3e7e13f2e";
                Some(get(&format!(
                    "{base}/scripts/charts/capacity-history?code_hash={dao}&hash_type=type"
                )))
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
