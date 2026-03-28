use crate::registry::*;

pub fn entries() -> Vec<EndpointEntry> {
    vec![
        EndpointEntry {
            module: "dao",
            method: Method::Get,
            path_template: "/dao/deposits",
            description: "List DAO deposits",
            resolve: Box::new(|base, _p| Some(get(&format!("{base}/dao/deposits?limit=20")))),
            expect_status: 200,
            risk_tier: RiskTier::High,
            read_pattern: ReadPattern::PrefixScan,
        },
        EndpointEntry {
            module: "dao",
            method: Method::Get,
            path_template: "/dao/deposits/{lock_hash}",
            description: "List DAO deposits for a lock hash",
            resolve: Box::new(|base, p| {
                let lh = p.dao_lock_hashes.first()?;
                Some(get(&format!("{base}/dao/deposits/{lh}")))
            }),
            expect_status: 200,
            risk_tier: RiskTier::High,
            read_pattern: ReadPattern::PrefixScan,
        },
        EndpointEntry {
            module: "dao",
            method: Method::Get,
            path_template: "/dao/summary/{lock_hash}",
            description: "Get DAO summary for a lock hash",
            resolve: Box::new(|base, p| {
                let lh = p.dao_lock_hashes.first()?;
                Some(get(&format!("{base}/dao/summary/{lh}")))
            }),
            expect_status: 200,
            risk_tier: RiskTier::Medium,
            read_pattern: ReadPattern::BatchLookup,
        },
        EndpointEntry {
            module: "dao",
            method: Method::Get,
            path_template: "/dao/statistics",
            description: "Get DAO statistics",
            resolve: Box::new(|base, _p| Some(get(&format!("{base}/dao/statistics")))),
            expect_status: 200,
            risk_tier: RiskTier::Low,
            read_pattern: ReadPattern::Cached,
        },
        EndpointEntry {
            module: "dao",
            method: Method::Get,
            path_template: "/dao/top-depositors",
            description: "List top DAO depositors",
            resolve: Box::new(|base, _p| Some(get(&format!("{base}/dao/top-depositors?limit=20")))),
            expect_status: 200,
            risk_tier: RiskTier::High,
            read_pattern: ReadPattern::PrefixScan,
        },
        EndpointEntry {
            module: "dao",
            method: Method::Get,
            path_template: "/dao/calculator",
            description: "DAO compensation calculator",
            resolve: Box::new(|base, p| {
                let (h, i) = p.dao_deposit_outpoint.as_ref()?;
                Some(get(&format!(
                    "{base}/dao/calculator?tx_hash={h}&output_index={i}"
                )))
            }),
            expect_status: 200,
            risk_tier: RiskTier::Low,
            read_pattern: ReadPattern::KeyLookup,
        },
        EndpointEntry {
            module: "dao",
            method: Method::Get,
            path_template: "/dao/charts/total-deposit",
            description: "DAO total deposit chart",
            resolve: Box::new(|base, _p| Some(get(&format!("{base}/dao/charts/total-deposit")))),
            expect_status: 200,
            risk_tier: RiskTier::Medium,
            read_pattern: ReadPattern::Aggregation,
        },
        EndpointEntry {
            module: "dao",
            method: Method::Get,
            path_template: "/dao/charts/daily-deposit",
            description: "DAO daily deposit chart",
            resolve: Box::new(|base, _p| Some(get(&format!("{base}/dao/charts/daily-deposit")))),
            expect_status: 200,
            risk_tier: RiskTier::Medium,
            read_pattern: ReadPattern::Aggregation,
        },
        EndpointEntry {
            module: "dao",
            method: Method::Get,
            path_template: "/dao/charts/deposit-rate",
            description: "DAO deposit rate chart",
            resolve: Box::new(|base, _p| Some(get(&format!("{base}/dao/charts/deposit-rate")))),
            expect_status: 200,
            risk_tier: RiskTier::Medium,
            read_pattern: ReadPattern::Aggregation,
        },
    ]
}
