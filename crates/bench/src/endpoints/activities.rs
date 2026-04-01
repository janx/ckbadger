use crate::registry::*;

pub fn entries() -> Vec<EndpointEntry> {
    vec![
        EndpointEntry {
            module: "activities",
            method: Method::Get,
            path_template: "/activities",
            description: "List recent activities",
            resolve: Box::new(|base, _p| Some(get(&format!("{base}/activities?limit=20")))),
            expect_status: 200,
            risk_tier: RiskTier::High,
            read_pattern: ReadPattern::RangeScan,
        },
        EndpointEntry {
            module: "activities",
            method: Method::Get,
            path_template: "/activities/latest",
            description: "Get latest activity",
            resolve: Box::new(|base, _p| Some(get(&format!("{base}/activities/latest")))),
            expect_status: 200,
            risk_tier: RiskTier::High,
            read_pattern: ReadPattern::RangeScan,
        },
        EndpointEntry {
            module: "activities",
            method: Method::Get,
            path_template: "/addresses/{addr}/activities",
            description: "List activities for an address",
            resolve: Box::new(|base, p| {
                let lh = p.top_lock_hashes.first()?;
                Some(get(&format!("{base}/addresses/{lh}/activities?limit=50")))
            }),
            expect_status: 200,
            risk_tier: RiskTier::High,
            read_pattern: ReadPattern::RangeScan,
        },
    ]
}
