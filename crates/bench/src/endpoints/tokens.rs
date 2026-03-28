use crate::registry::*;

pub fn entries() -> Vec<EndpointEntry> {
    vec![
        EndpointEntry {
            module: "tokens",
            method: Method::Get,
            path_template: "/tokens",
            description: "List all tokens",
            resolve: Box::new(|base, _p| Some(get(&format!("{base}/tokens")))),
            expect_status: 200,
            risk_tier: RiskTier::High,
            read_pattern: ReadPattern::FullCfScan,
        },
        EndpointEntry {
            module: "tokens",
            method: Method::Get,
            path_template: "/tokens/{type_hash}",
            description: "Get token by type hash",
            resolve: Box::new(|base, p| {
                let th = p.token_type_hashes.first()?;
                Some(get(&format!("{base}/tokens/{th}")))
            }),
            expect_status: 200,
            risk_tier: RiskTier::Low,
            read_pattern: ReadPattern::KeyLookup,
        },
        EndpointEntry {
            module: "tokens",
            method: Method::Get,
            path_template: "/tokens/{type_hash}/holders",
            description: "List token holders",
            resolve: Box::new(|base, p| {
                let th = p.token_type_hashes.first()?;
                Some(get(&format!("{base}/tokens/{th}/holders?limit=20")))
            }),
            expect_status: 200,
            risk_tier: RiskTier::High,
            read_pattern: ReadPattern::PrefixScan,
        },
        EndpointEntry {
            module: "tokens",
            method: Method::Get,
            path_template: "/tokens/{type_hash}/transfers",
            description: "List token transfers",
            resolve: Box::new(|base, p| {
                let th = p.token_type_hashes.first()?;
                Some(get(&format!("{base}/tokens/{th}/transfers?limit=20")))
            }),
            expect_status: 200,
            risk_tier: RiskTier::High,
            read_pattern: ReadPattern::PrefixScan,
        },
        EndpointEntry {
            module: "tokens",
            method: Method::Get,
            path_template: "/tokens/{type_hash}/activities",
            description: "List token activities",
            resolve: Box::new(|base, p| {
                let th = p.token_type_hashes.first()?;
                Some(get(&format!("{base}/tokens/{th}/activities?limit=20")))
            }),
            expect_status: 200,
            risk_tier: RiskTier::Medium,
            read_pattern: ReadPattern::PrefixScan,
        },
    ]
}
