use spark_transport::RxToken;

/// Encode `(chan_id << 32) | gen`.
#[inline]
pub fn encode(chan_id: u32, gen: u32) -> RxToken {
    RxToken(((chan_id as u64) << 32) | (gen as u64))
}

/// Decode `(chan_id, gen)` from `(chan_id<<32)|gen`.
#[inline]
pub fn decode(tok: RxToken) -> (u32, u32) {
    let v = tok.0;
    ((v >> 32) as u32, (v & 0xFFFF_FFFF) as u32)
}
