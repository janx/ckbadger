use crate::registry::*;

pub fn entries() -> Vec<EndpointEntry> {
    vec![
        EndpointEntry {
            module: "blocks",
            method: Method::Get,
            path_template: "/blocks",
            description: "List recent blocks",
            resolve: Box::new(|base, _p| Some(get(&format!("{base}/blocks?limit=20")))),
            expect_status: 200,
            risk_tier: RiskTier::Medium,
            read_pattern: ReadPattern::PrefixScan,
        },
        EndpointEntry {
            module: "blocks",
            method: Method::Get,
            path_template: "/blocks/{id}",
            description: "Get block by number",
            resolve: Box::new(|base, p| {
                let num = p.latest_block_number;
                Some(get(&format!("{base}/blocks/{num}")))
            }),
            expect_status: 200,
            risk_tier: RiskTier::Low,
            read_pattern: ReadPattern::KeyLookup,
        },
        EndpointEntry {
            module: "blocks",
            method: Method::Get,
            path_template: "/blocks/{id}/fee-stats",
            description: "Get fee statistics for a block",
            resolve: Box::new(|base, p| {
                let num = p.latest_block_number;
                Some(get(&format!("{base}/blocks/{num}/fee-stats")))
            }),
            expect_status: 200,
            risk_tier: RiskTier::Medium,
            read_pattern: ReadPattern::BatchLookup,
        },
        EndpointEntry {
            module: "blocks",
            method: Method::Get,
            path_template: "/blocks/{id}/proposals",
            description: "Get proposals for a block",
            resolve: Box::new(|base, p| {
                let num = p.latest_block_number;
                Some(get(&format!("{base}/blocks/{num}/proposals")))
            }),
            expect_status: 200,
            risk_tier: RiskTier::Low,
            read_pattern: ReadPattern::KeyLookup,
        },
    ]
}
