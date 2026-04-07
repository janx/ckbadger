use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum RiskTier {
    High,
    Medium,
    Low,
}

impl RiskTier {
    pub fn from_str_opt(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "high" => Some(Self::High),
            "medium" => Some(Self::Medium),
            "low" => Some(Self::Low),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum ReadPattern {
    KeyLookup,
    BatchLookup,
    PrefixScan,
    RangeScan,
    FullCfScan,
    CrossStore,
    RpcDependent,
    Cached,
    Aggregation,
}

impl std::fmt::Display for ReadPattern {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::KeyLookup => write!(f, "KeyLookup"),
            Self::BatchLookup => write!(f, "BatchGet"),
            Self::PrefixScan => write!(f, "PrefixScan"),
            Self::RangeScan => write!(f, "RangeScan"),
            Self::FullCfScan => write!(f, "FullScan"),
            Self::CrossStore => write!(f, "CrossStore"),
            Self::RpcDependent => write!(f, "RpcDep"),
            Self::Cached => write!(f, "Cached"),
            Self::Aggregation => write!(f, "Aggregation"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ResolvedRequest {
    pub url: String,
    pub method: Method,
    pub body: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Method {
    Get,
    Post,
}

impl std::fmt::Display for Method {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Get => write!(f, "GET"),
            Self::Post => write!(f, "POST"),
        }
    }
}

pub fn get(url: &str) -> ResolvedRequest {
    ResolvedRequest {
        url: url.to_string(),
        method: Method::Get,
        body: None,
    }
}

pub fn post(url: &str, body: &str) -> ResolvedRequest {
    ResolvedRequest {
        url: url.to_string(),
        method: Method::Post,
        body: Some(body.to_string()),
    }
}

pub struct EndpointEntry {
    pub module: &'static str,
    pub method: Method,
    pub path_template: &'static str,
    pub description: &'static str,
    #[allow(clippy::type_complexity)]
    pub resolve: Box<dyn Fn(&str, &DiscoveredParams) -> Option<ResolvedRequest> + Send + Sync>,
    pub expect_status: u16,
    pub risk_tier: RiskTier,
    pub read_pattern: ReadPattern,
}

#[derive(Debug, Clone, Default)]
pub struct DiscoveredParams {
    pub sync_tip: u64,
    pub latest_block_number: u64,
    pub latest_block_hash: String,
    pub mid_block_number: u64,
    pub tx_hashes: Vec<String>,
    pub complex_tx_hash: Option<String>,
    pub top_addresses: Vec<String>,
    pub top_lock_hashes: Vec<String>,
    pub dao_lock_hashes: Vec<String>,
    pub dao_deposit_outpoint: Option<(String, u32)>,
    pub dao_deposit_capacity: Option<String>,
    pub dao_deposit_block: Option<i64>,
    pub token_type_hashes: Vec<String>,
    pub cluster_ids: Vec<String>,
    pub spore_ids: Vec<String>,
    pub renderable_spore_id: Option<String>,
    pub script_names: Vec<String>,
    pub live_cell_outpoint: Option<(String, u32)>,
    pub fiber_channel_id: Option<String>,
    pub dotbit_item_id: Option<String>,
    pub did_item_id: Option<String>,
    pub object_collection_id: Option<String>,
    pub object_item_id: Option<String>,
    pub identity_collection_id: Option<String>,
    pub fork_id: Option<String>,
    // Heavy-page discovery: top-10 items by data volume
    pub top_script_names: Vec<String>,
    pub top_token_type_hashes: Vec<String>,
    pub top_spore_ids: Vec<String>,
    pub top_cluster_ids: Vec<String>,
    pub top_dotbit_item_ids: Vec<String>,
    pub busiest_lock_hashes: Vec<String>,
}

pub struct Registry {
    pub entries: Vec<EndpointEntry>,
}

impl Registry {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    pub fn add(&mut self, entry: EndpointEntry) {
        self.entries.push(entry);
    }

    pub fn filter_module(&mut self, module: &str) {
        self.entries.retain(|e| e.module == module);
    }

    pub fn filter_endpoint(&mut self, template: &str) {
        self.entries.retain(|e| e.path_template == template);
    }

    pub fn filter_risk(&mut self, tier: RiskTier) {
        self.entries.retain(|e| e.risk_tier == tier);
    }

    pub fn sort_by_risk(&mut self) {
        self.entries.sort_by_key(|e| match e.risk_tier {
            RiskTier::Low => 0,
            RiskTier::Medium => 1,
            RiskTier::High => 2,
        });
    }
}
