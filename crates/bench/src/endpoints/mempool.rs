use crate::registry::*;

pub fn entries() -> Vec<EndpointEntry> {
    vec![
        EndpointEntry {
            module: "mempool",
            method: Method::Get,
            path_template: "/mempool/info",
            description: "Get mempool info",
            resolve: Box::new(|base, _p| Some(get(&format!("{base}/mempool/info")))),
            expect_status: 200,
            risk_tier: RiskTier::Low,
            read_pattern: ReadPattern::RpcDependent,
        },
        EndpointEntry {
            module: "mempool",
            method: Method::Get,
            path_template: "/mempool/transactions",
            description: "List mempool transactions",
            resolve: Box::new(|base, _p| Some(get(&format!("{base}/mempool/transactions")))),
            expect_status: 200,
            risk_tier: RiskTier::Low,
            read_pattern: ReadPattern::RpcDependent,
        },
        EndpointEntry {
            module: "mempool",
            method: Method::Get,
            path_template: "/mempool/blocks",
            description: "List mempool blocks",
            resolve: Box::new(|base, _p| Some(get(&format!("{base}/mempool/blocks")))),
            expect_status: 200,
            risk_tier: RiskTier::Low,
            read_pattern: ReadPattern::RpcDependent,
        },
        EndpointEntry {
            module: "mempool",
            method: Method::Get,
            path_template: "/mempool/pending-proposals",
            description: "List mempool pending proposals",
            resolve: Box::new(|base, _p| Some(get(&format!("{base}/mempool/pending-proposals")))),
            expect_status: 200,
            risk_tier: RiskTier::Low,
            read_pattern: ReadPattern::RpcDependent,
        },
    ]
}
