use crate::registry::*;

pub fn entries() -> Vec<EndpointEntry> {
    vec![
        EndpointEntry {
            module: "identities",
            method: Method::Get,
            path_template: "/assets/identities/dotbit/items/{identity_id}",
            description: "Get .bit identity item",
            resolve: Box::new(|base, p| {
                let id = p.dotbit_item_id.as_ref()?;
                Some(get(&format!("{base}/assets/identities/dotbit/items/{id}")))
            }),
            expect_status: 200,
            risk_tier: RiskTier::Medium,
            read_pattern: ReadPattern::KeyLookup,
        },
        EndpointEntry {
            module: "identities",
            method: Method::Get,
            path_template: "/assets/identities/dotbit/items/{identity_id}/activities",
            description: "List activities for a .bit identity",
            resolve: Box::new(|base, p| {
                let id = p.dotbit_item_id.as_ref()?;
                Some(get(&format!(
                    "{base}/assets/identities/dotbit/items/{id}/activities"
                )))
            }),
            expect_status: 200,
            risk_tier: RiskTier::Medium,
            read_pattern: ReadPattern::PrefixScan,
        },
        EndpointEntry {
            module: "identities",
            method: Method::Get,
            path_template: "/assets/identities/did/items/{identity_id}",
            description: "Get did:ckb identity item",
            resolve: Box::new(|base, p| {
                let id = p.did_item_id.as_ref()?;
                Some(get(&format!("{base}/assets/identities/did/items/{id}")))
            }),
            expect_status: 200,
            risk_tier: RiskTier::Medium,
            read_pattern: ReadPattern::KeyLookup,
        },
        EndpointEntry {
            module: "identities",
            method: Method::Get,
            path_template: "/assets/identities/did/items/{identity_id}/activities",
            description: "List activities for a did:ckb identity",
            resolve: Box::new(|base, p| {
                let id = p.did_item_id.as_ref()?;
                Some(get(&format!(
                    "{base}/assets/identities/did/items/{id}/activities"
                )))
            }),
            expect_status: 200,
            risk_tier: RiskTier::Medium,
            read_pattern: ReadPattern::PrefixScan,
        },
        EndpointEntry {
            module: "identities",
            method: Method::Get,
            path_template: "/assets/identities/{collection_id}",
            description: "Get identity collection",
            resolve: Box::new(|base, p| {
                let id = p.identity_collection_id.as_deref().unwrap_or("dotbit");
                Some(get(&format!("{base}/assets/identities/{id}")))
            }),
            expect_status: 200,
            risk_tier: RiskTier::Medium,
            read_pattern: ReadPattern::KeyLookup,
        },
        EndpointEntry {
            module: "identities",
            method: Method::Get,
            path_template: "/assets/identities/{collection_id}/holders",
            description: "List holders of an identity collection",
            resolve: Box::new(|base, p| {
                let id = p.identity_collection_id.as_deref().unwrap_or("dotbit");
                Some(get(&format!(
                    "{base}/assets/identities/{id}/holders?limit=20"
                )))
            }),
            expect_status: 200,
            risk_tier: RiskTier::Medium,
            read_pattern: ReadPattern::PrefixScan,
        },
        EndpointEntry {
            module: "identities",
            method: Method::Get,
            path_template: "/assets/identities/{collection_id}/activities",
            description: "List activities for an identity collection",
            resolve: Box::new(|base, p| {
                let id = p.identity_collection_id.as_deref().unwrap_or("dotbit");
                Some(get(&format!(
                    "{base}/assets/identities/{id}/activities?limit=20"
                )))
            }),
            expect_status: 200,
            risk_tier: RiskTier::Medium,
            read_pattern: ReadPattern::PrefixScan,
        },
        EndpointEntry {
            module: "identities",
            method: Method::Get,
            path_template: "/assets/identities/{collection_id}/items",
            description: "List items in an identity collection",
            resolve: Box::new(|base, p| {
                let id = p.identity_collection_id.as_deref().unwrap_or("dotbit");
                Some(get(&format!(
                    "{base}/assets/identities/{id}/items?limit=20"
                )))
            }),
            expect_status: 200,
            risk_tier: RiskTier::Medium,
            read_pattern: ReadPattern::PrefixScan,
        },
    ]
}
