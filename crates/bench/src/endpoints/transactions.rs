use crate::registry::*;

pub fn entries() -> Vec<EndpointEntry> {
    vec![
        EndpointEntry {
            module: "transactions",
            method: Method::Get,
            path_template: "/transactions",
            description: "List recent transactions",
            resolve: Box::new(|base, _p| Some(get(&format!("{base}/transactions?limit=20")))),
            expect_status: 200,
            risk_tier: RiskTier::Medium,
            read_pattern: ReadPattern::PrefixScan,
        },
        EndpointEntry {
            module: "transactions",
            method: Method::Get,
            path_template: "/transactions/{hash}",
            description: "Get transaction by hash",
            resolve: Box::new(|base, p| {
                let h = p.tx_hashes.first()?;
                Some(get(&format!("{base}/transactions/{h}")))
            }),
            expect_status: 200,
            risk_tier: RiskTier::Low,
            read_pattern: ReadPattern::BatchLookup,
        },
        EndpointEntry {
            module: "transactions",
            method: Method::Get,
            path_template: "/transactions/{hash}/detail",
            description: "Get detailed transaction info with cells",
            resolve: Box::new(|base, p| {
                let h = p.tx_hashes.first()?;
                Some(get(&format!("{base}/transactions/{h}/detail")))
            }),
            expect_status: 200,
            risk_tier: RiskTier::High,
            read_pattern: ReadPattern::CrossStore,
        },
        EndpointEntry {
            module: "transactions",
            method: Method::Get,
            path_template: "/transactions/{hash}/cell-deps",
            description: "Get transaction cell dependencies",
            resolve: Box::new(|base, p| {
                let h = p.tx_hashes.first()?;
                Some(get(&format!("{base}/transactions/{h}/cell-deps")))
            }),
            expect_status: 200,
            risk_tier: RiskTier::Medium,
            read_pattern: ReadPattern::CrossStore,
        },
        EndpointEntry {
            module: "transactions",
            method: Method::Get,
            path_template: "/transactions/{hash}/cycles",
            description: "Get transaction cycle count",
            resolve: Box::new(|base, p| {
                let h = p.tx_hashes.first()?;
                Some(get(&format!("{base}/transactions/{h}/cycles")))
            }),
            expect_status: 200,
            risk_tier: RiskTier::Low,
            read_pattern: ReadPattern::KeyLookup,
        },
    ]
}
