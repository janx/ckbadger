use crate::registry::*;

/// Benchmark the heaviest detail pages: top-10 items from each list page,
/// plus the 10 busiest address pages by transaction count.
///
/// Purpose: detect latency regressions on pages with the most data.
pub fn entries() -> Vec<EndpointEntry> {
    let mut out = Vec::new();
    out.extend(script_detail_entries());
    out.extend(token_detail_entries());
    out.extend(spore_detail_entries());
    out.extend(identity_detail_entries());
    out.extend(busiest_address_entries());
    out
}

fn script_detail_entries() -> Vec<EndpointEntry> {
    (0..10)
        .flat_map(|i| {
            let rank = i + 1;
            let desc_detail: &'static str =
                Box::leak(format!("Script detail #{rank} (by cells)").into_boxed_str());
            let desc_usage: &'static str =
                Box::leak(format!("Script usage #{rank} (by cells)").into_boxed_str());

            vec![
                EndpointEntry {
                    module: "heavy_pages",
                    method: Method::Get,
                    path_template: "/scripts/{name}",
                    description: desc_detail,
                    resolve: Box::new(move |base, p| {
                        let name = p.top_script_names.get(i)?;
                        Some(get(&format!("{base}/scripts/{name}")))
                    }),
                    expect_status: 200,
                    risk_tier: RiskTier::Medium,
                    read_pattern: ReadPattern::KeyLookup,
                },
                EndpointEntry {
                    module: "heavy_pages",
                    method: Method::Get,
                    path_template: "/scripts/{name}/usage",
                    description: desc_usage,
                    resolve: Box::new(move |base, p| {
                        let name = p.top_script_names.get(i)?;
                        Some(get(&format!("{base}/scripts/{name}/usage")))
                    }),
                    expect_status: 200,
                    risk_tier: RiskTier::Medium,
                    read_pattern: ReadPattern::KeyLookup,
                },
            ]
        })
        .collect()
}

fn token_detail_entries() -> Vec<EndpointEntry> {
    (0..10)
        .flat_map(|i| {
            let rank = i + 1;
            let desc_detail: &'static str =
                Box::leak(format!("Token detail #{rank} (by holders)").into_boxed_str());
            let desc_holders: &'static str =
                Box::leak(format!("Token holders #{rank} (by holders)").into_boxed_str());
            let desc_activities: &'static str =
                Box::leak(format!("Token activities #{rank} (by holders)").into_boxed_str());

            vec![
                EndpointEntry {
                    module: "heavy_pages",
                    method: Method::Get,
                    path_template: "/tokens/{type_hash}",
                    description: desc_detail,
                    resolve: Box::new(move |base, p| {
                        let th = p.top_token_type_hashes.get(i)?;
                        Some(get(&format!("{base}/tokens/{th}")))
                    }),
                    expect_status: 200,
                    risk_tier: RiskTier::Medium,
                    read_pattern: ReadPattern::KeyLookup,
                },
                EndpointEntry {
                    module: "heavy_pages",
                    method: Method::Get,
                    path_template: "/tokens/{type_hash}/holders",
                    description: desc_holders,
                    resolve: Box::new(move |base, p| {
                        let th = p.top_token_type_hashes.get(i)?;
                        Some(get(&format!("{base}/tokens/{th}/holders?limit=50")))
                    }),
                    expect_status: 200,
                    risk_tier: RiskTier::High,
                    read_pattern: ReadPattern::PrefixScan,
                },
                EndpointEntry {
                    module: "heavy_pages",
                    method: Method::Get,
                    path_template: "/tokens/{type_hash}/activities",
                    description: desc_activities,
                    resolve: Box::new(move |base, p| {
                        let th = p.top_token_type_hashes.get(i)?;
                        Some(get(&format!("{base}/tokens/{th}/activities?limit=50")))
                    }),
                    expect_status: 200,
                    risk_tier: RiskTier::High,
                    read_pattern: ReadPattern::PrefixScan,
                },
            ]
        })
        .collect()
}

fn spore_detail_entries() -> Vec<EndpointEntry> {
    (0..10)
        .flat_map(|i| {
            let rank = i + 1;
            let desc_detail: &'static str =
                Box::leak(format!("Spore object detail #{rank}").into_boxed_str());
            let desc_activities: &'static str =
                Box::leak(format!("Spore object activities #{rank}").into_boxed_str());

            vec![
                EndpointEntry {
                    module: "heavy_pages",
                    method: Method::Get,
                    path_template: "/spore/objects/{spore_id}",
                    description: desc_detail,
                    resolve: Box::new(move |base, p| {
                        let id = p.top_spore_ids.get(i)?;
                        Some(get(&format!("{base}/spore/objects/{id}")))
                    }),
                    expect_status: 200,
                    risk_tier: RiskTier::Medium,
                    read_pattern: ReadPattern::KeyLookup,
                },
                EndpointEntry {
                    module: "heavy_pages",
                    method: Method::Get,
                    path_template: "/spore/objects/{spore_id}/activities",
                    description: desc_activities,
                    resolve: Box::new(move |base, p| {
                        let id = p.top_spore_ids.get(i)?;
                        Some(get(&format!(
                            "{base}/spore/objects/{id}/activities?limit=50"
                        )))
                    }),
                    expect_status: 200,
                    risk_tier: RiskTier::High,
                    read_pattern: ReadPattern::PrefixScan,
                },
            ]
        })
        .collect()
}

fn identity_detail_entries() -> Vec<EndpointEntry> {
    (0..10)
        .map(|i| {
            let rank = i + 1;
            let desc: &'static str =
                Box::leak(format!(".bit identity detail #{rank}").into_boxed_str());

            EndpointEntry {
                module: "heavy_pages",
                method: Method::Get,
                path_template: "/assets/identities/dotbit/items/{identity_id}",
                description: desc,
                resolve: Box::new(move |base, p| {
                    let id = p.top_dotbit_item_ids.get(i)?;
                    Some(get(&format!("{base}/assets/identities/dotbit/items/{id}")))
                }),
                expect_status: 200,
                risk_tier: RiskTier::Medium,
                read_pattern: ReadPattern::KeyLookup,
            }
        })
        .collect()
}

fn busiest_address_entries() -> Vec<EndpointEntry> {
    (0..10)
        .flat_map(|i| {
            let rank = i + 1;
            let desc_summary: &'static str =
                Box::leak(format!("Busiest address #{rank} summary").into_boxed_str());
            let desc_activities: &'static str =
                Box::leak(format!("Busiest address #{rank} activities").into_boxed_str());
            let desc_txs: &'static str =
                Box::leak(format!("Busiest address #{rank} transactions").into_boxed_str());

            vec![
                EndpointEntry {
                    module: "heavy_pages",
                    method: Method::Get,
                    path_template: "/addresses/{addr}",
                    description: desc_summary,
                    resolve: Box::new(move |base, p| {
                        let lh = p.busiest_lock_hashes.get(i)?;
                        Some(get(&format!("{base}/addresses/{lh}")))
                    }),
                    expect_status: 200,
                    risk_tier: RiskTier::Medium,
                    read_pattern: ReadPattern::KeyLookup,
                },
                EndpointEntry {
                    module: "heavy_pages",
                    method: Method::Get,
                    path_template: "/addresses/{addr}/activities",
                    description: desc_activities,
                    resolve: Box::new(move |base, p| {
                        let lh = p.busiest_lock_hashes.get(i)?;
                        Some(get(&format!("{base}/addresses/{lh}/activities?limit=50")))
                    }),
                    expect_status: 200,
                    risk_tier: RiskTier::High,
                    read_pattern: ReadPattern::RangeScan,
                },
                EndpointEntry {
                    module: "heavy_pages",
                    method: Method::Get,
                    path_template: "/addresses/{addr}/transactions",
                    description: desc_txs,
                    resolve: Box::new(move |base, p| {
                        let lh = p.busiest_lock_hashes.get(i)?;
                        Some(get(&format!("{base}/addresses/{lh}/transactions?limit=50")))
                    }),
                    expect_status: 200,
                    risk_tier: RiskTier::High,
                    read_pattern: ReadPattern::PrefixScan,
                },
            ]
        })
        .collect()
}
