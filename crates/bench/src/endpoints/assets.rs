use crate::registry::*;

pub fn entries() -> Vec<EndpointEntry> {
    vec![
        EndpointEntry {
            module: "assets",
            method: Method::Get,
            path_template: "/assets",
            description: "List all asset collections",
            resolve: Box::new(|base, _p| Some(get(&format!("{base}/assets")))),
            expect_status: 200,
            risk_tier: RiskTier::Medium,
            read_pattern: ReadPattern::Cached,
        },
        EndpointEntry {
            module: "assets",
            method: Method::Get,
            path_template: "/assets/objects/items/{object_id}",
            description: "Get object item by ID",
            resolve: Box::new(|base, p| {
                let id = p.object_item_id.as_ref()?;
                Some(get(&format!("{base}/assets/objects/items/{id}")))
            }),
            expect_status: 200,
            risk_tier: RiskTier::Medium,
            read_pattern: ReadPattern::KeyLookup,
        },
        EndpointEntry {
            module: "assets",
            method: Method::Get,
            path_template: "/assets/objects/items/{object_id}/activities",
            description: "List activities for an object item",
            resolve: Box::new(|base, p| {
                let id = p.object_item_id.as_ref()?;
                Some(get(&format!("{base}/assets/objects/items/{id}/activities")))
            }),
            expect_status: 200,
            risk_tier: RiskTier::Medium,
            read_pattern: ReadPattern::PrefixScan,
        },
        EndpointEntry {
            module: "assets",
            method: Method::Get,
            path_template: "/assets/objects/{collection_id}",
            description: "Get object collection by ID",
            resolve: Box::new(|base, p| {
                let id = p.object_collection_id.as_ref()?;
                Some(get(&format!("{base}/assets/objects/{id}")))
            }),
            expect_status: 200,
            risk_tier: RiskTier::Medium,
            read_pattern: ReadPattern::KeyLookup,
        },
        EndpointEntry {
            module: "assets",
            method: Method::Get,
            path_template: "/assets/objects/{collection_id}/items",
            description: "List items in an object collection",
            resolve: Box::new(|base, p| {
                let id = p.object_collection_id.as_ref()?;
                Some(get(&format!("{base}/assets/objects/{id}/items?limit=20")))
            }),
            expect_status: 200,
            risk_tier: RiskTier::Medium,
            read_pattern: ReadPattern::PrefixScan,
        },
        EndpointEntry {
            module: "assets",
            method: Method::Get,
            path_template: "/assets/objects/{collection_id}/holders",
            description: "List holders of an object collection",
            resolve: Box::new(|base, p| {
                let id = p.object_collection_id.as_ref()?;
                Some(get(&format!("{base}/assets/objects/{id}/holders?limit=20")))
            }),
            expect_status: 200,
            risk_tier: RiskTier::Medium,
            read_pattern: ReadPattern::PrefixScan,
        },
        EndpointEntry {
            module: "assets",
            method: Method::Get,
            path_template: "/assets/objects/{collection_id}/activities",
            description: "List activities for an object collection",
            resolve: Box::new(|base, p| {
                let id = p.object_collection_id.as_ref()?;
                Some(get(&format!(
                    "{base}/assets/objects/{id}/activities?limit=20"
                )))
            }),
            expect_status: 200,
            risk_tier: RiskTier::Medium,
            read_pattern: ReadPattern::PrefixScan,
        },
        EndpointEntry {
            module: "assets",
            method: Method::Get,
            path_template: "/assets/objects/{collection_id}/charts/capacity-history",
            description: "Capacity history chart for an object collection",
            resolve: Box::new(|base, p| {
                let id = p.object_collection_id.as_ref()?;
                Some(get(&format!(
                    "{base}/assets/objects/{id}/charts/capacity-history"
                )))
            }),
            expect_status: 200,
            risk_tier: RiskTier::Medium,
            read_pattern: ReadPattern::Aggregation,
        },
    ]
}
