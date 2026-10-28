//! Deterministic fixtures for CKB execution tests.
use ckb_types::bytes::Bytes;

// A complete RV64 ELF: return the requested exit code via Linux exit(93).
pub fn program(exit: u8) -> Bytes {
    let mut elf = vec![0_u8; 132];
    elf[..7].copy_from_slice(b"\x7fELF\x02\x01\x01");
    elf[16..18].copy_from_slice(&2_u16.to_le_bytes());
    elf[18..20].copy_from_slice(&243_u16.to_le_bytes());
    elf[20..24].copy_from_slice(&1_u32.to_le_bytes());
    elf[24..32].copy_from_slice(&0x10078_u64.to_le_bytes());
    elf[32..40].copy_from_slice(&64_u64.to_le_bytes());
    elf[52..54].copy_from_slice(&64_u16.to_le_bytes());
    elf[54..56].copy_from_slice(&56_u16.to_le_bytes());
    elf[56..58].copy_from_slice(&1_u16.to_le_bytes());
    elf[64..68].copy_from_slice(&1_u32.to_le_bytes());
    elf[68..72].copy_from_slice(&5_u32.to_le_bytes());
    elf[80..88].copy_from_slice(&0x10000_u64.to_le_bytes());
    elf[96..104].copy_from_slice(&132_u64.to_le_bytes());
    elf[104..112].copy_from_slice(&132_u64.to_le_bytes());
    elf[112..120].copy_from_slice(&4096_u64.to_le_bytes());
    elf[120..124].copy_from_slice(&(0x00000513_u32 | (u32::from(exit) << 20)).to_le_bytes());
    elf[124..128].copy_from_slice(&0x05d00893_u32.to_le_bytes());
    elf[128..132].copy_from_slice(&0x00000073_u32.to_le_bytes());
    elf.into()
}
