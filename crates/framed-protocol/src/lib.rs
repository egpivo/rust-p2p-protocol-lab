pub const MAX_PAYLOAD: u32 = 64 * 1024;

pub fn encode_frame(payload: &[u8]) -> Vec<u8> {
    let len = payload.len() as u32;
    let mut out = Vec::with_capacity(4 + len as usize);
    out.extend_from_slice(&len.to_be_bytes());
    out.extend_from_slice(payload);
    out
}
