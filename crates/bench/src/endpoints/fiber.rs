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
            path_template: "/addresses/{addr}/fiber/channels",
            description: "Get Fiber channels for an address",
            resolve: Box::new(|base, p| {
                let lh = p.top_lock_hashes.first()?;
                Some(get(&format!("{base}/addresses/{lh}/fiber/channels")))
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
