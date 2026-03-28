use crate::registry::*;

pub fn entries() -> Vec<EndpointEntry> {
    vec![
        EndpointEntry {
            module: "fiber",
            method: Method::Get,
            path_template: "/fiber/channels",
            description: "List Fiber channels",
            resolve: Box::new(|base, _p| Some(get(&format!("{base}/fiber/channels?limit=20")))),
            expect_status: 200,
            risk_tier: RiskTier::Medium,
            read_pattern: ReadPattern::PrefixScan,
        },
        EndpointEntry {
            module: "fiber",
            method: Method::Get,
            path_template: "/fiber/channels/{channel_id}",
            description: "Get Fiber channel by ID",
            resolve: Box::new(|base, p| {
                let id = p.fiber_channel_id.as_ref()?;
                Some(get(&format!("{base}/fiber/channels/{id}")))
            }),
            expect_status: 200,
            risk_tier: RiskTier::Low,
            read_pattern: ReadPattern::KeyLookup,
        },
        EndpointEntry {
            module: "fiber",
            method: Method::Get,
            path_template: "/fiber/channels/{channel_id}/nodes",
            description: "Get nodes for a Fiber channel",
            resolve: Box::new(|base, p| {
                let id = p.fiber_channel_id.as_ref()?;
                Some(get(&format!("{base}/fiber/channels/{id}/nodes")))
            }),
            expect_status: 200,
            risk_tier: RiskTier::Low,
            read_pattern: ReadPattern::KeyLookup,
        },
        EndpointEntry {
            module: "fiber",
            method: Method::Get,
            path_template: "/fiber/stats",
            description: "Get Fiber network statistics",
            resolve: Box::new(|base, _p| Some(get(&format!("{base}/fiber/stats")))),
            expect_status: 200,
            risk_tier: RiskTier::Low,
            read_pattern: ReadPattern::Cached,
        },
    ]
}
