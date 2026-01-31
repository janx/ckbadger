use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ActivityType {
    CkbTransfer,
    CellbaseReward,
    TokenMint,
    TokenTransfer,
    TokenBurn,
    DobMint,
    DobTransfer,
    DobBurn,
    NftMint,
    NftTransfer,
    DaoDeposit,
    DaoWithdrawRequest,
    DaoWithdrawComplete,
    ScriptDeploy,
    RgbppTransfer,
    RgbppLeapIn,
    RgbppLeapOut,
    RgbppIssuance,
}

impl ActivityType {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::CkbTransfer => "CKB_TRANSFER",
            Self::CellbaseReward => "CELLBASE_REWARD",
            Self::TokenMint => "TOKEN_MINT",
            Self::TokenTransfer => "TOKEN_TRANSFER",
            Self::TokenBurn => "TOKEN_BURN",
            Self::DobMint => "DOB_MINT",
            Self::DobTransfer => "DOB_TRANSFER",
            Self::DobBurn => "DOB_BURN",
            Self::NftMint => "NFT_MINT",
            Self::NftTransfer => "NFT_TRANSFER",
            Self::DaoDeposit => "DAO_DEPOSIT",
            Self::DaoWithdrawRequest => "DAO_WITHDRAW_REQUEST",
            Self::DaoWithdrawComplete => "DAO_WITHDRAW_COMPLETE",
            Self::ScriptDeploy => "SCRIPT_DEPLOY",
            Self::RgbppTransfer => "RGBPP_TRANSFER",
            Self::RgbppLeapIn => "RGBPP_LEAP_IN",
            Self::RgbppLeapOut => "RGBPP_LEAP_OUT",
            Self::RgbppIssuance => "RGBPP_ISSUANCE",
        }
    }

    pub fn category(&self) -> ActivityCategory {
        match self {
            Self::CkbTransfer => ActivityCategory::Ckb,
            Self::CellbaseReward => ActivityCategory::Cellbase,
            Self::TokenMint | Self::TokenTransfer | Self::TokenBurn => ActivityCategory::Token,
            Self::DobMint | Self::DobTransfer | Self::DobBurn => ActivityCategory::Dob,
            Self::NftMint | Self::NftTransfer => ActivityCategory::Nft,
            Self::DaoDeposit | Self::DaoWithdrawRequest | Self::DaoWithdrawComplete => {
                ActivityCategory::Dao
            }
            Self::ScriptDeploy => ActivityCategory::Script,
            Self::RgbppTransfer | Self::RgbppLeapIn | Self::RgbppLeapOut | Self::RgbppIssuance => {
                ActivityCategory::Rgbpp
            }
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "CKB_TRANSFER" => Some(Self::CkbTransfer),
            "CELLBASE_REWARD" => Some(Self::CellbaseReward),
            "TOKEN_MINT" => Some(Self::TokenMint),
            "TOKEN_TRANSFER" => Some(Self::TokenTransfer),
            "TOKEN_BURN" => Some(Self::TokenBurn),
            "DOB_MINT" => Some(Self::DobMint),
            "DOB_TRANSFER" => Some(Self::DobTransfer),
            "DOB_BURN" => Some(Self::DobBurn),
            "NFT_MINT" => Some(Self::NftMint),
            "NFT_TRANSFER" => Some(Self::NftTransfer),
            "DAO_DEPOSIT" => Some(Self::DaoDeposit),
            "DAO_WITHDRAW_REQUEST" => Some(Self::DaoWithdrawRequest),
            "DAO_WITHDRAW_COMPLETE" => Some(Self::DaoWithdrawComplete),
            "SCRIPT_DEPLOY" => Some(Self::ScriptDeploy),
            "RGBPP_TRANSFER" => Some(Self::RgbppTransfer),
            "RGBPP_LEAP_IN" => Some(Self::RgbppLeapIn),
            "RGBPP_LEAP_OUT" => Some(Self::RgbppLeapOut),
            "RGBPP_ISSUANCE" => Some(Self::RgbppIssuance),
            _ => None,
        }
    }
}

impl fmt::Display for ActivityType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ActivityCategory {
    Ckb,
    Cellbase,
    Token,
    Dob,
    Nft,
    Dao,
    Script,
    Rgbpp,
}

impl ActivityCategory {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Ckb => "ckb",
            Self::Cellbase => "cellbase",
            Self::Token => "token",
            Self::Dob => "dob",
            Self::Nft => "nft",
            Self::Dao => "dao",
            Self::Script => "script",
            Self::Rgbpp => "rgbpp",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "ckb" => Some(Self::Ckb),
            "cellbase" => Some(Self::Cellbase),
            "token" => Some(Self::Token),
            "dob" => Some(Self::Dob),
            "nft" => Some(Self::Nft),
            "dao" => Some(Self::Dao),
            "script" => Some(Self::Script),
            "rgbpp" => Some(Self::Rgbpp),
            _ => None,
        }
    }
}

impl fmt::Display for ActivityCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Activity {
    pub id: i64,
    pub activity_id: Vec<u8>,
    pub activity_type: ActivityType,
    pub activity_category: ActivityCategory,
    pub block_number: i64,
    pub tx_hash: Vec<u8>,
    pub tx_index: i32,
    pub activity_index: i16,
    pub from_lock_hash: Option<Vec<u8>>,
    pub to_lock_hash: Option<Vec<u8>>,
    pub amount: String,
    pub asset_id: Option<Vec<u8>>,
    pub metadata: serde_json::Value,
    pub timestamp: DateTime<Utc>,
}

impl Activity {
    pub fn tx_hash_hex(&self) -> String {
        format!("0x{}", hex::encode(&self.tx_hash))
    }

    pub fn activity_id_hex(&self) -> String {
        format!("0x{}", hex::encode(&self.activity_id))
    }

    pub fn from_lock_hash_hex(&self) -> Option<String> {
        self.from_lock_hash
            .as_ref()
            .map(|h| format!("0x{}", hex::encode(h)))
    }

    pub fn to_lock_hash_hex(&self) -> Option<String> {
        self.to_lock_hash
            .as_ref()
            .map(|h| format!("0x{}", hex::encode(h)))
    }

    pub fn asset_id_hex(&self) -> Option<String> {
        self.asset_id
            .as_ref()
            .map(|h| format!("0x{}", hex::encode(h)))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum ActivityMetadata {
    #[serde(rename_all = "camelCase")]
    CkbTransfer {},

    #[serde(rename_all = "camelCase")]
    CellbaseReward {
        total_reward: String,
        block_reward: String,
        proposal_reward: String,
    },

    #[serde(rename_all = "camelCase")]
    Token {
        symbol: Option<String>,
        decimals: u8,
        token_type_hash: String,
    },

    #[serde(rename_all = "camelCase")]
    Dob {
        cluster_id: Option<String>,
        content_type: String,
        spore_id: String,
    },

    #[serde(rename_all = "camelCase")]
    Nft {
        nft_type: String,
        nft_id: String,
        name: Option<String>,
    },

    #[serde(rename_all = "camelCase")]
    Dao {
        deposit_ar: Option<String>,
        withdraw_ar: Option<String>,
        compensation: Option<String>,
    },

    #[serde(rename_all = "camelCase")]
    Script { code_hash: String, data_size: u64 },

    #[serde(rename_all = "camelCase")]
    Rgbpp {
        btc_txid: Option<String>,
        commitment: Option<String>,
        asset_id: Option<String>,
    },
}

impl ActivityMetadata {
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::to_value(self).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_activity_type_serde() {
        let activity_type = ActivityType::CkbTransfer;
        let json = serde_json::to_string(&activity_type).unwrap();
        assert_eq!(json, "\"CKB_TRANSFER\"");

        let parsed: ActivityType = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, ActivityType::CkbTransfer);
    }

    #[test]
    fn test_activity_type_as_str() {
        assert_eq!(ActivityType::CkbTransfer.as_str(), "CKB_TRANSFER");
        assert_eq!(ActivityType::TokenMint.as_str(), "TOKEN_MINT");
        assert_eq!(ActivityType::DaoDeposit.as_str(), "DAO_DEPOSIT");
        assert_eq!(ActivityType::RgbppLeapIn.as_str(), "RGBPP_LEAP_IN");
    }

    #[test]
    fn test_activity_type_parse() {
        assert_eq!(
            ActivityType::parse("CKB_TRANSFER"),
            Some(ActivityType::CkbTransfer)
        );
        assert_eq!(
            ActivityType::parse("TOKEN_MINT"),
            Some(ActivityType::TokenMint)
        );
        assert_eq!(ActivityType::parse("INVALID"), None);
    }

    #[test]
    fn test_activity_type_category() {
        assert_eq!(ActivityType::CkbTransfer.category(), ActivityCategory::Ckb);
        assert_eq!(
            ActivityType::CellbaseReward.category(),
            ActivityCategory::Cellbase
        );
        assert_eq!(ActivityType::TokenMint.category(), ActivityCategory::Token);
        assert_eq!(
            ActivityType::TokenTransfer.category(),
            ActivityCategory::Token
        );
        assert_eq!(ActivityType::TokenBurn.category(), ActivityCategory::Token);
        assert_eq!(ActivityType::DobMint.category(), ActivityCategory::Dob);
        assert_eq!(ActivityType::NftMint.category(), ActivityCategory::Nft);
        assert_eq!(ActivityType::DaoDeposit.category(), ActivityCategory::Dao);
        assert_eq!(
            ActivityType::ScriptDeploy.category(),
            ActivityCategory::Script
        );
        assert_eq!(
            ActivityType::RgbppTransfer.category(),
            ActivityCategory::Rgbpp
        );
    }

    #[test]
    fn test_activity_category_serde() {
        let category = ActivityCategory::Token;
        let json = serde_json::to_string(&category).unwrap();
        assert_eq!(json, "\"token\"");

        let parsed: ActivityCategory = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, ActivityCategory::Token);
    }

    #[test]
    fn test_activity_category_as_str() {
        assert_eq!(ActivityCategory::Ckb.as_str(), "ckb");
        assert_eq!(ActivityCategory::Token.as_str(), "token");
        assert_eq!(ActivityCategory::Rgbpp.as_str(), "rgbpp");
    }

    #[test]
    fn test_activity_metadata_serde() {
        let metadata = ActivityMetadata::Token {
            symbol: Some("SEAL".to_string()),
            decimals: 8,
            token_type_hash: "0x123".to_string(),
        };
        let json = metadata.to_json();
        assert_eq!(json["type"], "token");
        assert_eq!(json["symbol"], "SEAL");
        assert_eq!(json["decimals"], 8);
    }

    #[test]
    fn test_activity_display() {
        assert_eq!(format!("{}", ActivityType::CkbTransfer), "CKB_TRANSFER");
        assert_eq!(format!("{}", ActivityCategory::Token), "token");
    }

    #[test]
    fn test_all_activity_types_have_categories() {
        let all_types = [
            ActivityType::CkbTransfer,
            ActivityType::CellbaseReward,
            ActivityType::TokenMint,
            ActivityType::TokenTransfer,
            ActivityType::TokenBurn,
            ActivityType::DobMint,
            ActivityType::DobTransfer,
            ActivityType::DobBurn,
            ActivityType::NftMint,
            ActivityType::NftTransfer,
            ActivityType::DaoDeposit,
            ActivityType::DaoWithdrawRequest,
            ActivityType::DaoWithdrawComplete,
            ActivityType::ScriptDeploy,
            ActivityType::RgbppTransfer,
            ActivityType::RgbppLeapIn,
            ActivityType::RgbppLeapOut,
            ActivityType::RgbppIssuance,
        ];

        for activity_type in all_types {
            let category = activity_type.category();
            assert!(
                !category.as_str().is_empty(),
                "Activity {:?} should have a valid category",
                activity_type
            );
        }
    }

    #[test]
    fn test_activity_type_roundtrip() {
        let all_types = [
            ActivityType::CkbTransfer,
            ActivityType::CellbaseReward,
            ActivityType::TokenMint,
            ActivityType::TokenTransfer,
            ActivityType::TokenBurn,
            ActivityType::DobMint,
            ActivityType::DobTransfer,
            ActivityType::DobBurn,
            ActivityType::NftMint,
            ActivityType::NftTransfer,
            ActivityType::DaoDeposit,
            ActivityType::DaoWithdrawRequest,
            ActivityType::DaoWithdrawComplete,
            ActivityType::ScriptDeploy,
            ActivityType::RgbppTransfer,
            ActivityType::RgbppLeapIn,
            ActivityType::RgbppLeapOut,
            ActivityType::RgbppIssuance,
        ];

        for activity_type in all_types {
            let s = activity_type.as_str();
            let parsed = ActivityType::parse(s);
            assert_eq!(
                parsed,
                Some(activity_type),
                "Round-trip failed for {:?}",
                activity_type
            );
        }
    }
}
