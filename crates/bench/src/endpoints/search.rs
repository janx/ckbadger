use crate::registry::*;

pub fn entries() -> Vec<EndpointEntry> {
    vec![EndpointEntry {
        module: "search",
        method: Method::Get,
        path_template: "/search",
        description: "Search by block number, tx hash, or address",
        resolve: Box::new(|base, p| {
            let q = p.latest_block_number;
            Some(get(&format!("{base}/search?q={q}")))
        }),
        expect_status: 200,
        risk_tier: RiskTier::Medium,
        read_pattern: ReadPattern::Cached,
    }]
}
