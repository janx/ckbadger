use crate::registry::*;

pub fn entries() -> Vec<EndpointEntry> {
    vec![
        EndpointEntry {
            module: "graph",
            method: Method::Get,
            path_template: "/graph/cell/{tx_hash}/{output_index}",
            description: "Get cell dependency graph",
            resolve: Box::new(|base, p| {
                let (tx_hash, idx) = p
                    .live_cell_outpoint
                    .as_ref()
                    .or(p.dao_deposit_outpoint.as_ref())?;
                Some(get(&format!("{base}/graph/cell/{tx_hash}/{idx}")))
            }),
            expect_status: 200,
            risk_tier: RiskTier::Medium,
            read_pattern: ReadPattern::RpcDependent,
        },
        EndpointEntry {
            module: "graph",
            method: Method::Get,
            path_template: "/graph/transaction/{hash}",
            description: "Get transaction dependency graph",
            resolve: Box::new(|base, p| {
                let h = p.tx_hashes.first()?;
                Some(get(&format!("{base}/graph/transaction/{h}")))
            }),
            expect_status: 200,
            risk_tier: RiskTier::Medium,
            read_pattern: ReadPattern::RpcDependent,
        },
        EndpointEntry {
            module: "graph",
            method: Method::Get,
            path_template: "/graph/proposals/{block_number}",
            description: "Get block proposals graph",
            resolve: Box::new(|base, p| {
                let num = p.latest_block_number;
                Some(get(&format!("{base}/graph/proposals/{num}")))
            }),
            expect_status: 200,
            risk_tier: RiskTier::Medium,
            read_pattern: ReadPattern::RpcDependent,
        },
    ]
}
