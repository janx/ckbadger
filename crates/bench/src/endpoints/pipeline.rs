use crate::registry::*;

pub fn entries() -> Vec<EndpointEntry> {
    vec![EndpointEntry {
        module: "pipeline",
        method: Method::Get,
        path_template: "/pipeline",
        description: "Homepage transaction pipeline snapshot",
        resolve: Box::new(|base, _p| Some(get(&format!("{base}/pipeline")))),
        expect_status: 200,
        risk_tier: RiskTier::Medium,
        read_pattern: ReadPattern::RpcDependent,
    }]
}
