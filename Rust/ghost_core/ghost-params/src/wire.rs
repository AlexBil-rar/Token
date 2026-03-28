// ghost-params/src/wire.rs

pub const WIRE_VERSION: u8 = 1;
pub const WIRE_MAGIC: [u8; 4] = [0x47, 0x48, 0x53, 0x54];
pub const MAX_WIRE_PAYLOAD: usize = 1 * 1024 * 1024;