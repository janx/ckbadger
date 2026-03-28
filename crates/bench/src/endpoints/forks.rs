use crate::registry::*;

pub fn entries() -> Vec<EndpointEntry> {
    vec![
        EndpointEntry {
            module: "forks",
            method: Method::Get,
            path_template: "/forks",
            description: "List all forks",
            resolve: Box::new(|base, _p| Some(get(&format!("{base}/forks")))),
            expect_status: 200,
            risk_tier: RiskTier::Low,
            read_pattern: ReadPattern::PrefixScan,
        },
        EndpointEntry {
            module: "forks",
            method: Method::Get,
            path_template: "/forks/recent",
            description: "Get most recent fork",
            resolve: Box::new(|base, _p| Some(get(&format!("{base}/forks/recent")))),
            expect_status: 200,
            risk_tier: RiskTier::Low,
            read_pattern: ReadPattern::KeyLookup,
        },
        EndpointEntry {
            module: "forks",
            method: Method::Get,
            path_template: "/forks/{id}",
            description: "Get fork by ID",
            resolve: Box::new(|base, p| {
                let id = p.fork_id.as_ref()?;
                Some(get(&format!("{base}/forks/{id}")))
            }),
            expect_status: 200,
            risk_tier: RiskTier::Low,
            read_pattern: ReadPattern::KeyLookup,
        },
    ]
}
