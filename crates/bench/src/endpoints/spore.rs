use crate::registry::*;

pub fn entries() -> Vec<EndpointEntry> {
    vec![
        EndpointEntry {
            module: "spore",
            method: Method::Get,
            path_template: "/spore/clusters",
            description: "List Spore clusters",
            resolve: Box::new(|base, _p| Some(get(&format!("{base}/spore/clusters")))),
            expect_status: 200,
            risk_tier: RiskTier::Medium,
            read_pattern: ReadPattern::FullCfScan,
        },
        EndpointEntry {
            module: "spore",
            method: Method::Get,
            path_template: "/spore/clusters/{cluster_id}",
            description: "Get Spore cluster by ID",
            resolve: Box::new(|base, p| {
                let id = p.cluster_ids.first()?;
                Some(get(&format!("{base}/spore/clusters/{id}")))
            }),
            expect_status: 200,
            risk_tier: RiskTier::Low,
            read_pattern: ReadPattern::KeyLookup,
        },
        EndpointEntry {
            module: "spore",
            method: Method::Get,
            path_template: "/spore/clusters/{cluster_id}/charts/capacity-history",
            description: "Cluster capacity history chart",
            resolve: Box::new(|base, p| {
                let id = p.cluster_ids.first()?;
                Some(get(&format!(
                    "{base}/spore/clusters/{id}/charts/capacity-history"
                )))
            }),
            expect_status: 200,
            risk_tier: RiskTier::Medium,
            read_pattern: ReadPattern::Aggregation,
        },
        EndpointEntry {
            module: "spore",
            method: Method::Get,
            path_template: "/spore/clusters/{cluster_id}/holders",
            description: "List holders of a Spore cluster",
            resolve: Box::new(|base, p| {
                let id = p.cluster_ids.first()?;
                Some(get(&format!("{base}/spore/clusters/{id}/holders?limit=20")))
            }),
            expect_status: 200,
            risk_tier: RiskTier::Medium,
            read_pattern: ReadPattern::PrefixScan,
        },
        EndpointEntry {
            module: "spore",
            method: Method::Get,
            path_template: "/spore/clusters/{cluster_id}/activities",
            description: "List activities for a Spore cluster",
            resolve: Box::new(|base, p| {
                let id = p.cluster_ids.first()?;
                Some(get(&format!(
                    "{base}/spore/clusters/{id}/activities?limit=20"
                )))
            }),
            expect_status: 200,
            risk_tier: RiskTier::Medium,
            read_pattern: ReadPattern::PrefixScan,
        },
        EndpointEntry {
            module: "spore",
            method: Method::Get,
            path_template: "/spore/clusters/{cluster_id}/spores",
            description: "List Spores in a cluster",
            resolve: Box::new(|base, p| {
                let id = p.cluster_ids.first()?;
                Some(get(&format!("{base}/spore/clusters/{id}/spores?limit=20")))
            }),
            expect_status: 200,
            risk_tier: RiskTier::High,
            read_pattern: ReadPattern::PrefixScan,
        },
        EndpointEntry {
            module: "spore",
            method: Method::Get,
            path_template: "/spore/objects",
            description: "List all Spore objects",
            resolve: Box::new(|base, _p| Some(get(&format!("{base}/spore/objects")))),
            expect_status: 200,
            risk_tier: RiskTier::High,
            read_pattern: ReadPattern::FullCfScan,
        },
        EndpointEntry {
            module: "spore",
            method: Method::Get,
            path_template: "/spore/objects/{spore_id}",
            description: "Get Spore object by ID",
            resolve: Box::new(|base, p| {
                let id = p.spore_ids.first()?;
                Some(get(&format!("{base}/spore/objects/{id}")))
            }),
            expect_status: 200,
            risk_tier: RiskTier::Low,
            read_pattern: ReadPattern::KeyLookup,
        },
        EndpointEntry {
            module: "spore",
            method: Method::Get,
            path_template: "/spore/objects/{spore_id}/activities",
            description: "List activities for a Spore object",
            resolve: Box::new(|base, p| {
                let id = p.spore_ids.first()?;
                Some(get(&format!("{base}/spore/objects/{id}/activities")))
            }),
            expect_status: 200,
            risk_tier: RiskTier::Medium,
            read_pattern: ReadPattern::PrefixScan,
        },
        EndpointEntry {
            module: "spore",
            method: Method::Get,
            path_template: "/spore/objects/{spore_id}/decode",
            description: "Decode a Spore object",
            resolve: Box::new(|base, p| {
                let id = p.spore_ids.first()?;
                Some(get(&format!("{base}/spore/objects/{id}/decode")))
            }),
            expect_status: 200,
            risk_tier: RiskTier::Low,
            read_pattern: ReadPattern::KeyLookup,
        },
        EndpointEntry {
            module: "spore",
            method: Method::Get,
            path_template: "/spore/objects/{spore_id}/render",
            description: "Render a Spore object (SVG)",
            resolve: Box::new(|base, p| {
                let id = p.renderable_spore_id.as_ref()?;
                Some(get(&format!("{base}/spore/objects/{id}/render")))
            }),
            expect_status: 200,
            risk_tier: RiskTier::Low,
            read_pattern: ReadPattern::KeyLookup,
        },
        EndpointEntry {
            module: "spore",
            method: Method::Get,
            path_template: "/spore/objects/{spore_id}/charts/capacity-history",
            description: "Spore object capacity history chart",
            resolve: Box::new(|base, p| {
                let id = p.spore_ids.first()?;
                Some(get(&format!(
                    "{base}/spore/objects/{id}/charts/capacity-history"
                )))
            }),
            expect_status: 200,
            risk_tier: RiskTier::Medium,
            read_pattern: ReadPattern::Aggregation,
        },
        EndpointEntry {
            module: "spore",
            method: Method::Get,
            path_template: "/spore/owner/{lock_hash}",
            description: "List Spores owned by lock hash",
            resolve: Box::new(|base, p| {
                let lh = p.top_lock_hashes.first()?;
                Some(get(&format!("{base}/spore/owner/{lh}")))
            }),
            expect_status: 200,
            risk_tier: RiskTier::Medium,
            read_pattern: ReadPattern::PrefixScan,
        },
    ]
}
