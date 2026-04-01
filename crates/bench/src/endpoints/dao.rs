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
                let capacity = p.dao_deposit_capacity.as_ref()?;
                let block = p.dao_deposit_block?;
                Some(get(&format!(
                    "{base}/dao/calculator?capacity={capacity}&deposit_block={block}"
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
            path_template: "/dao/charts/daily-depositors",
            description: "DAO daily depositors chart",
            resolve: Box::new(|base, _p| Some(get(&format!("{base}/dao/charts/daily-depositors")))),
            expect_status: 200,
            risk_tier: RiskTier::Medium,
            read_pattern: ReadPattern::Aggregation,
        },
        EndpointEntry {
            module: "dao",
            method: Method::Get,
            path_template: "/dao/charts/circulation-ratio",
            description: "DAO circulation ratio chart",
            resolve: Box::new(|base, _p| {
                Some(get(&format!("{base}/dao/charts/circulation-ratio")))
            }),
            expect_status: 200,
            risk_tier: RiskTier::Medium,
            read_pattern: ReadPattern::Aggregation,
        },
    ]
}
