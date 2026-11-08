//! Small, allocation-friendly on-disk encoding for byte records.

use alloc::vec::Vec;

const MAGIC: &[u8; 4] = b"RUST";
const VERSION: u8 = 1;
const HEADER: usize = 13;

pub fn encode(data: &[u8]) -> Option<Vec<u8>> {
    let len = u32::try_from(data.len()).ok()?;
    let mut out = Vec::with_capacity(HEADER + data.len());
    out.extend_from_slice(MAGIC);
    out.push(VERSION);
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(&checksum(data).to_le_bytes());
    out.extend_from_slice(data);
    Some(out)
}

pub fn decode(bytes: &[u8]) -> Option<&[u8]> {
    if bytes.len() < HEADER || &bytes[..4] != MAGIC || bytes[4] != VERSION { return None; }
    let len = u32::from_le_bytes(bytes[5..9].try_into().ok()?) as usize;
    let sum = u32::from_le_bytes(bytes[9..13].try_into().ok()?);
    let end = HEADER.checked_add(len)?;
    let data = bytes.get(HEADER..end)?;
    (checksum(data) == sum).then_some(data)
}

fn checksum(data: &[u8]) -> u32 {
    data.iter().fold(2166136261u32, |hash, byte| {
        hash.wrapping_mul(16777619) ^ u32::from(*byte)
    })
}