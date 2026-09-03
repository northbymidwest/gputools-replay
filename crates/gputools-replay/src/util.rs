//! Small shared helpers used by the crate's byte parsers and error paths.

/// Read a little-endian `u16` at byte offset `o`. Callers validate that `b`
/// is long enough before calling; an out-of-range `o` panics.
pub(crate) fn u16_at(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes(b[o..o + 2].try_into().unwrap())
}

/// Read a little-endian `u32` at byte offset `o`. See [`u16_at`] for the
/// length precondition.
pub(crate) fn u32_at(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes(b[o..o + 4].try_into().unwrap())
}

/// Read a little-endian `u64` at byte offset `o`. See [`u16_at`] for the
/// length precondition.
pub(crate) fn u64_at(b: &[u8], o: usize) -> u64 {
    u64::from_le_bytes(b[o..o + 8].try_into().unwrap())
}

/// Trim a replayer error description to a readable length. The framework's
/// descriptions embed a whole call stack; keep enough to name the failure
/// without turning a diagnostic into a page of frames.
pub(crate) fn truncate(s: &str) -> String {
    const LIMIT: usize = 512;
    if s.chars().nth(LIMIT).is_none() {
        return s.to_owned();
    }
    let mut out: String = s.chars().take(LIMIT).collect();
    out.push_str(" ... (truncated)");
    out
}
