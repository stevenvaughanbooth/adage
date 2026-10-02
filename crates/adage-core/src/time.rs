//! Wall-clock time, as every record Adage writes stamps it.

/// Milliseconds since the Unix epoch (0 if the clock is before it).
pub fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}
