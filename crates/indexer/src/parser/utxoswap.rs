// UTXOSwap Intent Lock (lock script)
pub const INTENT_LOCK_CODE_HASH_MAINNET: &str =
    "0x3547c9aa563804e47ba3ebd37e6012e447c91a238f7aa71b1a75319f11df060e";
pub const INTENT_LOCK_CODE_HASH_TESTNET: &str =
    "0x4e9c30c8d6ce275740fbe69eae49c3d8c213578c5bd066f4938fe3c7dec6e101";

pub fn is_intent_lock(code_hash: &[u8]) -> bool {
    crate::parser::registry::PROTOCOL_REGISTRY.is(
        code_hash,
        crate::parser::registry::ProtocolScript::UtxoSwapIntent,
    )
}

// --- Intent args parsing ---

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum IntentType {
    CreatePool = 0,
    AddLiquidity = 1,
    RemoveLiquidity = 2,
    SwapExactInputForOutput = 3,
    SwapInputForExactOutput = 4,
    ClaimProtocolLiquidity = 5,
}

impl IntentType {
    pub fn from_byte(b: u8) -> Option<Self> {
        match b {
            0 => Some(Self::CreatePool),
            1 => Some(Self::AddLiquidity),
            2 => Some(Self::RemoveLiquidity),
            3 => Some(Self::SwapExactInputForOutput),
            4 => Some(Self::SwapInputForExactOutput),
            5 => Some(Self::ClaimProtocolLiquidity),
            _ => None,
        }
    }

    pub fn action_name(&self) -> &'static str {
        match self {
            Self::CreatePool => "create_pool",
            Self::AddLiquidity => "add_liquidity",
            Self::RemoveLiquidity => "remove_liquidity",
            Self::SwapExactInputForOutput => "swap_exact_input",
            Self::SwapInputForExactOutput => "swap_exact_output",
            Self::ClaimProtocolLiquidity => "claim_protocol_liquidity",
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            Self::CreatePool => "CreatePool",
            Self::AddLiquidity => "AddLiquidity",
            Self::RemoveLiquidity => "RemoveLiquidity",
            Self::SwapExactInputForOutput => "SwapExactInputForOutput",
            Self::SwapInputForExactOutput => "SwapInputForExactOutput",
            Self::ClaimProtocolLiquidity => "ClaimProtocolLiquidity",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreatePoolExtra {
    pub total_fee_rate: u8,
    pub asset_x: [u8; 32],
    pub asset_y: [u8; 32],
    pub amount_x: u128,
    pub amount_y: u128,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedIntentArgs {
    pub owner_lock_hash: [u8; 20],
    pub pool_type_hash: [u8; 20],
    pub intent_type: IntentType,
    pub asset_in_index: u8,
    pub amount_in: u128,
    pub amount_out_min: u128,
    pub create_pool_extra: Option<CreatePoolExtra>,
}

/// Parses UTXOSwap intent lock args.
///
/// Layout:
///   [0..20]   owner_lock_hash
///   [20..40]  pool_type_hash
///   [40..48]  tx_fee (skipped)
///   [48..56]  expire_batch_id (skipped)
///   [56]      intent_type
///
/// Non-CreatePool (minimum 90 bytes):
///   [57]      asset_in_index
///   [58..74]  amount_in (u128 LE)
///   [74..90]  amount_out_min (u128 LE)
///
/// CreatePool (minimum 154 bytes):
///   [57]      total_fee_rate
///   [58..90]  asset_x
///   [90..122] asset_y
///   [122..138] amount_x (u128 LE)
///   [138..154] amount_y (u128 LE)
pub fn parse_intent_args(args: &[u8]) -> Option<ParsedIntentArgs> {
    // Need at least 57 bytes to read intent_type
    if args.len() < 57 {
        return None;
    }

    let mut owner_lock_hash = [0u8; 20];
    owner_lock_hash.copy_from_slice(&args[0..20]);

    let mut pool_type_hash = [0u8; 20];
    pool_type_hash.copy_from_slice(&args[20..40]);

    // [40..48] tx_fee — skipped
    // [48..56] expire_batch_id — skipped

    let intent_type = IntentType::from_byte(args[56])?;

    match intent_type {
        IntentType::CreatePool => {
            if args.len() < 154 {
                return None;
            }

            let total_fee_rate = args[57];

            let mut asset_x = [0u8; 32];
            asset_x.copy_from_slice(&args[58..90]);

            let mut asset_y = [0u8; 32];
            asset_y.copy_from_slice(&args[90..122]);

            let amount_x = u128::from_le_bytes(args[122..138].try_into().unwrap());
            let amount_y = u128::from_le_bytes(args[138..154].try_into().unwrap());

            Some(ParsedIntentArgs {
                owner_lock_hash,
                pool_type_hash,
                intent_type,
                asset_in_index: 0,
                amount_in: 0,
                amount_out_min: 0,
                create_pool_extra: Some(CreatePoolExtra {
                    total_fee_rate,
                    asset_x,
                    asset_y,
                    amount_x,
                    amount_y,
                }),
            })
        }
        _ => {
            if args.len() < 90 {
                return None;
            }

            let asset_in_index = args[57];
            let amount_in = u128::from_le_bytes(args[58..74].try_into().unwrap());
            let amount_out_min = u128::from_le_bytes(args[74..90].try_into().unwrap());

            Some(ParsedIntentArgs {
                owner_lock_hash,
                pool_type_hash,
                intent_type,
                asset_in_index,
                amount_in,
                amount_out_min,
                create_pool_extra: None,
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rpc::parse_hex_to_bytes;

    // --- is_intent_lock tests ---

    #[test]
    fn test_is_intent_lock_mainnet() {
        let code_hash = parse_hex_to_bytes(INTENT_LOCK_CODE_HASH_MAINNET);
        assert!(is_intent_lock(&code_hash));
    }

    #[test]
    fn test_is_intent_lock_testnet() {
        let code_hash = parse_hex_to_bytes(INTENT_LOCK_CODE_HASH_TESTNET);
        assert!(is_intent_lock(&code_hash));
    }

    #[test]
    fn test_is_intent_lock_rejects_other() {
        let code_hash = vec![0xAA; 32];
        assert!(!is_intent_lock(&code_hash));

        let zero = vec![0u8; 32];
        assert!(!is_intent_lock(&zero));
    }

    #[test]
    fn test_all_hashes_are_32_bytes() {
        assert_eq!(parse_hex_to_bytes(INTENT_LOCK_CODE_HASH_MAINNET).len(), 32);
        assert_eq!(parse_hex_to_bytes(INTENT_LOCK_CODE_HASH_TESTNET).len(), 32);
    }

    #[test]
    fn test_hashes_are_distinct() {
        let mainnet = parse_hex_to_bytes(INTENT_LOCK_CODE_HASH_MAINNET);
        let testnet = parse_hex_to_bytes(INTENT_LOCK_CODE_HASH_TESTNET);
        assert_ne!(
            mainnet, testnet,
            "mainnet and testnet hashes must be distinct"
        );
    }

    // --- IntentType tests ---

    #[test]
    fn test_intent_type_action_names() {
        assert_eq!(IntentType::CreatePool.action_name(), "create_pool");
        assert_eq!(IntentType::AddLiquidity.action_name(), "add_liquidity");
        assert_eq!(
            IntentType::RemoveLiquidity.action_name(),
            "remove_liquidity"
        );
        assert_eq!(
            IntentType::SwapExactInputForOutput.action_name(),
            "swap_exact_input"
        );
        assert_eq!(
            IntentType::SwapInputForExactOutput.action_name(),
            "swap_exact_output"
        );
        assert_eq!(
            IntentType::ClaimProtocolLiquidity.action_name(),
            "claim_protocol_liquidity"
        );

        assert_eq!(IntentType::CreatePool.display_name(), "CreatePool");
        assert_eq!(IntentType::AddLiquidity.display_name(), "AddLiquidity");
        assert_eq!(
            IntentType::RemoveLiquidity.display_name(),
            "RemoveLiquidity"
        );
        assert_eq!(
            IntentType::SwapExactInputForOutput.display_name(),
            "SwapExactInputForOutput"
        );
        assert_eq!(
            IntentType::SwapInputForExactOutput.display_name(),
            "SwapInputForExactOutput"
        );
        assert_eq!(
            IntentType::ClaimProtocolLiquidity.display_name(),
            "ClaimProtocolLiquidity"
        );
    }

    #[test]
    fn test_all_intent_types_roundtrip() {
        for byte in 0..=5u8 {
            let intent_type = IntentType::from_byte(byte).unwrap();
            assert_eq!(intent_type as u8, byte);
        }
        // 6 and above should return None
        assert!(IntentType::from_byte(6).is_none());
        assert!(IntentType::from_byte(255).is_none());
    }

    // --- parse_intent_args tests ---

    /// Helper: builds a non-CreatePool intent args buffer.
    fn build_non_create_args(
        intent_type: u8,
        asset_in_index: u8,
        amount_in: u128,
        amount_out_min: u128,
    ) -> Vec<u8> {
        let mut buf = vec![0u8; 90];
        // owner_lock_hash [0..20]
        for (i, b) in buf[..20].iter_mut().enumerate() {
            *b = (i as u8) + 1;
        }
        // pool_type_hash [20..40]
        for (i, b) in buf[20..40].iter_mut().enumerate() {
            *b = (i as u8) + 0x80;
        }
        // tx_fee [40..48] — skip (zeros)
        // expire_batch_id [48..56] — skip (zeros)
        buf[56] = intent_type;
        buf[57] = asset_in_index;
        buf[58..74].copy_from_slice(&amount_in.to_le_bytes());
        buf[74..90].copy_from_slice(&amount_out_min.to_le_bytes());
        buf
    }

    /// Helper: builds a CreatePool intent args buffer.
    fn build_create_pool_args(
        total_fee_rate: u8,
        asset_x: [u8; 32],
        asset_y: [u8; 32],
        amount_x: u128,
        amount_y: u128,
    ) -> Vec<u8> {
        let mut buf = vec![0u8; 154];
        // owner_lock_hash [0..20]
        for (i, b) in buf[..20].iter_mut().enumerate() {
            *b = (i as u8) + 1;
        }
        // pool_type_hash [20..40]
        for (i, b) in buf[20..40].iter_mut().enumerate() {
            *b = (i as u8) + 0x80;
        }
        // tx_fee [40..48] — skip (zeros)
        // expire_batch_id [48..56] — skip (zeros)
        buf[56] = 0; // CreatePool
        buf[57] = total_fee_rate;
        buf[58..90].copy_from_slice(&asset_x);
        buf[90..122].copy_from_slice(&asset_y);
        buf[122..138].copy_from_slice(&amount_x.to_le_bytes());
        buf[138..154].copy_from_slice(&amount_y.to_le_bytes());
        buf
    }

    #[test]
    fn test_parse_swap_intent_args() {
        let args = build_non_create_args(3, 1, 1000, 900);
        let parsed = parse_intent_args(&args).unwrap();

        assert_eq!(parsed.intent_type, IntentType::SwapExactInputForOutput);
        assert_eq!(parsed.asset_in_index, 1);
        assert_eq!(parsed.amount_in, 1000);
        assert_eq!(parsed.amount_out_min, 900);
        assert!(parsed.create_pool_extra.is_none());

        // Verify owner_lock_hash
        let mut expected_owner = [0u8; 20];
        for (i, b) in expected_owner.iter_mut().enumerate() {
            *b = (i as u8) + 1;
        }
        assert_eq!(parsed.owner_lock_hash, expected_owner);

        // Verify pool_type_hash
        let mut expected_pool = [0u8; 20];
        for (i, b) in expected_pool.iter_mut().enumerate() {
            *b = (i as u8) + 0x80;
        }
        assert_eq!(parsed.pool_type_hash, expected_pool);
    }

    #[test]
    fn test_parse_add_liquidity_args() {
        let args = build_non_create_args(1, 0, 5000, 4500);
        let parsed = parse_intent_args(&args).unwrap();

        assert_eq!(parsed.intent_type, IntentType::AddLiquidity);
        assert_eq!(parsed.asset_in_index, 0);
        assert_eq!(parsed.amount_in, 5000);
        assert_eq!(parsed.amount_out_min, 4500);
        assert!(parsed.create_pool_extra.is_none());
    }

    #[test]
    fn test_parse_create_pool_args() {
        let asset_x = [0xAA; 32];
        let asset_y = [0xBB; 32];
        let args = build_create_pool_args(30, asset_x, asset_y, 10000, 20000);
        let parsed = parse_intent_args(&args).unwrap();

        assert_eq!(parsed.intent_type, IntentType::CreatePool);
        assert_eq!(parsed.asset_in_index, 0);
        assert_eq!(parsed.amount_in, 0);
        assert_eq!(parsed.amount_out_min, 0);

        let extra = parsed.create_pool_extra.unwrap();
        assert_eq!(extra.total_fee_rate, 30);
        assert_eq!(extra.asset_x, asset_x);
        assert_eq!(extra.asset_y, asset_y);
        assert_eq!(extra.amount_x, 10000);
        assert_eq!(extra.amount_y, 20000);
    }

    #[test]
    fn test_parse_intent_args_too_short() {
        // Less than 57 bytes — can't even read intent_type
        assert!(parse_intent_args(&[0u8; 56]).is_none());
        assert!(parse_intent_args(&[]).is_none());

        // 57 bytes with a non-CreatePool type but missing the rest (need 90)
        let mut buf = vec![0u8; 57];
        buf[56] = 3; // SwapExactInputForOutput
        assert!(parse_intent_args(&buf).is_none());

        // 89 bytes — one byte short for non-CreatePool
        let mut buf = vec![0u8; 89];
        buf[56] = 3;
        assert!(parse_intent_args(&buf).is_none());
    }

    #[test]
    fn test_parse_intent_args_create_pool_too_short() {
        // 90 bytes with CreatePool type — need 154
        let mut buf = vec![0u8; 90];
        buf[56] = 0; // CreatePool
        assert!(parse_intent_args(&buf).is_none());

        // 153 bytes — one byte short for CreatePool
        let mut buf = vec![0u8; 153];
        buf[56] = 0;
        assert!(parse_intent_args(&buf).is_none());
    }

    #[test]
    fn test_parse_intent_args_unknown_type() {
        let mut buf = vec![0u8; 90];
        buf[56] = 6; // unknown
        assert!(parse_intent_args(&buf).is_none());

        buf[56] = 255;
        assert!(parse_intent_args(&buf).is_none());
    }
}
