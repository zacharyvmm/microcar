//! Trace normalization + stable hashing.
//!
//! The determinism lane hashes a scenario's trace and asserts the hash is
//! identical across repeated runs. We use FNV-1a (64-bit) rather than
//! `std::hash::DefaultHasher` because FNV-1a is a fixed, well-defined algorithm
//! whose output is stable across Rust versions and platforms — important for a
//! hash that may be recorded in CI artifacts.
//!
//! Normalization is deliberately **conservative**: microcar's trace output is
//! already deterministic (pointer values were removed upstream), so we only
//! trim trailing whitespace and drop blank lines. That keeps meaningful,
//! deterministic tokens (CAN IDs like `0x0102`, virtual timestamps) intact
//! while making the hash robust to incidental trailing-whitespace churn.

const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// Conservatively normalize trace lines: trim trailing whitespace and drop
/// empty lines. Idempotent.
pub fn normalize_trace(lines: &[String]) -> Vec<String> {
    lines
        .iter()
        .map(|l| l.trim_end().to_string())
        .filter(|l| !l.is_empty())
        .collect()
}

/// FNV-1a (64-bit) hash of the given lines, joined with `\n`, as lowercase hex.
///
/// An empty input hashes to the FNV offset basis (`cbf29ce484222325`).
pub fn trace_hash(lines: &[String]) -> String {
    let mut hash: u64 = FNV_OFFSET_BASIS;
    for (i, line) in lines.iter().enumerate() {
        if i > 0 {
            hash ^= b'\n' as u64;
            hash = hash.wrapping_mul(FNV_PRIME);
        }
        for &byte in line.as_bytes() {
            hash ^= byte as u64;
            hash = hash.wrapping_mul(FNV_PRIME);
        }
    }
    format!("{hash:016x}")
}

/// Normalize the trace, then hash it. This is the canonical hash used by the
/// determinism check and the summary.
pub fn normalized_hash(lines: &[String]) -> String {
    trace_hash(&normalize_trace(lines))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(items: &[&str]) -> Vec<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn empty_trace_hashes_to_fnv_offset_basis() {
        assert_eq!(trace_hash(&[]), "cbf29ce484222325");
    }

    #[test]
    fn hash_is_stable_and_order_sensitive() {
        let a = v(&["[machine.1] 10 x", "[machine.1] 20 y"]);
        assert_eq!(trace_hash(&a), trace_hash(&a.clone()));
        let b = v(&["[machine.1] 20 y", "[machine.1] 10 x"]);
        assert_ne!(trace_hash(&a), trace_hash(&b));
    }

    #[test]
    fn normalize_is_idempotent_and_trims() {
        let raw = v(&["  a  ", "", "b\t"]);
        let n1 = normalize_trace(&raw);
        assert_eq!(n1, v(&["  a", "b"]));
        assert_eq!(normalize_trace(&n1), n1);
    }

    #[test]
    fn normalized_hash_ignores_trailing_ws_and_blanks() {
        let a = v(&["line one", "line two"]);
        let b = v(&["line one   ", "", "line two\t"]);
        assert_eq!(normalized_hash(&a), normalized_hash(&b));
    }
}
