use crate::registry::*;

pub fn entries() -> Vec<EndpointEntry> {
    vec![EndpointEntry {
        module: "hardforks",
        method: Method::Get,
        path_template: "/hardforks",
        description: "List all hardforks",
        resolve: Box::new(|base, _p| Some(get(&format!("{base}/hardforks")))),
        expect_status: 200,
        risk_tier: RiskTier::Low,
        read_pattern: ReadPattern::Cached,
    }]
}
