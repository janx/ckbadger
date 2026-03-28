use crate::registry::*;

pub fn entries() -> Vec<EndpointEntry> {
    vec![
        EndpointEntry {
            module: "cells",
            method: Method::Get,
            path_template: "/cells/live",
            description: "List live cells",
            resolve: Box::new(|base, _p| Some(get(&format!("{base}/cells/live?limit=20")))),
            expect_status: 200,
            risk_tier: RiskTier::High,
            read_pattern: ReadPattern::CrossStore,
        },
        EndpointEntry {
            module: "cells",
            method: Method::Get,
            path_template: "/cells/by-script",
            description: "List cells by lock script hash",
            resolve: Box::new(|base, p| {
                let lh = p.top_lock_hashes.first()?;
                Some(get(&format!(
                    "{base}/cells/by-script?lock_script_hash={lh}&limit=10"
                )))
            }),
            expect_status: 200,
            risk_tier: RiskTier::High,
            read_pattern: ReadPattern::CrossStore,
        },
        EndpointEntry {
            module: "cells",
            method: Method::Get,
            path_template: "/cells/{tx_hash}/{output_index}",
            description: "Get cell by outpoint",
            resolve: Box::new(|base, p| {
                let (tx_hash, idx) = p.live_cell_outpoint.as_ref()?;
                Some(get(&format!("{base}/cells/{tx_hash}/{idx}")))
            }),
            expect_status: 200,
            risk_tier: RiskTier::High,
            read_pattern: ReadPattern::CrossStore,
        },
        EndpointEntry {
            module: "cells",
            method: Method::Get,
            path_template: "/addresses/top",
            description: "List top addresses by balance",
            resolve: Box::new(|base, _p| Some(get(&format!("{base}/addresses/top?limit=20")))),
            expect_status: 200,
            risk_tier: RiskTier::Low,
            read_pattern: ReadPattern::Cached,
        },
        EndpointEntry {
            module: "cells",
            method: Method::Get,
            path_template: "/addresses/active",
            description: "List recently active addresses",
            resolve: Box::new(|base, _p| Some(get(&format!("{base}/addresses/active?limit=20")))),
            expect_status: 200,
            risk_tier: RiskTier::Low,
            read_pattern: ReadPattern::Cached,
        },
        EndpointEntry {
            module: "cells",
            method: Method::Get,
            path_template: "/addresses/{addr}",
            description: "Get address summary",
            resolve: Box::new(|base, p| {
                let addr = p.top_addresses.first()?;
                Some(get(&format!("{base}/addresses/{addr}")))
            }),
            expect_status: 200,
            risk_tier: RiskTier::Low,
            read_pattern: ReadPattern::KeyLookup,
        },
        EndpointEntry {
            module: "cells",
            method: Method::Get,
            path_template: "/addresses/{addr}/transactions",
            description: "List transactions for an address",
            resolve: Box::new(|base, p| {
                let addr = p.top_addresses.first()?;
                Some(get(&format!(
                    "{base}/addresses/{addr}/transactions?limit=20"
                )))
            }),
            expect_status: 200,
            risk_tier: RiskTier::High,
            read_pattern: ReadPattern::PrefixScan,
        },
        EndpointEntry {
            module: "cells",
            method: Method::Get,
            path_template: "/addresses/{addr}/tokens",
            description: "List tokens held by an address",
            resolve: Box::new(|base, p| {
                let addr = p.top_addresses.first()?;
                Some(get(&format!("{base}/addresses/{addr}/tokens")))
            }),
            expect_status: 200,
            risk_tier: RiskTier::High,
            read_pattern: ReadPattern::PrefixScan,
        },
    ]
}
