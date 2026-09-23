//! Real `.cell` (DotCell) transactions, fetched 2026-09-24 from the local
//! nodes (mainnet `127.0.0.1:8114`, testnet `127.0.0.1:8124`) with
//! `get_transaction` plus `get_live_cell`/`get_transaction` for every input's
//! previous output.
//!
//! Every byte below is chain data: output cell data, the `WitnessArgs`
//! witnesses (the one at a name cell's own output index carries the records
//! payload whose blake2b is `data[1..33]`), Sale Lock script args and the
//! resolved input cells. Nothing is constructed from the parser's own
//! assumptions — a fixture built the same way as the code under test asserts
//! only self-consistency (POSTMORTEM PROTO-005).
//!
//! Fixture ids match the design plan's table: M1..M5 mainnet, T1..T9 testnet.

use crate::rpc::{CellInput, CellOutput, OutPoint, Script, TransactionView};

/// docs/metadata/scripts/dotcell-account.toml `[mainnet]`.
pub const ACCOUNT_TYPE_CODE_HASH_MAINNET: &str =
    "0xd96cee56727a2bb9a21408c154d278df5095fb4b4dcfd50516156424479bfe54";
/// docs/metadata/scripts/dotcell-account.toml `[testnet]`.
pub const ACCOUNT_TYPE_CODE_HASH_TESTNET: &str =
    "0xe0706b176678181d982290d93dfcd82098e60cceaa4a87f10f32dcbcc91df1d9";
pub const ACCOUNT_LOCK_CODE_HASH_MAINNET: &str =
    "0x9f0f0ba142b58cba2fe047546cfd8481d5b1769437cd3533e6458b21b61871ab";
pub const ACCOUNT_LOCK_CODE_HASH_TESTNET: &str =
    "0xede6a3d80717c3d7927eea678d095abbe68dbb08ca6fdbbbdd9de906455a4afd";
pub const SALE_LOCK_CODE_HASH_MAINNET: &str =
    "0x086c8f4e9d4272e3dfbaca399792f730e6604591e87931ee6d67047a3c900879";
pub const SALE_LOCK_CODE_HASH_TESTNET: &str =
    "0x498ab6b49b6b25b3c47fcea74bd8a4447bc4efda6417809152a846e058ad0ae4";
pub const PRICE_TYPE_CODE_HASH_MAINNET: &str =
    "0x97bf5f760cf72f918f13704d7184933b79d4ddc1fd85075762373e531152d4f9";
pub const PRICE_TYPE_CODE_HASH_TESTNET: &str =
    "0xe1057caf161b3c720fcdb80190e89c6efc36b6b4256b3c99635fda63a9dd4294";

/// The one namespace each network's Cells Account deployment uses (the type
/// script's constant 20-byte args).
pub const NAMESPACE_ARGS_MAINNET: &str = "0xb4f4302965b7d6421481a520ee7eb5971a5e808c";
pub const NAMESPACE_ARGS_TESTNET: &str = "0x2510c78057479c9b023fe6e98ce43979e92a1353";

/// One cell of a captured transaction: the RPC `CellOutput` fields plus its
/// data, exactly as the node served them.
pub struct CellFixture {
    pub capacity: &'static str,
    pub lock_code_hash: &'static str,
    pub lock_hash_type: &'static str,
    pub lock_args: &'static str,
    pub type_code_hash: Option<&'static str>,
    pub type_hash_type: Option<&'static str>,
    pub type_args: Option<&'static str>,
    pub data: &'static str,
}

impl CellFixture {
    pub fn lock_script(&self) -> Script {
        Script {
            code_hash: self.lock_code_hash.to_string(),
            hash_type: self.lock_hash_type.to_string(),
            args: self.lock_args.to_string(),
        }
    }

    pub fn type_script(&self) -> Option<Script> {
        let code_hash = self.type_code_hash?;
        Some(Script {
            code_hash: code_hash.to_string(),
            hash_type: self.type_hash_type.expect("type hash_type").to_string(),
            args: self.type_args.expect("type args").to_string(),
        })
    }

    pub fn output(&self) -> CellOutput {
        CellOutput {
            capacity: self.capacity.to_string(),
            lock: self.lock_script(),
            type_: self.type_script(),
        }
    }

    /// The cell's lock script hash, computed the way the chain computes it.
    pub fn lock_script_hash(&self) -> Vec<u8> {
        crate::parser::ScriptParser::compute_script_hash(&self.lock_script())
    }

    /// The cell as an RPC `CellOutput` plus its data hex, the pair the live
    /// parser entry points take.
    pub fn cell(&self) -> (CellOutput, &'static str) {
        (self.output(), self.data)
    }
}

/// One captured transaction: its outputs with data, its witnesses, and every
/// input's resolved previous output.
pub struct TxFixture {
    pub label: &'static str,
    pub network: &'static str,
    pub tx_hash: &'static str,
    pub block_number: i64,
    pub block_hash: &'static str,
    pub outputs: &'static [CellFixture],
    pub witnesses: &'static [&'static str],
    pub inputs: &'static [CellFixture],
    /// `(previous tx hash, previous output index)` per input, in input order.
    pub input_outpoints: &'static [(&'static str, u32)],
}

impl TxFixture {
    /// The transaction as the live sync path sees it. `cell_deps` and
    /// `header_deps` are left empty: no `.cell` classification reads them (a
    /// sub-name's parent is derived from its own label, spec §1.2).
    pub fn transaction(&self) -> TransactionView {
        TransactionView {
            hash: self.tx_hash.to_string(),
            version: "0x0".to_string(),
            cell_deps: Vec::new(),
            header_deps: Vec::new(),
            inputs: self
                .input_outpoints
                .iter()
                .map(|(tx_hash, index)| CellInput {
                    since: "0x0".to_string(),
                    previous_output: OutPoint {
                        tx_hash: (*tx_hash).to_string(),
                        index: format!("0x{index:x}"),
                    },
                })
                .collect(),
            outputs: self.outputs.iter().map(CellFixture::output).collect(),
            outputs_data: self
                .outputs
                .iter()
                .map(|cell| cell.data.to_string())
                .collect(),
            witnesses: self.witnesses.iter().map(|w| (*w).to_string()).collect(),
        }
    }
}

/// `M1_ring_root` — mainnet tx `0x219d1540e4dc06fd877a5094b93c5d5d302221b31fb972a4ff3d04f320f315ed`
/// at block 20515882. 2026-09-24 from local node http://127.0.0.1:8114.
pub const M1_RING_ROOT: TxFixture = TxFixture {
    label: "M1_ring_root",
    network: "mainnet",
    tx_hash: "0x219d1540e4dc06fd877a5094b93c5d5d302221b31fb972a4ff3d04f320f315ed",
    block_number: 20515882,
    block_hash: "0xb6d051974c3ead3ffc283f30b117d184f0fa79a8828917154dc39fa2921bb296",
    outputs: &[
        CellFixture {
            capacity: "0x47868c000",
            lock_code_hash: "0x9f0f0ba142b58cba2fe047546cfd8481d5b1769437cd3533e6458b21b61871ab",
            lock_hash_type: "type",
            lock_args: "0x",
            type_code_hash: Some("0xd96cee56727a2bb9a21408c154d278df5095fb4b4dcfd50516156424479bfe54"),
            type_hash_type: Some("type"),
            type_args: Some("0xb4f4302965b7d6421481a520ee7eb5971a5e808c"),
            data: "0x0372ad09e23868d88a8e85519ebeee56f60eda6c5a564e8a369a4c9d8ea29087e20000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000",
        },
        CellFixture {
            capacity: "0x749a018a84",
            lock_code_hash: "0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8",
            lock_hash_type: "type",
            lock_args: "0x68ae5937c2143d3b6b93e8b5aa62d4cd547d52ef",
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            data: "0x",
        },
    ],
    witnesses: &[
        "0x5b00000010000000550000005500000041000000fee51268d7edab7259e6f1fb460d2d523859de3e132f0910daa784ae288bc6295b666e226cca500c490e193bf5a412d68f3b5d1dbe9c7695c5934d2954420db500020000000000",
    ],
    inputs: &[
        CellFixture {
            capacity: "0x4a817c800",
            lock_code_hash: "0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8",
            lock_hash_type: "type",
            lock_args: "0x68ae5937c2143d3b6b93e8b5aa62d4cd547d52ef",
            type_code_hash: Some("0x00000000000000000000000000000000000000000000000000545950455f4944"),
            type_hash_type: Some("type"),
            type_args: Some("0xcd8d26f8a4afd70e5f429cc6a42801960787632ae007c2d7518f00c1ef6cba1b"),
            data: "0x",
        },
        CellFixture {
            capacity: "0x746a528800",
            lock_code_hash: "0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8",
            lock_hash_type: "type",
            lock_args: "0x68ae5937c2143d3b6b93e8b5aa62d4cd547d52ef",
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            data: "0x",
        },
    ],
    input_outpoints: &[
        ("0x522c91d3242450fdc5ae4cc6891d566de34d6f99663679f15e96c03d01c59268", 2),
        ("0x6536fa27039d29040a4b380c44d514df800f7bf771ef9742e35f1a2c2b291f3c", 0),
    ],
};

/// `M2_register_support` — mainnet tx `0xf20d2e9662dd0078a04ca929e4cf35191e939a8ea7f22536003ff5a70e17663e`
/// at block 20518306. 2026-09-24 from local node http://127.0.0.1:8114.
pub const M2_REGISTER_SUPPORT: TxFixture = TxFixture {
    label: "M2_register_support",
    network: "mainnet",
    tx_hash: "0xf20d2e9662dd0078a04ca929e4cf35191e939a8ea7f22536003ff5a70e17663e",
    block_number: 20518306,
    block_hash: "0xf88a07fc9328dad81882c559078ea1e5306f620d02be5623663f4e310b38b663",
    outputs: &[
        CellFixture {
            capacity: "0x59682f000",
            lock_code_hash: "0x9f0f0ba142b58cba2fe047546cfd8481d5b1769437cd3533e6458b21b61871ab",
            lock_hash_type: "type",
            lock_args: "0x",
            type_code_hash: Some("0xd96cee56727a2bb9a21408c154d278df5095fb4b4dcfd50516156424479bfe54"),
            type_hash_type: Some("type"),
            type_args: Some("0xb4f4302965b7d6421481a520ee7eb5971a5e808c"),
            data: "0x0372ad09e23868d88a8e85519ebeee56f60eda6c5a564e8a369a4c9d8ea29087e262d71147ac82b83c8531126cacb0d2f072bfd94adfdd916c00ac55d7dab2e9a4b85775a811bb4063e94cc98182ac55d7dab2e9a4b85775a811bb4063e94cc9818263656c6c756c61",
        },
        CellFixture {
            capacity: "0x59682f000",
            lock_code_hash: "0x9f0f0ba142b58cba2fe047546cfd8481d5b1769437cd3533e6458b21b61871ab",
            lock_hash_type: "type",
            lock_args: "0x",
            type_code_hash: Some("0xd96cee56727a2bb9a21408c154d278df5095fb4b4dcfd50516156424479bfe54"),
            type_hash_type: Some("type"),
            type_args: Some("0xb4f4302965b7d6421481a520ee7eb5971a5e808c"),
            data: "0x0372ad09e23868d88a8e85519ebeee56f60eda6c5a564e8a369a4c9d8ea29087e265b5fe7e7070b506f69bd8cabf9e4272111066455e00926c0057d926a44d83fc13b21ce037b1e31f4223e3c86757d926a44d83fc13b21ce037b1e31f4223e3c867737570706f7274",
        },
        CellFixture {
            capacity: "0x746a528800",
            lock_code_hash: "0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8",
            lock_hash_type: "type",
            lock_args: "0xe1f601a90f38dc2b551ee97a2f3d83876b2e6707",
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            data: "0x",
        },
        CellFixture {
            capacity: "0x7a00d40a54",
            lock_code_hash: "0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8",
            lock_hash_type: "type",
            lock_args: "0xe1f601a90f38dc2b551ee97a2f3d83876b2e6707",
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            data: "0x",
        },
    ],
    witnesses: &[
        "0x2200000010000000100000001c000000080000007265676973746572020000000000",
        "0x7f00000010000000550000007900000041000000ed734597ab6870f7b776f2a8027ff177955199ed326d8b9f87cf746459a90bb12a1bae3b6cf42d8099d6415562f9130513ae3200ce29175aeb395eec71e81f2f01200000007a3f3bc54d2f38c95ee986d87a0d3b4d2b1265a150bb56d82ea281397247490d020000000000",
    ],
    inputs: &[
        CellFixture {
            capacity: "0x59682f000",
            lock_code_hash: "0x9f0f0ba142b58cba2fe047546cfd8481d5b1769437cd3533e6458b21b61871ab",
            lock_hash_type: "type",
            lock_args: "0x",
            type_code_hash: Some("0xd96cee56727a2bb9a21408c154d278df5095fb4b4dcfd50516156424479bfe54"),
            type_hash_type: Some("type"),
            type_args: Some("0xb4f4302965b7d6421481a520ee7eb5971a5e808c"),
            data: "0x0372ad09e23868d88a8e85519ebeee56f60eda6c5a564e8a369a4c9d8ea29087e265b5fe7e7070b506f69bd8cabf9e427211106645dfdd916c00ac55d7dab2e9a4b85775a811bb4063e94cc98182ac55d7dab2e9a4b85775a811bb4063e94cc9818263656c6c756c61",
        },
        CellFixture {
            capacity: "0x2540be400",
            lock_code_hash: "0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8",
            lock_hash_type: "type",
            lock_args: "0xe1f601a90f38dc2b551ee97a2f3d83876b2e6707",
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            data: "0xca016f7a90a78db1b9952afffdc8847a1ee0bfe0051f4f3ca5da2de61a79cf8e",
        },
        CellFixture {
            capacity: "0x746a528800",
            lock_code_hash: "0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8",
            lock_hash_type: "type",
            lock_args: "0xe1f601a90f38dc2b551ee97a2f3d83876b2e6707",
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            data: "0x",
        },
        CellFixture {
            capacity: "0x7d434b20ee",
            lock_code_hash: "0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8",
            lock_hash_type: "type",
            lock_args: "0xe1f601a90f38dc2b551ee97a2f3d83876b2e6707",
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            data: "0x",
        },
    ],
    input_outpoints: &[
        ("0x8a2ec9c9d1505231c04207f76700ba3edb4de7f36b33e2c4319e2868a4fc5425", 0),
        ("0x6d71d551c9293f6178b0cf729ec90b95c31e010dfa963befb442257ccbea0059", 0),
        ("0x426ff94196e2e3bc1c59889b1bfa42f31706c3e767ffa844656182f51e9b9dbb", 2),
        ("0x426ff94196e2e3bc1c59889b1bfa42f31706c3e767ffa844656182f51e9b9dbb", 3),
    ],
};

/// `M3_transfer_abuse` — mainnet tx `0x53a0519e06fc4aac3eca2606e12fc19849919a226af872f687dfb3209a0470fd`
/// at block 20516391. 2026-09-24 from local node http://127.0.0.1:8114.
pub const M3_TRANSFER_ABUSE: TxFixture = TxFixture {
    label: "M3_transfer_abuse",
    network: "mainnet",
    tx_hash: "0x53a0519e06fc4aac3eca2606e12fc19849919a226af872f687dfb3209a0470fd",
    block_number: 20516391,
    block_hash: "0x6f84c3a158daadb5bf943aae89c41f18fd3ab88038e5ed1a8aeb2b347556b53e",
    outputs: &[
        CellFixture {
            capacity: "0x59682f000",
            lock_code_hash: "0x9f0f0ba142b58cba2fe047546cfd8481d5b1769437cd3533e6458b21b61871ab",
            lock_hash_type: "type",
            lock_args: "0x",
            type_code_hash: Some("0xd96cee56727a2bb9a21408c154d278df5095fb4b4dcfd50516156424479bfe54"),
            type_hash_type: Some("type"),
            type_args: Some("0xb4f4302965b7d6421481a520ee7eb5971a5e808c"),
            data: "0x0372ad09e23868d88a8e85519ebeee56f60eda6c5a564e8a369a4c9d8ea29087e20000000000000000000000000000000000000000c6c2916c00ac55d7dab2e9a4b85775a811bb4063e94cc98182ac55d7dab2e9a4b85775a811bb4063e94cc981826162757365",
        },
        CellFixture {
            capacity: "0x7fb525cbbd4",
            lock_code_hash: "0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8",
            lock_hash_type: "type",
            lock_args: "0xe1f601a90f38dc2b551ee97a2f3d83876b2e6707",
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            data: "0x",
        },
    ],
    witnesses: &[
        "0x2200000010000000100000001c000000080000007472616e73666572020000000000",
        "0x5500000010000000550000005500000041000000bcf6b555ab7722f9ab307f37f20490bc99ece31a11a7f9ad3687b73bd5cc0a6c608b9cb9ecd3fdb5548addb9e0a37b7990292edabcaddce29ff69627b442dd5301",
    ],
    inputs: &[
        CellFixture {
            capacity: "0x59682f000",
            lock_code_hash: "0x9f0f0ba142b58cba2fe047546cfd8481d5b1769437cd3533e6458b21b61871ab",
            lock_hash_type: "type",
            lock_args: "0x",
            type_code_hash: Some("0xd96cee56727a2bb9a21408c154d278df5095fb4b4dcfd50516156424479bfe54"),
            type_hash_type: Some("type"),
            type_args: Some("0xb4f4302965b7d6421481a520ee7eb5971a5e808c"),
            data: "0x0372ad09e23868d88a8e85519ebeee56f60eda6c5a564e8a369a4c9d8ea29087e20000000000000000000000000000000000000000c6c2916c0057d926a44d83fc13b21ce037b1e31f4223e3c86757d926a44d83fc13b21ce037b1e31f4223e3c8676162757365",
        },
        CellFixture {
            capacity: "0x7fb525cc1ec",
            lock_code_hash: "0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8",
            lock_hash_type: "type",
            lock_args: "0xe1f601a90f38dc2b551ee97a2f3d83876b2e6707",
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            data: "0x",
        },
    ],
    input_outpoints: &[
        ("0x8ee4e6fe22d2933eaa25763fb397e7edbb78869c5269d1ee8de59858771bd35a", 1),
        ("0x8ee4e6fe22d2933eaa25763fb397e7edbb78869c5269d1ee8de59858771bd35a", 3),
    ],
};

/// `M4_transfer_apt` — mainnet tx `0xccff8353b81b678777c7d41188c6dfba24b2b28d1d4cb73a8ab4219552ea00e3`
/// at block 20520552. 2026-09-24 from local node http://127.0.0.1:8114.
pub const M4_TRANSFER_APT: TxFixture = TxFixture {
    label: "M4_transfer_apt",
    network: "mainnet",
    tx_hash: "0xccff8353b81b678777c7d41188c6dfba24b2b28d1d4cb73a8ab4219552ea00e3",
    block_number: 20520552,
    block_hash: "0x542c46e26ed7b278bbc0bff6ec0972ed56b5d5e80ad543fee62e1ea7bd514a94",
    outputs: &[
        CellFixture {
            capacity: "0x59682f000",
            lock_code_hash: "0x9f0f0ba142b58cba2fe047546cfd8481d5b1769437cd3533e6458b21b61871ab",
            lock_hash_type: "type",
            lock_args: "0x",
            type_code_hash: Some("0xd96cee56727a2bb9a21408c154d278df5095fb4b4dcfd50516156424479bfe54"),
            type_hash_type: Some("type"),
            type_args: Some("0xb4f4302965b7d6421481a520ee7eb5971a5e808c"),
            data: "0x0372ad09e23868d88a8e85519ebeee56f60eda6c5a564e8a369a4c9d8ea29087e23a1e411c2444b8d586e3b9a03453ee8a1a34e94e6ec8916c001e3a88ca5cc39f1bd38c091b53e33b7c29ebd0191e3a88ca5cc39f1bd38c091b53e33b7c29ebd019617074",
        },
        CellFixture {
            capacity: "0x107b1e65af19",
            lock_code_hash: "0xd00c84f0ec8fd441c38bc3f87a371f547190f2fcff88e642bc5bf54b9e318323",
            lock_hash_type: "type",
            lock_args: "0x0001ea0eba2dd6b055c9f3c8340fa06e70547e019a3a",
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            data: "0x",
        },
    ],
    witnesses: &[
        "0x2200000010000000100000001c000000080000007472616e73666572020000000000",
        "0x68010000100000006801000068010000540100000128225bb834176f6e4ab74c9434274065b682f693edfce81d53367db02239d64006af6d05d59383a4579608b4e00a0af3684f15494b4e1b26e9ac8f0e9dda9aa01b051467530e84bc1a2847bc4f56302c124b84c7d67d0f4b42528f6af66046b2d1f52727436b43c3cc234adb79d9b9e6be94ffd9b4e5081531528bd74b3ef932d280d9320d7862ad09b32103900596a08ba01a51863a8aac3f5ac1969360ae301d000000007b2274797065223a22776562617574686e2e676574222c226368616c6c656e6765223a224e544a685957566a4e4467354f475a684e446469597a49325a5449334e6a466a5a6a5a6b4d4463774f475a684d4467335a6d59345a4449774d54526d4d574a6a4f54466d4f4445785a5441344e7a686d5a574d315a67222c226f726967696e223a2268747470733a2f2f6170702e6a6f792e6964222c2263726f73734f726967696e223a66616c73657d",
    ],
    inputs: &[
        CellFixture {
            capacity: "0x59682f000",
            lock_code_hash: "0x9f0f0ba142b58cba2fe047546cfd8481d5b1769437cd3533e6458b21b61871ab",
            lock_hash_type: "type",
            lock_args: "0x",
            type_code_hash: Some("0xd96cee56727a2bb9a21408c154d278df5095fb4b4dcfd50516156424479bfe54"),
            type_hash_type: Some("type"),
            type_args: Some("0xb4f4302965b7d6421481a520ee7eb5971a5e808c"),
            data: "0x0372ad09e23868d88a8e85519ebeee56f60eda6c5a564e8a369a4c9d8ea29087e23a1e411c2444b8d586e3b9a03453ee8a1a34e94e6ec8916c00ac55d7dab2e9a4b85775a811bb4063e94cc98182ac55d7dab2e9a4b85775a811bb4063e94cc98182617074",
        },
        CellFixture {
            capacity: "0x107b1e65bda7",
            lock_code_hash: "0xd00c84f0ec8fd441c38bc3f87a371f547190f2fcff88e642bc5bf54b9e318323",
            lock_hash_type: "type",
            lock_args: "0x0001ea0eba2dd6b055c9f3c8340fa06e70547e019a3a",
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            data: "0x",
        },
    ],
    input_outpoints: &[
        ("0x47bd4ca5e3e4c5d05ee10b51b52e48c5bb804143dd386a858f1cb54f53cb42f1", 0),
        ("0xa8266f1b6e6a34384302867d60c6520ffa7fb41d8824c60b91be6d7b87e1db22", 1),
    ],
};

/// `M5_list_satoshi` — mainnet tx `0xea72eceef12331f136667624c64a4e1ee8a3d98940321a0dd62664ded7f2f108`
/// at block 20521020. 2026-09-24 from local node http://127.0.0.1:8114.
pub const M5_LIST_SATOSHI: TxFixture = TxFixture {
    label: "M5_list_satoshi",
    network: "mainnet",
    tx_hash: "0xea72eceef12331f136667624c64a4e1ee8a3d98940321a0dd62664ded7f2f108",
    block_number: 20521020,
    block_hash: "0xe7c4a35a571cdd3debfe311d8ba4d3c3661566bb7792bb40aec19dde849216fa",
    outputs: &[
        CellFixture {
            capacity: "0x59682f000",
            lock_code_hash: "0x9f0f0ba142b58cba2fe047546cfd8481d5b1769437cd3533e6458b21b61871ab",
            lock_hash_type: "type",
            lock_args: "0x",
            type_code_hash: Some("0xd96cee56727a2bb9a21408c154d278df5095fb4b4dcfd50516156424479bfe54"),
            type_hash_type: Some("type"),
            type_args: Some("0xb4f4302965b7d6421481a520ee7eb5971a5e808c"),
            data: "0x039827525af55444ac293a956f915af8ef282ecf1c513913636ad0a9cc31a99251cb5c723f056bd288229168368c658fd66ccdb2905853926c004136f1b0aa24b8372b2e52a13e77a24b676d56894136f1b0aa24b8372b2e52a13e77a24b676d56897361746f736869",
        },
        CellFixture {
            capacity: "0x3b9aca000",
            lock_code_hash: "0x086c8f4e9d4272e3dfbaca399792f730e6604591e87931ee6d67047a3c900879",
            lock_hash_type: "type",
            lock_args: "0xac55d7dab2e9a4b85775a811bb4063e94cc98182835681d1b7706a491abf6bcf00407a10f35a0000",
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            data: "0x4b000000100000003000000031000000d00c84f0ec8fd441c38bc3f87a371f547190f2fcff88e642bc5bf54b9e31832301160000000001ea0eba2dd6b055c9f3c8340fa06e70547e019a3a",
        },
        CellFixture {
            capacity: "0x41085281ee4",
            lock_code_hash: "0xd00c84f0ec8fd441c38bc3f87a371f547190f2fcff88e642bc5bf54b9e318323",
            lock_hash_type: "type",
            lock_args: "0x0001ea0eba2dd6b055c9f3c8340fa06e70547e019a3a",
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            data: "0x",
        },
    ],
    witnesses: &[
        "0x9900000010000000100000001c000000080000007472616e736665727900000001000b616464726573732e333039006400636b62317172677165703873616a3861677377723330706c73373368726132387279386a6c6e6c6333656a7a6833646c326a75377878706a78716771713834716177336436366339746a306e657136716c67727777703238757176363867326a6b7938682c010000",
        "0x68010000100000006801000068010000540100000128225bb834176f6e4ab74c9434274065b682f693edfce81d53367db02239d64006af6d05d59383a4579608b4e00a0af3684f15494b4e1b26e9ac8f0e9dda9aa0409f2599753e04c1057af7fb9f5876af449404881e71f134ce493e48b704c53eef1cf222d3ecfb724c52d0324c30db191e5be1bd62890e0e824bcd42f16b4d05d280d9320d7862ad09b32103900596a08ba01a51863a8aac3f5ac1969360ae301d000000007b2274797065223a22776562617574686e2e676574222c226368616c6c656e6765223a224d4755354d4455344f4446684e6a49784e4759334d5468684d6a4d30597a49774d446b3559574e684d6d46694d7a45314e324d334d5751355a6d526b4e6a4d344e4463314d475a6d4d7a637a4e32517a4d6a41795a67222c226f726967696e223a2268747470733a2f2f6170702e6a6f792e6964222c2263726f73734f726967696e223a66616c73657d",
    ],
    inputs: &[
        CellFixture {
            capacity: "0x59682f000",
            lock_code_hash: "0x9f0f0ba142b58cba2fe047546cfd8481d5b1769437cd3533e6458b21b61871ab",
            lock_hash_type: "type",
            lock_args: "0x",
            type_code_hash: Some("0xd96cee56727a2bb9a21408c154d278df5095fb4b4dcfd50516156424479bfe54"),
            type_hash_type: Some("type"),
            type_args: Some("0xb4f4302965b7d6421481a520ee7eb5971a5e808c"),
            data: "0x039827525af55444ac293a956f915af8ef282ecf1c513913636ad0a9cc31a99251cb5c723f056bd288229168368c658fd66ccdb2905853926c00ac55d7dab2e9a4b85775a811bb4063e94cc98182ac55d7dab2e9a4b85775a811bb4063e94cc981827361746f736869",
        },
        CellFixture {
            capacity: "0x4143ed4d000",
            lock_code_hash: "0xd00c84f0ec8fd441c38bc3f87a371f547190f2fcff88e642bc5bf54b9e318323",
            lock_hash_type: "type",
            lock_args: "0x0001ea0eba2dd6b055c9f3c8340fa06e70547e019a3a",
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            data: "0x",
        },
    ],
    input_outpoints: &[
        ("0xc97a8078dcabd60ca7803ad3ab1717b16408430ae2183d5bca2c008092314f15", 1),
        ("0x75208a18dfc8daeacf17331783005c972b7b5f94edc5ec7f3850328b2f292ff7", 1),
    ],
};

/// `T1_ring_root` — testnet tx `0x561ced5afde747c0dbdae8a8790433421c0e317bc6f6bdbe92cf49884c4b8e9a`
/// at block 22365319. 2026-09-24 from local node http://127.0.0.1:8124.
pub const T1_RING_ROOT: TxFixture = TxFixture {
    label: "T1_ring_root",
    network: "testnet",
    tx_hash: "0x561ced5afde747c0dbdae8a8790433421c0e317bc6f6bdbe92cf49884c4b8e9a",
    block_number: 22365319,
    block_hash: "0x3a2e14a21fa249458e5677c30e1f074c9100c4391eb479df1215d9b0a7bde3b9",
    outputs: &[
        CellFixture {
            capacity: "0x47868c000",
            lock_code_hash: "0xede6a3d80717c3d7927eea678d095abbe68dbb08ca6fdbbbdd9de906455a4afd",
            lock_hash_type: "type",
            lock_args: "0x",
            type_code_hash: Some("0xe0706b176678181d982290d93dfcd82098e60cceaa4a87f10f32dcbcc91df1d9"),
            type_hash_type: Some("type"),
            type_args: Some("0x2510c78057479c9b023fe6e98ce43979e92a1353"),
            data: "0x0372ad09e23868d88a8e85519ebeee56f60eda6c5a564e8a369a4c9d8ea29087e20000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000",
        },
        CellFixture {
            capacity: "0x884e0eded2",
            lock_code_hash: "0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8",
            lock_hash_type: "type",
            lock_args: "0xadaec3261a8f17e0c2785990c02a7a3781635514",
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            data: "0x",
        },
    ],
    witnesses: &[
        "0x5b0000001000000055000000550000004100000087f61ac1e3c7afbfc9f975995b208fdca874f1336b37303d9c46b11357d3b9e043315415f940b0aa72e2ecf4ca98ed5916566ff5eaffea02e79512c5b055e02e01020000000000",
    ],
    inputs: &[
        CellFixture {
            capacity: "0x4a817c800",
            lock_code_hash: "0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8",
            lock_hash_type: "type",
            lock_args: "0xadaec3261a8f17e0c2785990c02a7a3781635514",
            type_code_hash: Some("0x00000000000000000000000000000000000000000000000000545950455f4944"),
            type_hash_type: Some("type"),
            type_args: Some("0x204e243d378e9fa97cbac99e690108bcd9d556c1bbb349b4921de058ea6f1993"),
            data: "0x",
        },
        CellFixture {
            capacity: "0x881e5fdc4e",
            lock_code_hash: "0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8",
            lock_hash_type: "type",
            lock_args: "0xadaec3261a8f17e0c2785990c02a7a3781635514",
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            data: "0x",
        },
    ],
    input_outpoints: &[
        ("0x50b82bc32202debe7cca0ca27a9355e3b12f5ebfa8d643ec17d6044f891648ee", 2),
        ("0x50b82bc32202debe7cca0ca27a9355e3b12f5ebfa8d643ec17d6044f891648ee", 3),
    ],
};

/// `T2_register_joaom_maria_records` — testnet tx `0x89191ea4bae150f82521140968fae918040e724f647eadec02e02077d748386b`
/// at block 22471181. 2026-09-24 from local node http://127.0.0.1:8124.
pub const T2_REGISTER_JOAOM: TxFixture = TxFixture {
    label: "T2_register_joaom_maria_records",
    network: "testnet",
    tx_hash: "0x89191ea4bae150f82521140968fae918040e724f647eadec02e02077d748386b",
    block_number: 22471181,
    block_hash: "0x90cf2eef4b66dcc075cb15b273cbee1bb5cd546d3e377e1d04efd54a8d83d3b8",
    outputs: &[
        CellFixture {
            capacity: "0x59682f000",
            lock_code_hash: "0xede6a3d80717c3d7927eea678d095abbe68dbb08ca6fdbbbdd9de906455a4afd",
            lock_hash_type: "type",
            lock_args: "0x",
            type_code_hash: Some("0xe0706b176678181d982290d93dfcd82098e60cceaa4a87f10f32dcbcc91df1d9"),
            type_hash_type: Some("type"),
            type_args: Some("0x2510c78057479c9b023fe6e98ce43979e92a1353"),
            data: "0x033b339494bd0e29b77c0dca959bc4d6d87e4ac232bd7df9c1335163fe85f5eb18241e3586a41eb75dd6d68bf555acea74d6649ed529da856c0058e6c6f873af57732daae458be3c56c2c847b14158e6c6f873af57732daae458be3c56c2c847b1416d61726961",
        },
        CellFixture {
            capacity: "0x59682f000",
            lock_code_hash: "0xede6a3d80717c3d7927eea678d095abbe68dbb08ca6fdbbbdd9de906455a4afd",
            lock_hash_type: "type",
            lock_args: "0x",
            type_code_hash: Some("0xe0706b176678181d982290d93dfcd82098e60cceaa4a87f10f32dcbcc91df1d9"),
            type_hash_type: Some("type"),
            type_args: Some("0x2510c78057479c9b023fe6e98ce43979e92a1353"),
            data: "0x0372ad09e23868d88a8e85519ebeee56f60eda6c5a564e8a369a4c9d8ea29087e22cf2cdac7ab0e7b97f4b50475fb5ce1b32dfe711fbe68f6c0069e8165efb4cb3b2cd62300e7d16f41a1c65ceab69e8165efb4cb3b2cd62300e7d16f41a1c65ceab6a6f616f6d",
        },
        CellFixture {
            capacity: "0x5c89ddb680",
            lock_code_hash: "0xd23761b364210735c19c60561d213fb3beae2fd6172743719eff6920e020baac",
            lock_hash_type: "type",
            lock_args: "0x000140911fa94eaef8c1d0eca81b23e1972ecb0548dc",
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            data: "0x",
        },
        CellFixture {
            capacity: "0x19f6a3c11688",
            lock_code_hash: "0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8",
            lock_hash_type: "type",
            lock_args: "0x23870b08ec5f6260c50a63646170d61e26d155c7",
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            data: "0x",
        },
    ],
    witnesses: &[
        "0xc901000010000000100000001c000000080000007265676973746572a901000006000b616464726573732e333039006400636b7431717266727763646e76737373776477706e337339763866703837656d617433303663746a77736d336e6d6c6b6a673871797a61326371677171397837357a75346c37676c64363036723665796430306d346c7a79337a6b786b71346e79777a752c01000009616464726573732e30003e00746231703237767464306a776d3235746478336d36746763757168346c6578756c3076736c7572683079637a633766393272667030736b737264737936742c0100000a616464726573732e3630002a003078656646324634436132444536656444363364416665343833383536463134453639346437333144442c0100000d70726f66696c652e656d61696c0011006d61726961406578616d706c652e636f6d2c0100000d70726f66696c652e70686f6e650010002b3335312039313220333435203637382c0100000a647765622e636b626673004800636b6266733a2f2f346266306362646261633066386538656231616664646534333963336263306234333731623366336165636538633730343866656630366566366563376231302c010000",
        "0x7f00000010000000550000007900000041000000e329de7a51feb20409c370832f99a08b76236b2c6cdbb7cc5bbc2858d1bc69b753a8b932eaa752fa46c794eaa36adee5cd063a25760cbd0fa151034f6b0e66010020000000b749b13ab9026ab71f3cb0cc971d0dbf1030bae1f8b3a006a45055d5eec73f0a020000000000",
    ],
    inputs: &[
        CellFixture {
            capacity: "0x59682f000",
            lock_code_hash: "0xede6a3d80717c3d7927eea678d095abbe68dbb08ca6fdbbbdd9de906455a4afd",
            lock_hash_type: "type",
            lock_args: "0x",
            type_code_hash: Some("0xe0706b176678181d982290d93dfcd82098e60cceaa4a87f10f32dcbcc91df1d9"),
            type_hash_type: Some("type"),
            type_args: Some("0x2510c78057479c9b023fe6e98ce43979e92a1353"),
            data: "0x033b339494bd0e29b77c0dca959bc4d6d87e4ac232bd7df9c1335163fe85f5eb182cf2cdac7ab0e7b97f4b50475fb5ce1b32dfe71129da856c0058e6c6f873af57732daae458be3c56c2c847b14158e6c6f873af57732daae458be3c56c2c847b1416d61726961",
        },
        CellFixture {
            capacity: "0x2540be400",
            lock_code_hash: "0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8",
            lock_hash_type: "type",
            lock_args: "0x23870b08ec5f6260c50a63646170d61e26d155c7",
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            data: "0x8079285d17c4b63a6a194321fa8aede956d6521814b8b258ca02ed2180f2a737",
        },
        CellFixture {
            capacity: "0x1a567015e694",
            lock_code_hash: "0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8",
            lock_hash_type: "type",
            lock_args: "0x23870b08ec5f6260c50a63646170d61e26d155c7",
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            data: "0x",
        },
    ],
    input_outpoints: &[
        ("0x974fc983a62a6f7b977c6e2170695cc4aafc5a7544c1aa2513574f1563fa0fad", 0),
        ("0xb1745c34666bed64ec8654b16ca3d34af815576ad8cad7330957b3a54c9b34ae", 0),
        ("0xb1745c34666bed64ec8654b16ca3d34af815576ad8cad7330957b3a54c9b34ae", 1),
    ],
};

/// `T3_register_subname` — testnet tx `0x1fd44b418d7bdb7b3081f9c26ce3d4b3ef1cb5998b4287fa39ee0534c3e3cb32`
/// at block 22367979. 2026-09-24 from local node http://127.0.0.1:8124.
pub const T3_REGISTER_SUBNAME: TxFixture = TxFixture {
    label: "T3_register_subname",
    network: "testnet",
    tx_hash: "0x1fd44b418d7bdb7b3081f9c26ce3d4b3ef1cb5998b4287fa39ee0534c3e3cb32",
    block_number: 22367979,
    block_hash: "0x31cbc09ef74c20b8a585f40135c7a626f516732c2ec54f4f69dfec16604624b8",
    outputs: &[
        CellFixture {
            capacity: "0x59682f000",
            lock_code_hash: "0xede6a3d80717c3d7927eea678d095abbe68dbb08ca6fdbbbdd9de906455a4afd",
            lock_hash_type: "type",
            lock_args: "0x",
            type_code_hash: Some("0xe0706b176678181d982290d93dfcd82098e60cceaa4a87f10f32dcbcc91df1d9"),
            type_hash_type: Some("type"),
            type_args: Some("0x2510c78057479c9b023fe6e98ce43979e92a1353"),
            data: "0x03a15b28707237cee42120cce959579a2af24b1a4f5e1f575e373f9b23afe57650bb008a3e9045554d5b1b609c072b59b404320f9f6731646e00d5026c2c742a379b0c422271a4e1f59b211a6c3158e6c6f873af57732daae458be3c56c2c847b14176332d66697273742d6e616d65",
        },
        CellFixture {
            capacity: "0x59682f000",
            lock_code_hash: "0xede6a3d80717c3d7927eea678d095abbe68dbb08ca6fdbbbdd9de906455a4afd",
            lock_hash_type: "type",
            lock_args: "0x",
            type_code_hash: Some("0xe0706b176678181d982290d93dfcd82098e60cceaa4a87f10f32dcbcc91df1d9"),
            type_hash_type: Some("type"),
            type_args: Some("0x2510c78057479c9b023fe6e98ce43979e92a1353"),
            data: "0x0372ad09e23868d88a8e85519ebeee56f60eda6c5a564e8a369a4c9d8ea29087e20000000000000000000000000000000000000000774e836c0058e6c6f873af57732daae458be3c56c2c847b14158e6c6f873af57732daae458be3c56c2c847b14173686f702e76332d66697273742d6e616d65",
        },
        CellFixture {
            capacity: "0x68c6171400",
            lock_code_hash: "0xd23761b364210735c19c60561d213fb3beae2fd6172743719eff6920e020baac",
            lock_hash_type: "type",
            lock_args: "0x000140911fa94eaef8c1d0eca81b23e1972ecb0548dc",
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            data: "0x",
        },
        CellFixture {
            capacity: "0x4a8171e64",
            lock_code_hash: "0x147ecbb5c5127d982ee1362d2c2bb4267803da2eb006d150e88af6caaa0a7eaf",
            lock_hash_type: "data1",
            lock_args: "0x5ea09d41003bb714874a268c4ba2332bb2bf755d3e195970b7fd21e67e61f7da",
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            data: "0x",
        },
        CellFixture {
            capacity: "0x1ae0b2720402",
            lock_code_hash: "0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8",
            lock_hash_type: "type",
            lock_args: "0x87a174cb43b161c9d4329fd04e73f28943577220",
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            data: "0x",
        },
    ],
    witnesses: &[
        "0xb700000010000000100000001c0000000800000072656769737465729700000002000b616464726573732e333039006100636b7431717a646130637230386d38356863386a6c6e6670337a65723778756c656a79777434396b7432727230767468797761613530787773717638353936766b736133763879616776356c36703838387535666764746879677134337466346b2c0100000f70726f66696c652e77656273697465000a0063656c6c756c612e69642c010000",
        "0x7f000000100000005500000079000000410000005cadecec9d0f14053fd64ff224f267327e4832dca303e994f60722089176874639721d07597fcce8e4dfbf1f235b2db935d0a3521cb4d22a45454158561c5a5000200000000e7cf41a5605aa8aa2a152d85133eb0520d4f8aad00122aca8eb881b3bb5d535020000000000",
        "0xe91e000010000000e91e0000e91e0000d51e000080010101634695c309ccc237d2effce999f326f9e3e1ff99542edf20ead57720cf48ff9597ea630d92af9fda256f6a4f4e1c0643743fc7befb6841490874be97411ab6c68f3b9ee6c4e133142f02cc01bbc59cc2d2928aaca110b556e4261df30e4d5a7cdff65c2c394a4b740e936879d5ceb08af7d6efc7e882df251c9c8633cd7dc63f0682cb026125e4011d41b7d5d447f8089811aa03371e164d248ed66ca678f8b1a7319f81df74f2830e50f9aaeac2f53ab0f714a0047d991d18c7107439c966efcb8146f9a685c3212e116b0052c455e723e3de9d2784de0a48922d87e602ebf2ec049153512fdbb01ab3386c56a4264c54389c277c7e513bf24312ea9ee642adc9267c78df0c5c6306664f26e73b5f6a1721c8cfc2095da27828c65d79826d6816eeda72d5827472d8b2a5eaa9e9364e84be89b72e7a05bd7aa9faf9eaf88d55dd8a2c2779301b95bd643da6ddf970eca65f05cdc5fe6d5a503398c88f8a07ef4d139f546380f73e4a964e68f25ee90985e188b63a0a7d32e8e6b86569dfb98fa2ffa5a46dee7b9109c19f482cef6046556783abc6902e90a3e580ff05fa078ce041f82b12b890d91e280880e3269cf3b9aa497fc3bdca1ebdc78e4ff7712bfb2b99e511e8c7d21c2cfe728ab58455dae6c78c58fece012758e0bb6df05863fc9325c3dc2f1980e272d9296c21c5bbdb5cdb1b4c85d33d7056c4df9be510beefe4abf34aabb7236a8397ee1351ed919f1b6607d0113880fcff40a2092ac0eaa93f040a4f5adc4788a5d7ec4ad5a01fc8f9122411f7d0b1003cf1b099c336b73d1069514db5e04f8d2b558c88022a270e63900783db322440a4b5f0c4f8107674c6f868efdbcf3a752bed5299c72b79d00f32d338b01b4f6f35fc076eeff1c10862f35dcef86f3f57fd59d3ff9f0b17725bc15588b0f74db9e25c81fafcea18f3e128a314b4c6f531a0d0aa9bbea887678689d98916c684ebf484efe6c994f3bf3f37e29d1f2b5c2e1672a7fc0de69a4449510d2fcf5f695cb05e8c7eb8044a2124aa165c2e03e19b21d78b927d2931ec110a1dfb16e6901c414a3714944ed83a1beac61021bb04553253e7d8a529895ff0dd26c226c7e013e37a2714af65c8f6931be41abbccbe8592468e45430e85632271d8e4a8ff8572fd6ccf7ef10cfb238493ac4dfa7d21af8e1179d358add1edee06f9ab572094902060a644cba50bf4d5ee683af0215c4b0ce23b25583f08160a592f7d2e89a7e40ef60b511e9e2d9d104af70ae3f4eba319f06bc0431ca744fe5d5811b815ce80796ac3c48d456008a650d2aef916ad8d38c8d33a422c80a764f65b63ed15caa175505ce71a8927e68bdf107408f8b35f0f3ebdeba5aa8cfc382d70e18e2fecbff2d9b0c07430e4d7aa88bd32228bb57bae7e4bbedf54fbc3781f84d4f3c31f4294749feb100a7f0cd209b5518655d2cd743bd71a3e281904261e472d2c5a55d15170405c06e3d750ceb68b23bab321e972ae858f20de39d17d40fed4bcf79cfc8f42b12d6584c9ad2e3bf0a2b05d66b5bc69edd409b4861fb7d3bbdda9ab76d7c04301455d43cd0a9abbd3b92f7eeafd50733add7387fa4a1d78748a0722ee3e1023c7863243ccbd076bd83e2b9a66e9ef77b509de57c2b43279444c552a4820d9c445f72aa52c6dde4d0448ab69d9c25cedec2630395d2cf46a8cb3cea5f1f6d992f684fbf15ed558b69a23e96e0f9e156779044fa27f00f3753da5053cb3c88b182d054e65443bafe1b1e33271d9a50a27dd1ff92a2c60fcd73416184ecfbe0a31b534946df74abd04867982b1c50c102b2d0423ee042fdd9effa1b2b6eeca13539adca5405d7a2fb8809cb11060ae8ded0dfa63f343f7e1859b0986d0a809ff8df64ab0255e3d46d7fda9ec313502c810d716f291b713db20f563d8cfbfcc73bed7f739421c1e323b88adfb26b3b4b4854e7255c6789ee2e5bff106bc3cc4f1ab8c006ff28c8a28a85bada6fd919e1a748204c655ef9017b003154a410e7a1d3f312df84d96a8d30d60188a16de17d245287aeecf7e2e2d66312c8078c9b92ebc32bf3ee150194ec4fb15ff4ce708e59a3184be5714ecf663f87796692297aa57af18f55d614db5683abae6b920847b035ef1a82eb5d9d606174e627ca569ada04d3db0934d5213d269d4c82b604b9782588aac28ed334f9cbfc94dae445648ed04b069e3c2ad75467d5d587ac954f5b215ce7d16c3bd37dda2e7e8627bc63d51141c113e9995b070b0295f76b2cf85cbd2e9dca89a235c6bcfc45d1446c8594782e51d1537d919d9dcf8efa4903b235c159537a818f07a1957c40ab9dbd76c82ce99f222e190fda25447d94fabc4e9916cb15f489513aa01142b048fb1bf8b36443e4164e0893b8248f10048a053293fdf0ba6751ad39093b6f970f740573159800cfcf0ac9fd29d1d3b5fe1aa9662878e28ddf91b359eeb6fd399fd13e66df207cf3504123e3c9606b52885209f39eba90b4fa0e3b2cee054947c158403467852db5be2b56a8db07dbb2c81ae2bc0c6c58026abad8210c097819a33de9d0b304763ec13f4fcd33b3b9bdd18ea36caff4bc10403a377de65908ef0580f7a96a6520a5a809e8ad6e02e29bb2be21086a3947b9845b4a08226760a0dcb2a9f67b049a8f0009bd12e0161dc9fadc5d9c6810ae2c17a1eee388c4ebae2149bbcdc838c8af70c4f6dce80270aa6b5943f3cf29ac4005f0c4dab0cc3ef38acec087f84039766562660792bd58786a6900d009f377b6931020ccfe0c2d10086551fbeca36691bd15a4cf532eca7da8956202b6e53418044d9cd0fb9303d78450f164945aff4b5eb2e46dac4168e942e17ff0d36daa44d08b1233dd5f29b0e582249c83e7268f506ce65f5da91fb1bf77cfc3c0f6014631a3301b7a66e411bc02874098c7e3231b084545522b98a783df567b6cec1a6e9b1c1f5573fa6fe0ff8031f4e3d63eeee2df883b156c503e1d83302316bd62da9c93ac9fcb05a2daadcba872c8d404d01bb7fa983292753f0d376a1d96cb1034d9930f10d26dbfc0329cf439fda77332a9fa6a1a63c257a0dd6c0752891e174d25e3504613a9e2d98cca7a7a7e7be6be5bb36db0576b893895ba1fdb8334f47debdd0a29325b6315a62d6bf00fbf691b8749c7018bb47f8af5a26df34bac9b70f95252df9a53fe27fddbcf5d4d1035b9c54808d00151bccf2fa198a58b7911c8e12dd8a2aab81108dfbba6e5c268ced9320f0a0895edc072be6aa50ea0f6428c93f4143099aab92332171e70a6db3a5183affb848c82668242b8bcfd05348fb381d5db6e0c33a0385010856f44279d21d4ecd8735d2f0fe735d18572b58c0fac86734dbf74426345937c50fc5070dd416d2f78db30cc9ed48723b9f7df3ecdefe99440620cc945d29fe02fe05513824be5bf031fd27e84072667ba2d4ddc964f6e0c28547417773245f4d99799e4c335f77b73ce09808b5d899a0d0083210493630e767dcf2d9e20d025004080c1ff464743c6e93569d55c9d4cea780268bca2c18210117e517cb12da6f806b49abdc07c1aeadb4ba33662852d87b0976ca3275737c9b77d32acf31da7009a6c2d76b9c1d8be5f504e0cce50b58a51fd9b941584bde18cd8068c10cdc404601762ca1c960ece286744da365d5dac5de7706e5fbac5310e243fc9921c31b63925ecb8fe041d16e2d1818cd2ce46057ef402e04b3a22f9216511c1e4fbf0f8138585c7c1eba9d91cd71cea301290cbee12261e9129b32b47b92d134c454c9b0fdc898b90344962d8a9863bae6a2e821c97f4fd1792dd1b424eca4927733ee0050cec4341d917aafd30dfbaa020e6f817e52a0adcad1cddfd632c16279625f53627a34de9dcf03c580553d2527d41ca1a46101e4fdaf91c4d5d0e1e2f84af548b3c8b406d47bfd672f856bd68d18e1a3b35a999add5f32326492af24722be10346cd5798d8da9816fba36bfa283311c47d02912fd3ebf9f8f1369071f27a819acb6887ee80d37565a66e968b9f19ed7064f82e565245dfe2788f40d32005ec0214d8cb4fa72191bfe3e87a1b546e7ccb7c46b46c711f88ac7bd6f8f1de54718d3b399575c4ecf6377ad25dc6f5bb3ca935ace85cc63d8ae81a6b6f4fe9427c7f2e8287f43b1797d8b2d9f58d123a14547ae858394934060359a79ddbd891a72fddd0f9d635cdbb22985cab929353461ca85c8651f222e69e2f04a8b48368227e4a189d96bcfc52590ef28ec1cf8f78da0291fed8cff186402c3e2791ade1719a3fdd9ba22de6286655547b282666f4e66bc21a3163bfc6652014aee879a0c77103a5864f113af235c428e7c501b7a18f3488ee2456c3f8224246c05e62a8c5d0b7e3815fdefb87f771e7bc826d86b269bec532cc0f12e65b3249a031c52e4bf632b7651c1b83e9992158b0d920ea89b2c83f654694245730e389baa9a87b5d0478ecb39a5763f331bf1de71b88bd98c7d9c6b317fb22b09a51d97b4ba5270c5a499c6ba46b705348d4678b5fda2f7c091689b1acfffb0cb7bb9e78abfcc684083e9b2d01c0a6dd51071729a5c01e2c7e3161f3686723bcae8744ec031fe43a611dfeb564b5612e8fd597ea58219dc1b099f9d88fc903e792d479d014f5225ed1594bc81d8f570fcab2803ee773ce26dde143e99223ca15079fbc727ca303fe3eac716d1fe54edc639ebecfefc5a927683b4f3961e306f3c406c70cca173faa0871ee9fe3e54afe458c4509a43dafaf12e59ff2270f09b11e09161606c861c1f2316c340b0ef4847ab2e1a07c0848b37329e5ef7d462a52e9beabbe97883fb4b364880ee026200723cbb314611dd5fd97dfa28cca4afdcd5f103d7e0f14ef2d529faee77c0d8a1733153b86b25e471485c03d565ae23a1a4017aaad70092273ee7bb83353e520106d2c216cdcf8327768541eb28cf16df2db7e962403b4187bbf5cd8d7535c622435f62ce27b4af53af390c9490115fa9e143da93c98870d86b5b0c900021fa0179383a9ee5fa11cd55d814c79d68140781062e0bb039eded0980d336e2e83c5341c0189e4405ef204e215431ac2f4a37b117a23fca7ec9079489a1abfa68c7fd61a35f695c938b5380a549e6a3bb3cf2c3ce47ba534f31a819b829983bc614cc8ae45d60132eaba3c06c89572fe0c19180248d50e3e8aaeb575204dc32f73cc353355af80bfeb1b09a04e036aa49f35c268aee14b03bf0817132d5b2e854f8e21d054c0b99b6111c6e763d9a9bdbb3a2260a5c543e9f07281596b6ad5e2b418947f54e665409240ff2c2219dc0f9ed1aee4a5b55154a33ea105952ef0c46526eab490545280a39fa72283d0560dcba76a129d5d9bf425807170f65c417361a7e2d56d2926a81a6cfc74f2c6b927ed99f58a816ae1c2fbefbdc69bd3b0e10f1c186ec3567c130d278d269c6b3ded657a8ac0754b26903519800fdc0656441d8650bbc6b5a4ca9f8ee0babcd6e5ca9199463aed1a92fc69a2518f8c2593339a24ed2db2a499d40a19cfe5dc7c37fc1a2717b6263946eac3d99cd845fa91dd1dd0733c04277b5f50ab645a2b61ce1b476256b678473be3aa6b775d1f9de5b9bea5f8411d96e453ef85b88c5345525adeaf5a15929a1ce0db1e3e8217c65cd78e0a798f94b1df9a94148adf1f7ddfc9a878da4c84bde3ad32fd3300c4f459eeaf32e618c0f4fffd2759d1253387aea33c00de07478f45b591661f5b92ed045d8b025dea836bfd533b76e18c68b18b83d92e327a2bea91f090141f0b6410c778b4ef5080916e8db105ffb43f14583be19f5e17116db6b0690ee577886d4cac62d3834420df7c17fc63baca32fd5b8be47710ae3c40c13db8ad2e65a3daf73e7b2517ad0f14b853fc52f5613840bd283d64bbf0f4b72e37253555ed28bf8f3078ce3e74d60f8a849ca033f60f95c5c4fe4602186e070591675cd477c6348cff4722b0deef2f42af3478ec5bc123b2c0ed21b1bae822af9282881258421279871567ed5df7452a7adc740f73e5547d93dfe02d49ec57e8f357901a0823a75d5ab3d3ebf857bdd6b71c508fe502985831d7ba0c717e1a778071755e22304b61b5ab8a13ac9157ee0acd9d9fa693da604f51ab643c99462d46308bf0489114104499192bd0552603ced5628a68f353da3392697d3bee3848070980e301574f5ccab4dcd1244b4b58e86f290bb3b604c21a823701516b771eaba8aad7bd1076393266a0fbe449142f98556db1f552695eccac3855d36f886ab994cbae8b09563c3b91a3c79f6a80be7208704d69005aae3a82a2b259797111ec4e269f6d530ec1fc1cac07edbfdbd0ea46d9eb7915cb6387f4f01ecaece841afc1ba9b2e2a6a5f184028679b19403a3a0d813ef782cbb615a29d0abffcae0a74eee5da7fd725f9aedb9ac2284e3055a4e13b9a95b54326f7928eb24d9c92c645ba990abae51fa6a483d34ffc835180132f4843b4d95c0e7501ede71291ad852edaef5083f3192d1ba84f72a760717021be4970634853bccaaa31a958af9da7fa91dced09550ab8bdf828a06da7b0f6b4cb8a50525cbdec75bb390b4e233fd1a4ecfb4cc9fa651d4a2968d332b03736829c05d1aa3ec92e7b375fc584d85c37de7637faaec0e5994fef173b808e53610581314f610531b91c86fdb9ae24cc7da612b0b7cc02b8a458ec3b50de18abc4efc1752d971be2c3a7da8b1563298bb77da44f2f395065311122c90ce40ce878960b5492f2a63b0a168d9c1fd9ffa21f0b03e5d2f342ae638539f4531e9ab9c8255987409a331d8c99967a1872ce4712d462c730037948e5f8a38c501d6a639095c296d562ec4be426669d131b28f436dce39cc1f1d500d067e5b5ad247a433059f21dd71d7d3094793970fda43f8ac7859bcaf57a93f89e91d51786b188ddb8801c7fb9d003a539c4ebe3dddb6306c9a7c686a6a00fa428269de297c311be4e9b7a6cff404b6e1a2c95ff21bafff710d0c8abee82f19c643528c34ed9ff01257dff32845de8312ff02d1d13d647afa16edba760259d888061bc647cee4211777f86fb0b046f314791ad46de25e56196efc4d40c75e8ac4f2fc2db79d1c7405d45113b9715d2f39967a208c67bec4e60573c4aebf96c377e564f9104df5b19c799eaa09727e321573e0daf68285b574032c38d6491a9f8baafc715bd47db58852276c72b436f6c0d784bd84a1367ab837b589b69459d6102387c0285d927aaa2214e0a097c6e5bcd53d86bf5c40d7adb3086109af9bf48e644ec861a7489bc4d695fa8d3435a015f1af4e03f9b43f9bc03d43eb1972ff7b117c3ee877a620da7b6c002557a28318e235bcc788b3e9e6a2d917810432cb4c926bf33602c6dfe16d57f7e3a2922f775861a396529ef69473093bcc08d05215e51073837030b2a81bddd82e362a1744ee84966e388079e518230109906ad67f073cc16adb116b52a58ed3b4f1127076e33b5ceff7f102908ecb446f128e12908f2b2e3076f105237173f4b2a5ce01c177b49a048859b43fcfc82f43bd35d0e683b34a368a9665da014dba6e7f91805646556f7a484661c8863a64250562f365c94eeb1de2efed25314e862e1f90e609e74b7325a8bf81495790c8a6ec37e58cad1e9e0705792c159ec32407d4b01f5765e5b4c7f98d413c6f3ad3691a14f7cda19bc79f335d26e7ab4fd56d7d561f867e3ea80dfe9c3979b243b38df53a3ccaadd54e92bc32d15814a653179df47b10f7da86b6c4df50067e9700b55336b66177bd844e362b503e169fd46972194a6f36ea8ad9e39c5d916863d4253a39d0960486d40aff6ca5aaeefda29df14e3b00c911dd750c9006001b02d45216ae00d089cd110054683c41908b238feb7bd78a5fc104f93e27ff9e247f7cd019878191f6cebe2bb924a90beaba31598c3d4a459460cd752925290c77d315c8975a9f0a7cb6468b4be8e6b4a49299a4efdd76aa706170e520d909647dfc0c587fe6f023f9046e0ef51ec8b098af5b35e19e4a7c6eaf897b93952ea8d2e33f3ff05ecbe81e7feaf13a5815497a057a2527ff128681bd5e754781acf495f2f7c7a69f4fa7648fceb48557b5b96957a938224fac3a754bfcb3a764bfe9d1a6975ee50beefd35a9b9f197b4b008fc02c444bdc5ba768ebcf3f2303ef998ba0d95a74ebb1e255c47f5c3ff26065e240d0342fb373fbbb4b226b51e523e36fe92209208252b72bf99d3aee0f55da4a624a2c580aef34adb278b9598b399863d6bdb0f0259be6370839fb5edafcd9a4ba9c296348aa8e511331818427c88c7bae9918439dfa5bc8fa44f429b0f28c220344d12a84593029ecb57e2d6f8f0536c0f326aa11ee776b85317fadcdb1228573b82941ed5b36dd6fc4014d96bc71ecd00a5f7459ad33253c7b9aedfd711110a1c685b2afd13a366123f5ecb5d78fce38b4101f9eaac44eaf011bfa2f05a785a62383af11c926e547bcc12c152120035869281fa342b4a96aae0aa5dfbbaeae1b65d5f3c32f791274036a5951afb380a81794c8136110a7970897b6eac29635b08e70309c1c9d6fb499e380f01d05a4927a7dd1e48451415a6600599f4185c8b1a2921f503f4ae3e09cf274219bc751fcb1f229bcb8fefede70630ffa5f2e907a94efccfd18de4bae880ee7508fcd1b7e41373d8abaf8e5cb8d901dd6f69912e83dcaf41c161b78017f940cca6863202d2180477e11943960ccbb1129451ad35688dfae0a60e2f87ac7e3eeba0ae2df30c3ef22c98b448461c23830ded4317fc901b83456689dc51d0ed2c0f1964deadfc88642b90c415532a2cc8bfc773504604379b422e5b1aa553738a6227c64a5fe6fc11d4741f73d682982cb403e8e6fdbf9453c8a6a0340811806edfc7b4ce9ca59048832b38e5ae4bd8cd38298d63458f2f32fbc48d0a52e2003c42a7bd0c42956eb3c1003846835fc1e89c6a97bcb312e667b58157fb349fc8f08f01a1cb86caf7158942fe9cd8d340b4592214337a9f1b24cd4dfda16f01ce3a693ae8f7d9d0305106f10b5f1de332a7f9b76e4463703992aa70bfe9904d5d4d44fcf46c1b482fac6bd1cff9cdd1b3ef42a82104638f6ad62bc3ce436fcc88f446008d5b98ee9eed2bf313152cb7aad40c1c67a31c4774a9c46573078ac2b0e160fc5f0bff1efe55d9969a7fe6698c21c6ed7a6b741d83d795cac356434aa78b6488b1b13c6fe101cb583f4652e3c21a1007de7b6a2a8d40d4188d2611e57c93161777c21b99f173a7eb5f43b6dba6c1f42dbc4ca574c885c8759f25fa120dc8fd2036a0914ed0455317eb6f52e72fccf43c9c5e2a77d3392a7358db84c2ea57582fec3f3c2617297c727dc6641e91d9cbac2ed95810bd7ef51bd96bc104127c3ccca74d267f4c86ca8a305a8212a2d119dc9067cb8ea90f766a615fd293bea44ec1a943827fb22b949fc73eb762bf4d431cc1444c58f6490a905c92e72411b605c2f3fe2bf59465d035e39a51f5ce7ec50aaa1b20095a5aac71531833cd132e3ddb60f23d598bcb48e86c97657cd3fc1d27c64cca57c48c686a0bf31fc74b94cfc66eb62019702b9a0297461b5d68e763afda487084b7cd37e2f6c044b1f19dce29b30fec7489eefab06ab46cb6156bd71fd28c0685dfcfe1e17371cdabe0e44dcb74a59dca643b0855914caaec2c2be0d2b9dc8578a16d3adeb5b009e0d2f6010110ee1590d59e3fcac0f4d0c065d9a523e5ef22027f381fe69430ba57ef4e685c4317a7f96c91294bbbe21a1bdf23f00bece7ca844fe9735169dbc3570b9405a9e6f64ecbeb43e1e5eaf0a45d6e370e022561d5b5e86593f218d52a5571ee660c8735599f13c59b7b11e01b9d4129f00e3113e35d7d465e8d5a74623cc64f8622ea9dfab07999c9c466bc55b6cf62cba0dbdbdcd309dd6ff0c9b8cabfa8476908ab4e44a4e3768465e16917d2558dcf7f06496df809e9ae4219b48b3bd33d2e7a2efd8aa0097180d3d2a72ed809b729ae0968fab608862c75efdd8eb02c3b7782f6f35b108d5a83a9c2180a2a97c98f27bf4e34530d6c0a6bef196d972ae6d5e7c7c046e7eee8bd97c828b4789314e13279ef8af3b5121080fb3977e0806de264eca29752957329bcac485648d9db5ab2c503cbae7cb2b2cf75874652d2e9f61857f79537e348017c56914db6b79fe21675a0aaf582260a3cd295035f00fa7f7f2b3011a74a1020dc1164b385c7757cf8b5defb4014e5fadd2f94807fe881f4fc74c142d0f5a066ecfc032c7cb73d873c406f45ec1d807b818891439c8d03b7be37ad40b54f6e61187b7e0741e3d6171a3cc04fb32cd365cbf04c35fefeb9c630232b6148f7014306d39e784a8884e1b16d8c1c1adc2c293aef48713187835cc332459e60720d168f9f7c1a0cc33754c0cdd2a8856efa2cfd17c7a7942521231f961a6ae4dc81d6f3ba922a5fcd82e455cc21ff93def51f713bfadfc48c20a12b6ed7664ce21b68fe447f5ee08b89316c88108c83adb5855efb67d03d3abec6a8236ae8fd3f9aa0bd22d20f074bb8bf906d2aabce23078eda5ab684a799116d1e247b77b9460bda289116f4381403c1f90862f83a516ee1b854dcb399da78aacb0c748c014bf2a4c74bd72963fa6ff908966b83ead3440ccaf1e7ef5a913b15ecd88543ebc5ca2ede3bca427b688a0fef32584b6a5b445966aaa8db6ae0bfa954410f7b9075d6d6d7b166b3eea8db940fff1d3f0dcf57fc8b3b2554cee907a34dfa7860300b28b08e77478ba884a11380d886ba0d744f92aa7cc31740227e7b5667e06054f973d44aa5492e56abbf84b19079697b49efba69a04ce0fd4fbf52c3f8adaa24970b368a7639b6a7897ef15207cd5646e31b5816611c31bd069473f513bcbdf9afe5fbcb56bf24689d95bdbe133c689a2759bbd936d3373251bee2f8205626a7a45b19df6ab5474df67c0c9d9b06f86def0bb597a82cf6c78c7a05ba1bde63d341dad0dacd127b03671ba86ffc28913ed175aec91a2e7f607c1016a52402b166f83f63f6ca3971c9ae03ad52c931a39ad2afaf0c1ea5113f209f86a5f1cf79b53a59199ac05bde7519f49b47abd578cb8c74d39fa18117ef96d74b1bfd0dc9487feefc3dad4cf4382991d099052683720c3a72cd1ab6d13d617983d8a56",
    ],
    inputs: &[
        CellFixture {
            capacity: "0x59682f000",
            lock_code_hash: "0xede6a3d80717c3d7927eea678d095abbe68dbb08ca6fdbbbdd9de906455a4afd",
            lock_hash_type: "type",
            lock_args: "0x",
            type_code_hash: Some("0xe0706b176678181d982290d93dfcd82098e60cceaa4a87f10f32dcbcc91df1d9"),
            type_hash_type: Some("type"),
            type_args: Some("0x2510c78057479c9b023fe6e98ce43979e92a1353"),
            data: "0x03a15b28707237cee42120cce959579a2af24b1a4f5e1f575e373f9b23afe5765000000000000000000000000000000000000000006731646e00d5026c2c742a379b0c422271a4e1f59b211a6c3158e6c6f873af57732daae458be3c56c2c847b14176332d66697273742d6e616d65",
        },
        CellFixture {
            capacity: "0x2540be400",
            lock_code_hash: "0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8",
            lock_hash_type: "type",
            lock_args: "0x87a174cb43b161c9d4329fd04e73f28943577220",
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            data: "0x9996e4ae282306bbf5724e5d24c5a3b9259f2248afc0f3f70335dbbe035abd93",
        },
        CellFixture {
            capacity: "0x4a8171e64",
            lock_code_hash: "0x147ecbb5c5127d982ee1362d2c2bb4267803da2eb006d150e88af6caaa0a7eaf",
            lock_hash_type: "data1",
            lock_args: "0x5ea09d41003bb714874a268c4ba2332bb2bf755d3e195970b7fd21e67e61f7da",
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            data: "0x",
        },
        CellFixture {
            capacity: "0x3cfa88ed8",
            lock_code_hash: "0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8",
            lock_hash_type: "type",
            lock_args: "0x87a174cb43b161c9d4329fd04e73f28943577220",
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            data: "0x",
        },
        CellFixture {
            capacity: "0x1b48eb57e000",
            lock_code_hash: "0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8",
            lock_hash_type: "type",
            lock_args: "0x87a174cb43b161c9d4329fd04e73f28943577220",
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            data: "0x",
        },
    ],
    input_outpoints: &[
        ("0xb13d64a39a4546aabb367e8415b3089bab0ad2b40a8fa7ff2f82c05eb7917324", 0),
        ("0xf9ae5a7bd1a89bc2f3d7ffc0ec9af1e2be0f2f570547c9c23225faa5b95c1fbc", 0),
        ("0x8d053fb0e71d6679405d7dd079cfa7c09b1bf21664f282848e287657485b4c98", 1),
        ("0xf9ae5a7bd1a89bc2f3d7ffc0ec9af1e2be0f2f570547c9c23225faa5b95c1fbc", 1),
        ("0xbf1aac18bf111efa2e99a35f2a28431974c66acb3a692e6f2ed18ca75cae1e1b", 0),
    ],
};

/// `T4_edit_manager` — testnet tx `0x2d0329838bc730479151d75f02b1c6f86072616532a71d97d915607f7f96aacd`
/// at block 22366352. 2026-09-24 from local node http://127.0.0.1:8124.
pub const T4_EDIT_MANAGER: TxFixture = TxFixture {
    label: "T4_edit_manager",
    network: "testnet",
    tx_hash: "0x2d0329838bc730479151d75f02b1c6f86072616532a71d97d915607f7f96aacd",
    block_number: 22366352,
    block_hash: "0x37ee6f26e685f089e88eefba39204c8e8673e89c39c9d4343446b50510a2ff6c",
    outputs: &[
        CellFixture {
            capacity: "0x59682f000",
            lock_code_hash: "0xede6a3d80717c3d7927eea678d095abbe68dbb08ca6fdbbbdd9de906455a4afd",
            lock_hash_type: "type",
            lock_args: "0x",
            type_code_hash: Some("0xe0706b176678181d982290d93dfcd82098e60cceaa4a87f10f32dcbcc91df1d9"),
            type_hash_type: Some("type"),
            type_args: Some("0x2510c78057479c9b023fe6e98ce43979e92a1353"),
            data: "0x03a15b28707237cee42120cce959579a2af24b1a4f5e1f575e373f9b23afe576500000000000000000000000000000000000000000e7fd826c00d5026c2c742a379b0c422271a4e1f59b211a6c3158e6c6f873af57732daae458be3c56c2c847b14176332d66697273742d6e616d65",
        },
        CellFixture {
            capacity: "0x4a81762f6",
            lock_code_hash: "0x147ecbb5c5127d982ee1362d2c2bb4267803da2eb006d150e88af6caaa0a7eaf",
            lock_hash_type: "data1",
            lock_args: "0x5ea09d41003bb714874a268c4ba2332bb2bf755d3e195970b7fd21e67e61f7da",
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            data: "0x",
        },
    ],
    witnesses: &[
        "0xbb0000001000000010000000200000000c000000656469745f6d616e616765729700000002000b616464726573732e333039006100636b7431717a646130637230386d38356863386a6c6e6670337a65723778756c656a79777434396b7432727230767468797761613530787773717638353936766b736133763879616776356c36703838387535666764746879677134337466346b2c0100000f70726f66696c652e77656273697465000a0063656c6c756c612e69642c010000",
        "0xe91e000010000000e91e0000e91e0000d51e000080010101634695c309ccc237d2effce999f326f9e3e1ff99542edf20ead57720cf48ff959771645d87bfb4b3a2467f5cab4e7544e9526b319bf9c61f6ef28744da59a10002c6513168681f991468f5154973fc5e4f2e8261e440a2221525d619094c3b0e876af7b09934d27ff81d9d1bed050633a7767339628b286cb1a92aaf2c83ed8a4396bd7505335a2f7676574808ca42b8ea764e55e7a9b6621c6eaf6e577cb3d59ae7546089d75fe7d4471c0838f2bbbaa7018f0ae3a35ec0fa10b465cca03a845eed8798f3b9e9d41180b3a8a738e9f54357b9be84257044234e5d8e92910bf87a5802fb1fb9322c9ec67a3e87f93b8f72ff8f62d61bd04e245a0d4f29985d6431834d3ae673c494e56201df8be113e15d65ca3e6e69c81482cd3d0bdf73fb0caeb0eed171e715b205d77f97c1161d856df20c210f7f2742af0fd082e4b735325ba29a0d4a637ffdadcb2f3d9b83b1617c7ce996b25f8c4c8ed5b5e02afca8dc5e8447962946f746ef7f47be29691308c6fce64f83f774264b76d770d28651d95b57ac9474fedf7bd454a7b6dc11b8b6ca841815f22cc2fdaa682b5c2e85f45cf37981b8242889383b0cf4a1ee673ef8edbdbd02bb526308f6dc53052c0043e9bd8d7f567ac1ebf43ff01b49fe99b1b8732eec9e0dcc823881b4aa6ad8c532ba828c8d0882ba16d47083103d5c050295f6ae19bc79f0a1b7597871e319094378c2a8c3757c58a600a03c0599e3bc726028ad295a4a42ee0d62ae9768ca7a4ccc86b47861a9af23600e377cfaf15e28963622e78e8abac54ae48070ecc0db95ad79946d1c011bedfd6fde1ff77783b041639dfcc13d75cf84edaf0ab905b13a18d424820230b1902419f8d38d4cc26b53a20e41b26a70fe4ecaa7ee55cf54236842d077ff92ef070b6d978abbb8ad5430e41da4bb599bff2b2b9f810796f78b86aa7db5c34c2d172ccf6857bddc5e08ca4fda50d0da0e38f49e6fcd5557f3df4c51782ca3bcd93b6af2d834dcca54457850998d8469f5afb0ca9c2706329abe4c155f5b17cd8186bd29e652344a089eea75de3fdbd0bf8a2df6233a999938a8f053311713cd2b14dc7c2a1b2107a3462ff865118ad4b99278ac22cf61ab947fd9a1f319e36b725009195fdf5b7aece99398d37799477c09a04802768dac2787262c35e4b201ba97289904bccb9de446a3bf2d86367b46fbca209639139e44ac1e83a2761f243b54a606ff7b7591a7b1481a6907a848416cc07ceaba65ef8e1cc3f7bcb94718c0ce4b219fe331b4079f45d4f7a617d8e5144df65faaaeb23e9489f37ec0d28594fae1b0da52639d0252ab3ae7d3efa467d7e68ef1d6b05c9be9706e636f7ea7afd6084ab3f7d6d81002d1721c9f331ee6f396f543daee96e14ca6399cc07a819191c1154e09d9198aa3cbc5ce68a33d6a59928ab514797593ad7e2b3ff1e075e24fca4357509d6d1f7f1af401d776ddcfd8f1b8be1a21a5f82c308c5434d2422948695b88bf53d63de9e625d59d34ee25561c6f3cbc0c2f7a9a99ab2927ab7338456f57aae09c10c636b1e8e7ab0c1ca94f8607163fe9bc3ae755f10d5e73ee4b7eeff7db17ec8ba7596a78fd6f0b70ed3e12fdbe657af1ebbcf9b15c375c4d8d38cd31af7c1aa88ab4ff848fe3855ec01ffe6021ad7d32fc8a60e756d135ca7dd50e424a2234512d4799926430b6207189fd30880680fa620d48f4a7ee4fd5f196734ae842340d089568b515eac3df304d3e2a87ce32bf1ffccec437fa7eda31225faa4b4d38cb0a3217b6cab1d75bca45c615c658f7f5fcce1fc2ea3b70784c65f33e6974d6f3354d3d1ae219cbcc813d81dcbf336ca18d9521b0ced88728dc87c921189f76d9d1d0c3ab9e33d199d8249d9d111db29a1724510135531caa4a79864ef6ab52c1ce050c4a2f08e4df1e1da8af19279516949ce28d4715ac4cfb6ffabaaf41a5f34b3ade4d0d7d764266ec45b397f580a8e2c7b9f07eab43d511629ced966718bc8ebde18b3df0a104ce60739e9c9ff56231cfd2573b33b0140783e87381aa3bad6e68a64295d50b27c26b27a8e00b79bab1683c1e4199bdbd2c9ad86a178a79821e06cc53f51a6cdd5be9e288820f492ae3caf9923fd9ffcda589b0195d632630c8d5e8e20c1c1c737d8ba6ec8cfad482454ce7a31b95ae554b9798046b400afb48649b2be0e14afa0947e7df1f4cddfd3050e93a8a3a1e31176e9df20051365ee1bf78260115f6cb6ab8701cb1c272681a21e01aaa11014595e3026578298b7c0a8e4bc801723bcb992cccc08993accb140f33e884665fb7fc705558e93cf417ffa780b8e11163995bd69bc86fccd3545330f83df692112d69146446d74e245e4ba83f2ba10f43a9d58af162ece550c0e2e335beeb526d4abc3780dde08ca1913a74da6c09740b9126bfc91d399f056a6dd4920c1a5c7027b91d7f98c24bace5cf0ac6286480d903dfe9f66f3ba434443818b0866b7e87b814ad0fe3069d012392b71c763cbd94c375a28b38418545d5e028a82c9c2d9ffc246b68cdbb0b4ae67c150aa6df02c7a35a417ede50b8c1c8dbc8614c3878e2c1906d423f4ec62c2b9325b3050df60cfdc8ec1238bf104f995bc8d9502535d52492041f4b1fc885141d37ae300af190379e6d8de9f1aa2ab3bd3806d6bc3dd26878161f4d0d0391ba9ee7bcf27fb4ffe6f4c609ac43f0fd6eaa4dfcd760f2d54493a517fa4bc4d9a93bd8c29a0c9e615057bf8038a3e1f13074e2b3f8794c396df3ed745229d83c454eb0ccacb292a078ca6a60a6fe12101304fbf22f79bc53036b606a171629268db4f8c4a095f7c19b17262930229aa9ed8c390d91d92f7d514e581056ced3ca327e3a82b2fb695bfa400437fa123d85c44c784ff5a323e776e092d407e8e58ea12e23aa62f84d6fab24d3557598518f08a6034a77e485d2c1b8c68990c9b7f1690130997b18b20a50efb1cb529eb935fd42a1a8b13b186171c35669488f54ebc9d2731609c67232a7fe32683636fb0584e01daad378ed8bad94f459581819d17fb9764e24f118448326b67f786593af1b57d30d64d4c0584b9c6804246d6bb9b54a825502707521280b01315f129f8bc679e4127a3dd3640ad07b81973f103a26b53229bae48a1725401566f0dfdd5673970099972de03429270be8c0ffbfcee5b69f4f0795b4bd1357526b69696e0189d3e976da6209316cf20f6f0729914c0f086d9776d8ca54aab411069309acdc2ae088e99733947561a66518a6c4a38afdf5eb8b70811337b4cd0391fe951da6f82744865a113872f9d677443ef6c4e05125a0f50f03d2db7ba0e603a3b0ee9fe3a5808d5d6b3eaf346db701f1b8fa80f7d5599cf024279951030d643204a3c69f886746e5059dad4158aa5e6a1788fdfc442a1d869644c8c27072ca94fa51a7cc10782dbf2eedc7ed67c83c7e8f04f0c9dd41b645eb80f93d6b865198b01e45e7b20ede3f512a6c1f37e578f1878e6a3a25db61c4d0c421c7cb9cb222074b04e29e8f4fc945b9ce99527070756184efd51152b3a5fc8465ea31127fcb81dc77a9f4d9128bfe232bf587e10e4b82596d355e55c3d07a6e96557d99e23886bf0ff4e0382e5d68af8c3fc2636dbde310eebef7604b2def5c0fe8a215b8bbf5fb239bdbe82e78e5963ba1fe0ff10e6ec7478b22e2a5099d5c12ebe988a605b769555b950458a635cb624fc922031c3c0d927b4455075a623cbf71b8de4a00342077921b71eabbba876aaa7f4459d6c74568e023dffbdd34bcf802e58efcb584dc3368034fef4bd10af99c35a9c7f943884920cce70effd0186093e5e00edd1764b928f20caa483eb21ce404a1c4364035cf110381c942dadc05f6921f82ef29fc9d61c623cff036541b6abc59b83f14ec0f99295cbff46b8a5c876b77c9a185a24743fdae0dc305c24e789426b7aeedff87843b38543ef058ccbc8402618b98418455c90a99a23871e33c90953246a1c597f0e6258ff0cccbad4f82ead585d8e435f48603d62c6cc9abdb2b462a7cf880ba5125b4a63dbb13723213aa3d2e7cc8fc86c500ff6cd6d67b40fff8410d13794e3abef69625bf83ff22707f98694fcc0dbeba2232e3acbfdce01b809e71b58dd510c1b933e3598668fe23e3d4e29ed2137bc2ad59d8592036f41366161fca318f5fdcc3e6aa6b8a27c92088dbc6a7ae66b7205cb7f2d2fd4b9584ff720264d9950fc4d427ce6d8b022a17ba4f36016153264e1f1a5bf700558cbf079f193e9469c77745725c940157fbf2289eea72aeffe1018f08ff10f86f21c43cc22ad640a36e428def7401ca3b2ec847efde2ce692cb7ddc4d7437aff971867c4db948e97a1a8332b104564ffff3efe264ad54d85ca762dd6849380dc44a571c91207908fae09ec3fc5a7edc1c2b334d30653b5b6feb62217e110bfc788d0300a49b6797c7c3bcca2bf41df7eb76e52a4a74e5048971bcd902d68a5a0b5c0c88710d0eeef9964b933bafe535b8fd93f6815630702763f2f4b047923537baa356dde3e2c87bd30cf7901c1338474b5efd5b7cf90788f00c512efc6912484f30ab9ff59b90f40fa7a6d01f191e062439f44f919a2e839b281273b5abaf1894473d433a7a55f310110ff28d93a726c18c86122fa3ab1ef1e0c235b8792e831caf795908af0a16c5d1222baed85337efed1aca695a39adb94ceb1374b48fea7ffe5b304f0d5c935bee8963d7cb0f468f584c9798385af568430fed672e87832a8352f85c4b32e1146016294b05dce65c44806ae42224b7f0953104548f915016c00503d4cc85eebe949cece70d09e177ec3d3ff5d331db4e81a52cbe7120c38ccee7cd105501dd2a5b8c9eda5e28cee3a719b44debf0858d170e57584fbed0b4a9fc718cc0eb07c8382c4b5afc58c6bc9ed51bf8f2d43327ed3f7c90a47db7a4a9e272c7f0f479ec73047abf88bc6bf94f3900193c215fa0b716d7bbb5d4e06e23c05cf544fa4dba21d4c8c467d15cc348f87bc190ff25e945a793eb1afc8e028f4010161724d5781050d5b5e77cce1475ffa6b668730780f17af17bc6253217242bf44f94b9c86d73c02c2439bbfd7bc04609ce7ce7c76fc7e73e71ac1c9840e976a18baaf176f07954c453fdf6a5d8b8928b31d90055917cd27e1a9ee6801e86f4b5a6fe51dd09b1d48a90a93dfcfb18dd933beef6949f05961d14b1c9311f8acb71866c8f7360281b6a498b71918877554a540c4125dbed42be92a16678f8abc6b681dbf92189b9d330edf2dc269e2cc2180755076e7272f30f1a6107e4731423dfcc0c0fac809893534a63bac3e0c5eecdc2d65964ade0d1fb6d63a7fda496257ddca96bea40e5b13afa5ccfdcfb9de193c353891329ca7b5cba9dca6ef82f20aee0fdc05356b3179dc27752258b66ee1c599cffbb59e73a633dd92fd776fb17663e4cadaed7ffea10a70bb51b78a402a1cdced7c319ee71cbb767451ca86176985d6bd6401c6e91e6ab8d6d5524dcaa69267ed60cd734a01b4c1de4f4a60c255ffdbc9d22e5f95ac16cecd64d886d0830ad57e3bf57e4796ae65eeb4a08e5858d389ea69039b634bd0bc6026d98a8f18a858388e5e0d568560faaee2f7ad5ceaf4cb819efc4dccf83df8d4ee76c5e9a30cedbc3def778eac5f40bc9223b00fd8091017b7bf510a33d3937525a4cc40f13db64316f36b23489a988b7c29ef0e6e201ab997f52ba5979d89912899c7f32d1281e0c0fff07e1e1816604a9196545f38502436bbf95826de0eed0ae1406c4bb3938a86dc386013da570bb808d19a0366bc383399f487a0354675286a77260bd520344dd132874b107c7c5e54752a90c88f54de694909b2d074b8cab9520842dfaa71c0558e12a28200fe7b608c7ff5c4305885aec645eb0bc86a37175052c8c921e011ecadb7282b6767f7315aa8aae1359f7e7c9b2146b8e485e50150d33ad388ab6a7f98f61105a53cec20d7536d7d4735ead00be9362f17f2d4b807d7dbe0722f1a7b869aeda1755ccc872e7e6b0071d6b0c1a13ea3a6bb4440d58c124f6797ad9f9405a1384614401c1b6550dc3e23b0d8d126bb7ab498d5df91077127a7fa14e86f29e25f15755beba949671b76dd33a274f6d30e4a5d2153f92707b59c5e3411f70fb5270359c0423f96fdb267e92dec8da9b790a053be7944436ab277551730e1b3f28ea2af35ab9295c38fb0a4b835295d44efc405a28938f20cd714719c418ca118103e0d0b935eb3902f81af824c18ed003da3297dd854153dacb541ec44be6cb8c6b0dcf879afa2a7e12f682c3298d5198a9b157a5fb631ff7695467e999e166157fd2f80dec25fa19b160b49761a1864f6e354313c2fa4738ede2877aa6a8f1f4af48d1ca337db1fda0efc7ffbe5171074020a97761e8bf5ba1a116f0e4205ce26b45279d52410f04dd232570c2a25bea3fe841797639315045cfba0c5399c4eb734cfcf1c133eafdf9dfb02340427ec5a7fb6575a5a18ec4bdb7764685c1f09c5eb2089d2f1f89d97f1da072239ffd7ce54537dbe0c783debf45a0c819b19a74aa3350ecd1cc0eb467d57024a25266d3c877238ea9465431c205b441178bbff8cac3792de0c1658bd619ce484a7091ab01bc50a765a5b90817bf2596416e962d73b0622237f31f919fbe539bdf8d793494e6ab9395b954f3ae1367f42eb327d51072e20be8741225c1069dde0d5a2bd60a729f553dbd3dd31cba00821a2db2ee83e8a541c95599b8a9a005b7017b9190625288687f9fffd1dd1250b935909584c0fde6985ae25717d91c49f4fe81dd9e34c6dc84fe2f040f80ee4dcd200bc6b2ef5366d20fb9eb06fc9343dc28de55058a8b4b397183e3c97b5b29923fbca1ed13c4e31253797773f6d2d67dbe636c4d1d2d795e9c3fe8d6cfc22e27a91c13fbae8e598588852548f31ec97c2964e403cf5ec883d6e27eac38d7151e02cede09e1ff81b2c986c4742bda9a5beb82bda7e44c8df9d289fd0d17ade8c07a5d54ea38bed313b116eb9963dcca15f4b87c23cc2b142cae7cb0c39f3547e33937cbadaef68a0bab3fb4b9ea4c6eeba609fc9a4d5990d1fb74981d29ed6622a7c72a19381133a1851d84e3b6e65d3b41f320302fab490241d4405232328b1eba221c4c227164cf2266b99c4eaa3f8918d5e0282d83e56a32bf0e86d6e9b8cb570e1b2ff97f372d961f508d5035e2e615381b9959d3647bc2fba22fe9ab61403c3358250208509fc7aacd64d252e8690e3ae019e6a9b84701d3abd840ed4fe6464247a98a0fc1248bb2b98edd3839eab8accebccc45e3bd46e74c42dab022b759a427fba8bbb43a2bbda90c69dcc9a6ff0d300cc17c03f8e4f49b653bd3bb22fe88bbe65eb4ab83414792874682ef47bc3c36142c51718f539fc11229841cb9a65e2ff4ea003148544d8e70d796301464c87a82c89a7caa9f7f3bdd6d96338d86b2cbcd9085c5088c1f664f82f2df15204d42aeaefaa538a824c7614d62746aa1fe8e0685d840c5b497db9e87b81792d6bc2f75705c25326267afa23c5df3a3e343b476a8b003ef534974dd095b112197188b06d75a60d2d4f2bbb2033dc2f9874b64ef4a04d6a138e2ea70ee759c3782200ca3571acbdaed18db1660d9e057dd89a63697af502248511748cf845bf1a03bc68cacb367c3bef786e045bbc44ed63a050f0316a633c2600cef383abffd021ce99b2a8c0818652799b58ae92079bb2b26181b2818be5ba547fdc00df20599d5737b56a631c7127082bec909675e432d064ed4f932b042951c831bbc9c17f2df2563e45ee6890762b98af50d6c3f287919244183b972171bdc44ac803a75d7b3f2862d2974b946c9900b0675c74ec45744829f1b95cf85e42e248b3438f7d6ca633428056fcbfe3dcb98494a893a45b82095e8356f3a4be31cd57bcb73519a6efe2dd4fcad399ba0fdb78fbf95b15503e29dc06488d45a9564275c66a0dfd857f730e6f2ab6f3d304e70406caa0dbccc89e269b657be87b9ed263bc188cb2b05967dc4305c5d00c62eb1144cecaf17567162b1c8971c37800596205f849d611cfd23c75d1b3d62567c78a32ca304980ba3e07602899aac33256c05f0c17a9adbdc8540ea79547e5924de694f3c7aa32123a4aab16394dfd61a7498733785ed7f5697651a42e5c06f8a77ea784132dcf17a638c80b65e6763b00c407a67dd6c8f7ac21380fd470f6a159696ddc83aec6d8799c78dd8e801bb7be83af2421994ee918520d53facf9de9b37108e204699cc25ee1034d2bab194577dfb9d34cf07d08e428299aa9aea2a5ce134a387d6f96e6d35d11866e4d0d70444d2ddebdf643dd6c74cbadd378a861f94d1e8f7e89678d55a1c3c3a255e1c8f24c43063646e46374950fe5a28c8e1d045d68360affcd969c2a6840a7b9029ca8225b474ade66f69fb1c86fe6a3ad2acf9e3a0ac9a59a7730767be43ad87101c51bbf271ffd435530d966ef380fdd7345a464014ced92694b02ca7e59cb62695a49ac45dc10e8e38818561abc2d141a8634400e308a592ee899a9888d38a61ca12413a22e46ea4e6e4541580193a6141687e6c9034033cbc2863335f89ee2011cea3d47c47b2e8716da04fc38c406f43cd33653efec45858bae1b568c34ba4c7628a2e6b1e4acde011ac5504752796d6fcd39485c72fcbcfe8bdb71d80f21fb2dee05c2f4ff2f9ec3659e712996fd87f064c6872e881d5170cff70f7c3eabf5d5a571ffe029edeb68943673d7eb8b0420b0d7edb1b100d768d344644a3fb3d2e8cb83c2805a7bf3c679a32762f2c192e156a647f31df3825918f7c41702f884062d30d83b57845728b218afffa11cc5d8356643331220dbbfca180fc01d82899c94851602e16198ef4d89eb8426b69e3fd98c2822ccb3521aeb583a9fbf41cd6d883a99cc332423994e1ec6d787dc5d74392521793e52297d61e50a5ad616682ddbcbae0a3d055789f1eaf197afd168b362f1b6d62fa83cce296ad732be37490108a9f89a54fcc445e3050fcb7a28fa33727d7c21802db31a701210d1aacf1aa38193d062316e0be900328a54b68cb3bf5cdb28f532d493aa7e36aeb5a35b74fc708f0d02debf0381a8af4d767ccbbd6d4527a151124f3b056b9fa8cf9516507aeb990c29a77f6b74c50f4634c9016a1eeb8d9e734ce3fb6442c0395c120a25da1c927b7a1ee87adaa97a7e1c40511f6503dd4c8df772e5b2e20eed21ba234fc791608c2bc37acfecb04771799e5184adf93fc73f38f9234b0edd083ec7c9c2f468e5e844b580c9a80404f258d49421159c0ce620b8bc04529c0b75aba50c104e335d2291e7bbf9d16c66a1fd4c2623441c4f49b80feb1c7ec115d8c7542ae8c303abbea70b615484f7daf3d15aa5450d2d5de44a7b6eecae5ef8b973ebd02d8b086bf6e2ca9ae0d995459c72fb015a00554f6e45794ef64b5a7d07887e5928b2df771b55359e732887cc182b8563be26ad86302407d14030c779dabf3670ac3182fb106fb1409a451416e9cc503bba9921060d9cbc307184b2f9cb3bf8a140b5ef759ad46f0d1d6514f6a036d48035a7db208619d948f5701b7f69a83645cb1a69366b07edceb6c4c4d81a4939b461985e237c58c2f3c4022f007c22515280b0f60dd6e709cae8f8c333ef6f8f8b53b8e4bffb5ff3ed7cdccce58d222e7c4226ecb07e5e030e2f2af6d354fca474644ad5586f4b1388e1b12e95e860581a97d63c903906111b336b7f27ab1d95eee24c9c973866d2d951fb3356d69655d004563a453f41daa1cb869f44848a97a8dcfaa5edf63670909d3b9cb812908e8502facde35254d65bdef4e648a2ba40f667276867f305acf6398f2788e649fae50341b20b0999ca7887df1e9edd56c6fbd643018946f9f581e9fa566512dd16d79c2c4a44b05cd4c1248dae7aab959931fd687722a16284dd02d3d887a2d50ebfcf6a4981928269ea16b76c7e1948b5bff05120eb4762352c53efb5c52c7cf2180fb914abbe150e58fcbd937f795b9f089efb98b26549c6aa057c6ea901dfecbe6c4d24051f15fbedf4318c330f2f448ad6f2da4eb9e548fd0ea5e114222e3758772d1c85b219820cb944cd1c57192b51bc3c1a73f2e857249f51d1516b49dbb7ca45cfc3d5a6a492f9609fb57c8085df873273310f18dd426b2400d6ce4f5e90b288850477bb84a5351e59e86ced6dd62226819ffc711056bb8a0e4b09d1013647fbca8087a988c35fc57f22399c9b031fc454800db7b6112c445a9314fc28f198441e0b2fb2d6d3e5ebc08c4a3e536d4ee6ff078b4879b18929b3fd6aab97f91682e2e3fc4dd22ed58faa5f3a5ad0663b452ae530ffa21435b1a14dc172420c9d38ef2e4d5f82404db7daaf547e5346a812c8698cdd78fd8e234b55a6163310e1c404c24e745ff989f1728db9984a7d6586ed9ea6b2971e2296300c507229bdf233f965b3577a0cac070d405c05851984ce7760b48a1580883663eb87eed31216bf65a9e3e3f8a3c9d82877b6e495c95c236a1cbd6deec1f733c1aa823174c1d2faa9532715a86901d2a54ae52aa9a5cfcb7c428ef40af137f0c0798dbf922128b77cfa2ef81114997eaf1502a527b32b72490bdd312e3c6542b6367e5eef63b89d9a70019b13c7673a1785269130e513ec39c441c75b53a177cbffcc9d3fa395a6806d0b8725b2ad5970c6f9da1f53b49da7d276d8991b59a816cfe1690b38b88005dbe462a9932f9aeae8bf68e8884267c4c5795773254d229f823ebb0c6343b0d62031cef9c8e5894cf6f67affe2b2849d53c8cf4f2d78e23375f1b8876e523752233f63af8c40f5f03cce87bd6d3409d48daecbcb1c601d96475d791c304c76c7cd43f699bf4e3ff5e048c81179f3be24a861275c14fadfa3b9e76654c863874b2328a3ad3d02b8a124bc309d41184f776c3cbeaf029329b9b792b44082c202c6f94b6affb9efd2620133cd77c6a89259fd5463f56c66200657d668642df3ea8ceb3da2cb9d6d04d15f1b27d2f94c241a17543146b35e5d55ee25bb06add38a2bdc9635a48db4244c0bea725b0d3e29138567bf7c1055e9b117165db4de27ad1d306550edf83b408c3c89315686215c77bca5a3eedbd44daffbc44c759cf56618fcb3824324497b5edca",
    ],
    inputs: &[
        CellFixture {
            capacity: "0x59682f000",
            lock_code_hash: "0xede6a3d80717c3d7927eea678d095abbe68dbb08ca6fdbbbdd9de906455a4afd",
            lock_hash_type: "type",
            lock_args: "0x",
            type_code_hash: Some("0xe0706b176678181d982290d93dfcd82098e60cceaa4a87f10f32dcbcc91df1d9"),
            type_hash_type: Some("type"),
            type_args: Some("0x2510c78057479c9b023fe6e98ce43979e92a1353"),
            data: "0x03a15b28707237cee42120cce959579a2af24b1a4f5e1f575e373f9b23afe576500000000000000000000000000000000000000000e7fd826c00d5026c2c742a379b0c422271a4e1f59b211a6c31d5026c2c742a379b0c422271a4e1f59b211a6c3176332d66697273742d6e616d65",
        },
        CellFixture {
            capacity: "0x4a817a790",
            lock_code_hash: "0x147ecbb5c5127d982ee1362d2c2bb4267803da2eb006d150e88af6caaa0a7eaf",
            lock_hash_type: "data1",
            lock_args: "0x5ea09d41003bb714874a268c4ba2332bb2bf755d3e195970b7fd21e67e61f7da",
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            data: "0x",
        },
    ],
    input_outpoints: &[
        ("0x08b662bbe41258171b98864c2966cfeaa619cfd89c79d3fe3f362bcc18d4c829", 0),
        ("0x9129954b29a08258ca0d77f9b98458b8fa0b09dfa46cbd830853a265f191db6e", 1),
    ],
};

/// `T5_renew` — testnet tx `0xb13d64a39a4546aabb367e8415b3089bab0ad2b40a8fa7ff2f82c05eb7917324`
/// at block 22367947. 2026-09-24 from local node http://127.0.0.1:8124.
pub const T5_RENEW: TxFixture = TxFixture {
    label: "T5_renew",
    network: "testnet",
    tx_hash: "0xb13d64a39a4546aabb367e8415b3089bab0ad2b40a8fa7ff2f82c05eb7917324",
    block_number: 22367947,
    block_hash: "0x9d7fb04c0d1a3a9f2eecbadaaf90d974acbcc78ef6d6423541fd681dfd6e59f6",
    outputs: &[
        CellFixture {
            capacity: "0x59682f000",
            lock_code_hash: "0xede6a3d80717c3d7927eea678d095abbe68dbb08ca6fdbbbdd9de906455a4afd",
            lock_hash_type: "type",
            lock_args: "0x",
            type_code_hash: Some("0xe0706b176678181d982290d93dfcd82098e60cceaa4a87f10f32dcbcc91df1d9"),
            type_hash_type: Some("type"),
            type_args: Some("0x2510c78057479c9b023fe6e98ce43979e92a1353"),
            data: "0x03a15b28707237cee42120cce959579a2af24b1a4f5e1f575e373f9b23afe5765000000000000000000000000000000000000000006731646e00d5026c2c742a379b0c422271a4e1f59b211a6c3158e6c6f873af57732daae458be3c56c2c847b14176332d66697273742d6e616d65",
        },
        CellFixture {
            capacity: "0x68c6171400",
            lock_code_hash: "0xd23761b364210735c19c60561d213fb3beae2fd6172743719eff6920e020baac",
            lock_hash_type: "type",
            lock_args: "0x000140911fa94eaef8c1d0eca81b23e1972ecb0548dc",
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            data: "0x",
        },
        CellFixture {
            capacity: "0x623b475b4",
            lock_code_hash: "0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8",
            lock_hash_type: "type",
            lock_args: "0x87a174cb43b161c9d4329fd04e73f28943577220",
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            data: "0x",
        },
    ],
    witnesses: &[
        "0xb40000001000000010000000190000000500000072656e65779700000002000b616464726573732e333039006100636b7431717a646130637230386d38356863386a6c6e6670337a65723778756c656a79777434396b7432727230767468797761613530787773717638353936766b736133763879616776356c36703838387535666764746879677134337466346b2c0100000f70726f66696c652e77656273697465000a0063656c6c756c612e69642c010000",
        "0x5500000010000000550000005500000041000000916d894a97b5b54bf58d47d1420bf373c0ef170ac7e8d1a65db51a0bf29e7cef02e4322f8cad829feb7336263efbd7ae9e0e6b70b35fb7e424f903a5cc8d7eda00",
    ],
    inputs: &[
        CellFixture {
            capacity: "0x59682f000",
            lock_code_hash: "0xede6a3d80717c3d7927eea678d095abbe68dbb08ca6fdbbbdd9de906455a4afd",
            lock_hash_type: "type",
            lock_args: "0x",
            type_code_hash: Some("0xe0706b176678181d982290d93dfcd82098e60cceaa4a87f10f32dcbcc91df1d9"),
            type_hash_type: Some("type"),
            type_args: Some("0x2510c78057479c9b023fe6e98ce43979e92a1353"),
            data: "0x03a15b28707237cee42120cce959579a2af24b1a4f5e1f575e373f9b23afe576500000000000000000000000000000000000000000e7fd826c00d5026c2c742a379b0c422271a4e1f59b211a6c3158e6c6f873af57732daae458be3c56c2c847b14176332d66697273742d6e616d65",
        },
        CellFixture {
            capacity: "0x104c533c00",
            lock_code_hash: "0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8",
            lock_hash_type: "type",
            lock_args: "0x87a174cb43b161c9d4329fd04e73f28943577220",
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            data: "0x",
        },
        CellFixture {
            capacity: "0x5e9d78557c",
            lock_code_hash: "0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8",
            lock_hash_type: "type",
            lock_args: "0x87a174cb43b161c9d4329fd04e73f28943577220",
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            data: "0x",
        },
    ],
    input_outpoints: &[
        ("0x8b168483302c1cabe36b8654b1d5284f3d3231342b7feed9b4c1472b8cff2455", 0),
        ("0x323b48cd3738c8da18d0d4f9ab0bf401802d508dac9b2cd0a04144c36245f07b", 0),
        ("0xb2404f9d79f8abc0a7104ad584a6bb58aa5b880585ecd2b2034b6065808f0245", 1),
    ],
};

/// `T6_list_cartaoprova` — testnet tx `0x534e57dac6bfee1569dcd4163e8ae3c5c4778060310109e0a15a7a43037f667e`
/// at block 22415210. 2026-09-24 from local node http://127.0.0.1:8124.
pub const T6_LIST_CARTAOPROVA: TxFixture = TxFixture {
    label: "T6_list_cartaoprova",
    network: "testnet",
    tx_hash: "0x534e57dac6bfee1569dcd4163e8ae3c5c4778060310109e0a15a7a43037f667e",
    block_number: 22415210,
    block_hash: "0xb95c00e8d43287d611dcf45cfc829e19ebccc6b3cef51a2bab040614b9e41940",
    outputs: &[
        CellFixture {
            capacity: "0x59682f000",
            lock_code_hash: "0xede6a3d80717c3d7927eea678d095abbe68dbb08ca6fdbbbdd9de906455a4afd",
            lock_hash_type: "type",
            lock_args: "0x",
            type_code_hash: Some("0xe0706b176678181d982290d93dfcd82098e60cceaa4a87f10f32dcbcc91df1d9"),
            type_hash_type: Some("type"),
            type_args: Some("0x2510c78057479c9b023fe6e98ce43979e92a1353"),
            data: "0x0372ad09e23868d88a8e85519ebeee56f60eda6c5a564e8a369a4c9d8ea29087e2b40a46d8ee587825ff41800694b4fb8f206663d21974866c00cb736f437a28b77ecb038cc147c2171ed83fc371cb736f437a28b77ecb038cc147c2171ed83fc37163617274616f70726f766133363935",
        },
        CellFixture {
            capacity: "0x3b9aca000",
            lock_code_hash: "0x498ab6b49b6b25b3c47fcea74bd8a4447bc4efda6417809152a846e058ad0ae4",
            lock_hash_type: "type",
            lock_args: "0x9d602bfc26415da790c79526703cbbc1e9267cbe4be38f8f98b769ad0a90e1a100e40b5402000000",
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            data: "0x490000001000000030000000310000009bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce80114000000adaec3261a8f17e0c2785990c02a7a3781635514",
        },
        CellFixture {
            capacity: "0x110b0e9d12",
            lock_code_hash: "0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8",
            lock_hash_type: "type",
            lock_args: "0xadaec3261a8f17e0c2785990c02a7a3781635514",
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            data: "0x",
        },
    ],
    witnesses: &[
        "0x2200000010000000100000001c000000080000007472616e73666572020000000000",
        "0x5500000010000000550000005500000041000000bc9e9a0ce25166209b063f37df79374a99ea6d868a24d0831e8f7f1d5b1ce8813c305af7608fa4ba12153fe9757c6bc99bf9a0876c3fe0e4212b592bf2f8873000",
    ],
    inputs: &[
        CellFixture {
            capacity: "0x59682f000",
            lock_code_hash: "0xede6a3d80717c3d7927eea678d095abbe68dbb08ca6fdbbbdd9de906455a4afd",
            lock_hash_type: "type",
            lock_args: "0x",
            type_code_hash: Some("0xe0706b176678181d982290d93dfcd82098e60cceaa4a87f10f32dcbcc91df1d9"),
            type_hash_type: Some("type"),
            type_args: Some("0x2510c78057479c9b023fe6e98ce43979e92a1353"),
            data: "0x0372ad09e23868d88a8e85519ebeee56f60eda6c5a564e8a369a4c9d8ea29087e2b40a46d8ee587825ff41800694b4fb8f206663d21974866c009d602bfc26415da790c79526703cbbc1e9267cbe9d602bfc26415da790c79526703cbbc1e9267cbe63617274616f70726f766133363935",
        },
        CellFixture {
            capacity: "0x14c4bb43ce",
            lock_code_hash: "0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8",
            lock_hash_type: "type",
            lock_args: "0xadaec3261a8f17e0c2785990c02a7a3781635514",
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            data: "0x",
        },
    ],
    input_outpoints: &[
        ("0xd860a1c79129f937c44792232bf5521496012b28f34967282465a1562dbbb1e3", 0),
        ("0xec7c0690466942c6a75c6de0eec3e544505e3b77bf131847f233f01c26565c77", 1),
    ],
};

/// `T7_buy_cartaoprova` — testnet tx `0xb891cae3b6ac8bd8829f3a0dfd0fed5eff30e07cfd295f62dd4e4c581c3110ba`
/// at block 22415215. 2026-09-24 from local node http://127.0.0.1:8124.
pub const T7_BUY_CARTAOPROVA: TxFixture = TxFixture {
    label: "T7_buy_cartaoprova",
    network: "testnet",
    tx_hash: "0xb891cae3b6ac8bd8829f3a0dfd0fed5eff30e07cfd295f62dd4e4c581c3110ba",
    block_number: 22415215,
    block_hash: "0x3087dfe6f0d057fec4629d0d0deba19a43fd59d647a1d0e0277f49a837a07f09",
    outputs: &[
        CellFixture {
            capacity: "0x59682f000",
            lock_code_hash: "0xede6a3d80717c3d7927eea678d095abbe68dbb08ca6fdbbbdd9de906455a4afd",
            lock_hash_type: "type",
            lock_args: "0x",
            type_code_hash: Some("0xe0706b176678181d982290d93dfcd82098e60cceaa4a87f10f32dcbcc91df1d9"),
            type_hash_type: Some("type"),
            type_args: Some("0x2510c78057479c9b023fe6e98ce43979e92a1353"),
            data: "0x0372ad09e23868d88a8e85519ebeee56f60eda6c5a564e8a369a4c9d8ea29087e2b40a46d8ee587825ff41800694b4fb8f206663d21974866c0058e6c6f873af57732daae458be3c56c2c847b14158e6c6f873af57732daae458be3c56c2c847b14163617274616f70726f766133363935",
        },
        CellFixture {
            capacity: "0x2540be400",
            lock_code_hash: "0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8",
            lock_hash_type: "type",
            lock_args: "0xadaec3261a8f17e0c2785990c02a7a3781635514",
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            data: "0x",
        },
        CellFixture {
            capacity: "0x5cb22b3b587",
            lock_code_hash: "0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8",
            lock_hash_type: "type",
            lock_args: "0x87a174cb43b161c9d4329fd04e73f28943577220",
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            data: "0x",
        },
    ],
    witnesses: &[
        "0x2200000010000000100000001c000000080000007472616e73666572020000000000",
        "0x",
        "0x5500000010000000550000005500000041000000479a53f191e3607f4b1249486b1cd4237d69f82cd539f6d434ff54557e7e20053c4eea8a7ed8a52b5ac33d33757911e5351a7bde0848695b11d5d15d8029d05800",
    ],
    inputs: &[
        CellFixture {
            capacity: "0x59682f000",
            lock_code_hash: "0xede6a3d80717c3d7927eea678d095abbe68dbb08ca6fdbbbdd9de906455a4afd",
            lock_hash_type: "type",
            lock_args: "0x",
            type_code_hash: Some("0xe0706b176678181d982290d93dfcd82098e60cceaa4a87f10f32dcbcc91df1d9"),
            type_hash_type: Some("type"),
            type_args: Some("0x2510c78057479c9b023fe6e98ce43979e92a1353"),
            data: "0x0372ad09e23868d88a8e85519ebeee56f60eda6c5a564e8a369a4c9d8ea29087e2b40a46d8ee587825ff41800694b4fb8f206663d21974866c00cb736f437a28b77ecb038cc147c2171ed83fc371cb736f437a28b77ecb038cc147c2171ed83fc37163617274616f70726f766133363935",
        },
        CellFixture {
            capacity: "0x3b9aca000",
            lock_code_hash: "0x498ab6b49b6b25b3c47fcea74bd8a4447bc4efda6417809152a846e058ad0ae4",
            lock_hash_type: "type",
            lock_args: "0x9d602bfc26415da790c79526703cbbc1e9267cbe4be38f8f98b769ad0a90e1a100e40b5402000000",
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            data: "0x490000001000000030000000310000009bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce80114000000adaec3261a8f17e0c2785990c02a7a3781635514",
        },
        CellFixture {
            capacity: "0x5c9bd13002b",
            lock_code_hash: "0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8",
            lock_hash_type: "type",
            lock_args: "0x87a174cb43b161c9d4329fd04e73f28943577220",
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            data: "0x",
        },
    ],
    input_outpoints: &[
        ("0x534e57dac6bfee1569dcd4163e8ae3c5c4778060310109e0a15a7a43037f667e", 0),
        ("0x534e57dac6bfee1569dcd4163e8ae3c5c4778060310109e0a15a7a43037f667e", 1),
        ("0x974fc983a62a6f7b977c6e2170695cc4aafc5a7544c1aa2513574f1563fa0fad", 1),
    ],
};

/// `T8_cancel_cartaoprova` — testnet tx `0xa5cc0a4445d338253815311ecb61ef5a97e09fe32efcd420f5d2572c76e80132`
/// at block 22415222. 2026-09-24 from local node http://127.0.0.1:8124.
pub const T8_CANCEL_CARTAOPROVA: TxFixture = TxFixture {
    label: "T8_cancel_cartaoprova",
    network: "testnet",
    tx_hash: "0xa5cc0a4445d338253815311ecb61ef5a97e09fe32efcd420f5d2572c76e80132",
    block_number: 22415222,
    block_hash: "0x3b82825cd74753ff08dff520f90f34d9ca4fe4daaa80149dab02e9a1e602fd12",
    outputs: &[
        CellFixture {
            capacity: "0x59682f000",
            lock_code_hash: "0xede6a3d80717c3d7927eea678d095abbe68dbb08ca6fdbbbdd9de906455a4afd",
            lock_hash_type: "type",
            lock_args: "0x",
            type_code_hash: Some("0xe0706b176678181d982290d93dfcd82098e60cceaa4a87f10f32dcbcc91df1d9"),
            type_hash_type: Some("type"),
            type_args: Some("0x2510c78057479c9b023fe6e98ce43979e92a1353"),
            data: "0x0372ad09e23868d88a8e85519ebeee56f60eda6c5a564e8a369a4c9d8ea29087e2b40a46d8ee587825ff41800694b4fb8f206663d21974866c0058e6c6f873af57732daae458be3c56c2c847b14158e6c6f873af57732daae458be3c56c2c847b14163617274616f70726f766133363935",
        },
        CellFixture {
            capacity: "0x5cb22b3a901",
            lock_code_hash: "0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8",
            lock_hash_type: "type",
            lock_args: "0x87a174cb43b161c9d4329fd04e73f28943577220",
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            data: "0x",
        },
    ],
    witnesses: &[
        "0x2200000010000000100000001c000000080000007472616e73666572020000000000",
        "0x",
        "0x550000001000000055000000550000004100000081bbebee266a16563e4397797d6e9d6eb4804469d8a0a1b9fd16e5794a96639c76fc5872064e94061b8caf37caf1f954e364bd8ba6b4235ccf201c18cd676a0001",
    ],
    inputs: &[
        CellFixture {
            capacity: "0x59682f000",
            lock_code_hash: "0xede6a3d80717c3d7927eea678d095abbe68dbb08ca6fdbbbdd9de906455a4afd",
            lock_hash_type: "type",
            lock_args: "0x",
            type_code_hash: Some("0xe0706b176678181d982290d93dfcd82098e60cceaa4a87f10f32dcbcc91df1d9"),
            type_hash_type: Some("type"),
            type_args: Some("0x2510c78057479c9b023fe6e98ce43979e92a1353"),
            data: "0x0372ad09e23868d88a8e85519ebeee56f60eda6c5a564e8a369a4c9d8ea29087e2b40a46d8ee587825ff41800694b4fb8f206663d21974866c00e9d1bfb8a04cb4323b0b2514635dd85ba88161c1e9d1bfb8a04cb4323b0b2514635dd85ba88161c163617274616f70726f766133363935",
        },
        CellFixture {
            capacity: "0x3b9aca000",
            lock_code_hash: "0x498ab6b49b6b25b3c47fcea74bd8a4447bc4efda6417809152a846e058ad0ae4",
            lock_hash_type: "type",
            lock_args: "0x58e6c6f873af57732daae458be3c56c2c847b14123b09908c29da905e754fc4200e40b5402000000",
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            data: "0x490000001000000030000000310000009bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8011400000087a174cb43b161c9d4329fd04e73f28943577220",
        },
        CellFixture {
            capacity: "0x5c769070ecb",
            lock_code_hash: "0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8",
            lock_hash_type: "type",
            lock_args: "0x87a174cb43b161c9d4329fd04e73f28943577220",
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            data: "0x",
        },
    ],
    input_outpoints: &[
        ("0x89799c70c41c2fd81adda36d627b20da40f2a0b8c7fddbc4a6b7d1784916df94", 0),
        ("0x89799c70c41c2fd81adda36d627b20da40f2a0b8c7fddbc4a6b7d1784916df94", 1),
        ("0x89799c70c41c2fd81adda36d627b20da40f2a0b8c7fddbc4a6b7d1784916df94", 2),
    ],
};

/// `T9_touch_ref43euew` — testnet tx `0x93813914fa8576979764947b23314d6e176e016e090e6937f6128dd7a4b9131e`
/// at block 22492487. 2026-09-24 from local node http://127.0.0.1:8124.
pub const T9_TOUCH_REF43EUEW: TxFixture = TxFixture {
    label: "T9_touch_ref43euew",
    network: "testnet",
    tx_hash: "0x93813914fa8576979764947b23314d6e176e016e090e6937f6128dd7a4b9131e",
    block_number: 22492487,
    block_hash: "0xc436644f46b525a1de85932aade14fdcc9e41f45c26ea7503d217459dd4823c9",
    outputs: &[
        CellFixture {
            capacity: "0x59682f000",
            lock_code_hash: "0xede6a3d80717c3d7927eea678d095abbe68dbb08ca6fdbbbdd9de906455a4afd",
            lock_hash_type: "type",
            lock_args: "0x",
            type_code_hash: Some("0xe0706b176678181d982290d93dfcd82098e60cceaa4a87f10f32dcbcc91df1d9"),
            type_hash_type: Some("type"),
            type_args: Some("0x2510c78057479c9b023fe6e98ce43979e92a1353"),
            data: "0x03f2e839126ec95b438660d7a74572b783cbf0a219a994882551b3edd82740cb38ee93d0606516f4ed3125117ceddaa7b71f288d21c7c28b6c0058e6c6f873af57732daae458be3c56c2c847b14158e6c6f873af57732daae458be3c56c2c847b141726566343365756577",
        },
        CellFixture {
            capacity: "0x17a8f460fc3",
            lock_code_hash: "0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8",
            lock_hash_type: "type",
            lock_args: "0x87a174cb43b161c9d4329fd04e73f28943577220",
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            data: "0x",
        },
    ],
    witnesses: &[
        "0xd60000001000000010000000200000000c000000656469745f7265636f726473b200000002000b616464726573732e333039006100636b7431717a646130637230386d38356863386a6c6e6670337a65723778756c656a79777434396b7432727230767468797761613530787773717638353936766b736133763879616776356c36703838387535666764746879677134337466346b2c0100000a616464726573732e3630002a003078656565656565656565656565656565656565656565656565656565656565656565656565656565652c010000",
        "0x55000000100000005500000055000000410000006c3496f71e9b5ab93fb765e117b07cb6209daa6379086300ff5dc7bf963493f01c84d06b4a19999a7978011c7cc46b0bc678f5ef7629c7b91eae3523fe0864f001",
    ],
    inputs: &[
        CellFixture {
            capacity: "0x59682f000",
            lock_code_hash: "0xede6a3d80717c3d7927eea678d095abbe68dbb08ca6fdbbbdd9de906455a4afd",
            lock_hash_type: "type",
            lock_args: "0x",
            type_code_hash: Some("0xe0706b176678181d982290d93dfcd82098e60cceaa4a87f10f32dcbcc91df1d9"),
            type_hash_type: Some("type"),
            type_args: Some("0x2510c78057479c9b023fe6e98ce43979e92a1353"),
            data: "0x03f2e839126ec95b438660d7a74572b783cbf0a219a994882551b3edd82740cb38ee93d0606516f4ed3125117ceddaa7b71f288d21c7c28b6c0058e6c6f873af57732daae458be3c56c2c847b14158e6c6f873af57732daae458be3c56c2c847b141726566343365756577",
        },
        CellFixture {
            capacity: "0x17a8f46174b",
            lock_code_hash: "0x9bd7e06f3ecf4be0f2fcd2188b23f1b9fcc88e5d4b65a8637b17723bbda3cce8",
            lock_hash_type: "type",
            lock_args: "0x87a174cb43b161c9d4329fd04e73f28943577220",
            type_code_hash: None,
            type_hash_type: None,
            type_args: None,
            data: "0x",
        },
    ],
    input_outpoints: &[
        ("0xe1a06b45ed78d2b4c2bacf3f34206e2ed2a1a179bf5451498e23aa1888b6d5c4", 0),
        ("0xe1a06b45ed78d2b4c2bacf3f34206e2ed2a1a179bf5451498e23aa1888b6d5c4", 1),
    ],
};

// ── Convenience handles for the cells the parser tests decode ──────────────

/// The mainnet ring root cell (empty label, zero owner/manager/next).
pub const M1_OUT0_DATA: &str = M1_RING_ROOT.outputs[0].data;
/// `support.cell` as registered on mainnet, with its own-index witness.
pub const M2_OUT1_DATA: &str = M2_REGISTER_SUPPORT.outputs[1].data;
pub const M2_WITNESS_1: &str = M2_REGISTER_SUPPORT.witnesses[1];
/// The ring predecessor `cellula.cell` re-created by that registration.
pub const M2_OUT0_DATA: &str = M2_REGISTER_SUPPORT.outputs[0].data;
/// `maria.cell` on testnet: six records in the witness at its own index.
pub const T2_OUT0_DATA: &str = T2_REGISTER_JOAOM.outputs[0].data;
pub const T2_WITNESS_0: &str = T2_REGISTER_JOAOM.witnesses[0];
/// `joaom.cell`, the name that registration created.
pub const T2_OUT1_DATA: &str = T2_REGISTER_JOAOM.outputs[1].data;
pub const T2_WITNESS_1: &str = T2_REGISTER_JOAOM.witnesses[1];
/// `shop.v3-first-name.cell`, a sub-name.
pub const T3_OUT1_DATA: &str = T3_REGISTER_SUBNAME.outputs[1].data;
/// The Sale Lock instance created by the testnet listing: args are
/// `seller_lock_hash(32) ‖ price_shannons(u64 LE)`.
pub const T6_SALE_LOCK_ARGS: &str = T6_LIST_CARTAOPROVA.outputs[1].lock_args;

/// The mainnet ring root as `(CellOutput, data hex)`.
pub fn m1_out0() -> (CellOutput, &'static str) {
    M1_RING_ROOT.outputs[0].cell()
}

/// `support.cell` as `(CellOutput, data hex)`.
pub fn m2_out1() -> (CellOutput, &'static str) {
    M2_REGISTER_SUPPORT.outputs[1].cell()
}

/// `maria.cell` as `(CellOutput, data hex)`.
pub fn t2_out0() -> (CellOutput, &'static str) {
    T2_REGISTER_JOAOM.outputs[0].cell()
}

/// The testnet listing's Sale Lock script, whose hash's first 20 bytes are the
/// listed name's owner.
pub fn t6_sale_lock_script() -> Script {
    T6_LIST_CARTAOPROVA.outputs[1].lock_script()
}

// ── Molecule form, for the binary bulk-build path ──────────────────────────

impl CellFixture {
    /// The cell as the node's own molecule `CellOutput`.
    pub fn packed_output(&self) -> ckb_types::packed::CellOutput {
        use ckb_types::{bytes::Bytes, packed, prelude::*};

        let script = |code_hash: &str, hash_type: &str, args: &str| {
            packed::Script::new_builder()
                .code_hash(
                    packed::Byte32::from_slice(&crate::rpc::parse_hex_to_bytes(code_hash))
                        .expect("32-byte code hash"),
                )
                .hash_type(packed::Byte::new(
                    crate::parser::ScriptParser::parse_hash_type(hash_type),
                ))
                .args(Bytes::from(crate::rpc::parse_hex_to_bytes(args)).pack())
                .build()
        };

        let capacity =
            u64::from_str_radix(self.capacity.trim_start_matches("0x"), 16).expect("hex capacity");
        packed::CellOutput::new_builder()
            .capacity(packed::Uint64::from_slice(&capacity.to_le_bytes()).expect("u64"))
            .lock(script(
                self.lock_code_hash,
                self.lock_hash_type,
                self.lock_args,
            ))
            .type_(
                self.type_code_hash
                    .map(|code_hash| {
                        script(
                            code_hash,
                            self.type_hash_type.expect("type hash_type"),
                            self.type_args.expect("type args"),
                        )
                    })
                    .pack(),
            )
            .build()
    }
}

impl TxFixture {
    /// The transaction as the node's own molecule `Transaction`: outputs,
    /// outputs_data and witnesses, which is everything the binary bulk-build
    /// facts path reads from a transaction's outputs side.
    pub fn packed_transaction(&self) -> ckb_types::packed::Transaction {
        self.packed_transaction_with_witnesses(self.witnesses)
    }

    /// Same, with the witness list replaced — used to prove the facts path
    /// refuses a name cell whose records payload is missing.
    pub fn packed_transaction_with_witnesses(
        &self,
        witnesses: &[&str],
    ) -> ckb_types::packed::Transaction {
        use ckb_types::{bytes::Bytes, packed, prelude::*};

        let mut outputs = packed::CellOutputVec::new_builder();
        let mut outputs_data = packed::BytesVec::new_builder();
        for cell in self.outputs {
            outputs = outputs.push(cell.packed_output());
            outputs_data =
                outputs_data.push(Bytes::from(crate::rpc::parse_hex_to_bytes(cell.data)).pack());
        }
        let mut witness_vec = packed::BytesVec::new_builder();
        for witness in witnesses {
            witness_vec =
                witness_vec.push(Bytes::from(crate::rpc::parse_hex_to_bytes(witness)).pack());
        }
        packed::Transaction::new_builder()
            .raw(
                packed::RawTransaction::new_builder()
                    .outputs(outputs.build())
                    .outputs_data(outputs_data.build())
                    .build(),
            )
            .witnesses(witness_vec.build())
            .build()
    }
}
