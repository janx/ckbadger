use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(i16)]
pub enum ScriptHashType {
    Data = 0,
    Type = 1,
    Data1 = 2,
    Data2 = 4,
}

impl From<i16> for ScriptHashType {
    fn from(value: i16) -> Self {
        match value {
            0 => Self::Data,
            1 => Self::Type,
            2 => Self::Data1,
            4 => Self::Data2,
            _ => Self::Data,
        }
    }
}

impl From<ScriptHashType> for i16 {
    fn from(value: ScriptHashType) -> Self {
        value as i16
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Script {
    pub code_hash: Vec<u8>,
    pub hash_type: ScriptHashType,
    pub args: Vec<u8>,
}

impl Script {
    pub fn new(code_hash: Vec<u8>, hash_type: ScriptHashType, args: Vec<u8>) -> Self {
        Self {
            code_hash,
            hash_type,
            args,
        }
    }

    pub fn code_hash_hex(&self) -> String {
        format!("0x{}", hex::encode(&self.code_hash))
    }

    pub fn args_hex(&self) -> String {
        format!("0x{}", hex::encode(&self.args))
    }

    pub fn compute_hash(&self) -> Vec<u8> {
        use blake2b_rs::Blake2bBuilder;

        let mut hasher = Blake2bBuilder::new(32)
            .personal(b"ckb-default-hash")
            .build();

        hasher.update(&self.code_hash);
        hasher.update(&[self.hash_type as u8]);
        hasher.update(&self.args);

        let mut hash = vec![0u8; 32];
        hasher.finalize(&mut hash);
        hash
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScriptInfo {
    pub script_hash: Vec<u8>,
    pub code_hash: Vec<u8>,
    pub hash_type: ScriptHashType,
    pub cells_count: i64,
    pub capacity_sum: String,
}
