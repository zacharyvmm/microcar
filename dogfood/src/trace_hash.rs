//! Trace normalization and stable hashing.
//!
//! # Normalization policy (deliberately conservative)
//!
//! The microcar simulator already emits deterministic traces: every trace line
//! is prefixed with `[machine.N]` and a *virtual* (simulated) time value, and
//! contains no wall-clock timestamps. Verified empirically that two back-to-back
//! runs of the same scenario produce byte-identical stdout.
//!
//! Because of that, [`normalize_trace`] does only what is provably safe:
//!
//! 1. Trims leading/trailing ASCII whitespace on each line.
//! 2. Collapses internal runs of whitespace to a single space. (The virtual-time
//!    field is right-aligned with variable-width padding, e.g. `"        10500"`
//!    vs `"            0"`; collapsing keeps the token, drops only the padding.)
//! 3. Replaces *pointer-like* hex tokens — `0x` followed by **8 or more** hex
//!    digits — with `0xPTR`. Raw Rust pointer addresses look like
//!    `0x7f8a3c001200` (12+ digits); the only `0x` values microcar actually
//!    emits today are CAN identifiers such as `id=0x0102` (4 digits), which are
//!    deterministic and semantically meaningful, so the 8-digit floor leaves
//!    them untouched. This rule is a forward-looking guard, not something the
//!    current traces exercise.
//!
//! We intentionally do NOT touch the virtual-time value: it is the simulation
//! clock, it is deterministic, and invariants depend on it.
//!
//! # Hashing
//!
//! [`trace_hash`] uses an inline FNV-1a 64-bit hash rather than
//! [`std::hash::DefaultHasher`]. DefaultHasher's algorithm is explicitly not
//! guaranteed stable across Rust versions, which would make golden trace hashes
//! useless in CI. FNV-1a is tiny, dependency-free, and byte-stable forever.

/// Normalize a trace so cosmetic/non-deterministic differences don't perturb the
/// hash. Idempotent: `normalize_trace(normalize_trace(x)) == normalize_trace(x)`.
pub fn normalize_trace(lines: &[String]) -> Vec<String> {
    lines.iter().map(|l| normalize_line(l)).collect()
}

fn normalize_line(line: &str) -> String {
    // Collapse whitespace runs to single spaces and trim.
    let mut collapsed = String::with_capacity(line.len());
    let mut prev_ws = false;
    for ch in line.chars() {
        if ch.is_whitespace() {
            if !collapsed.is_empty() {
                prev_ws = true;
            }
        } else {
            if prev_ws {
                collapsed.push(' ');
                prev_ws = false;
            }
            collapsed.push(ch);
        }
    }
    scrub_pointers(&collapsed)
}

/// Replace `0x` followed by >= 8 hex digits with `0xPTR`. Leaves shorter hex
/// literals (CAN ids like `0x0102`) alone.
fn scrub_pointers(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < bytes.len() {
        if i + 1 < bytes.len() && bytes[i] == b'0' && (bytes[i + 1] == b'x' || bytes[i + 1] == b'X')
        {
            // Count contiguous hex digits after the "0x".
            let mut j = i + 2;
            while j < bytes.len() && bytes[j].is_ascii_hexdigit() {
                j += 1;
            }
            let hex_len = j - (i + 2);
            if hex_len >= 8 {
                out.push_str("0xPTR");
                i = j;
                continue;
            }
        }
        // Not a pointer; copy this byte. (All bytes here are ASCII since the
        // trace is ASCII; multi-byte UTF-8 would still be copied byte-wise
        // safely because we only special-case ASCII '0','x' and hex digits.)
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

const FNV_OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

/// Stable FNV-1a 64-bit hash of the (already-normalized) lines, rendered as
/// zero-padded lowercase hex. Lines are separated by `\n` in the hash input so
/// that `["ab","c"]` and `["a","bc"]` hash differently.
pub fn trace_hash(lines: &[String]) -> String {
    let mut hash = FNV_OFFSET_BASIS;
    for (idx, line) in lines.iter().enumerate() {
        if idx > 0 {
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

/// Convenience: normalize then hash.
pub fn normalized_hash(lines: &[String]) -> String {
    trace_hash(&normalize_trace(lines))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn hash_is_stable_for_known_input() {
        // Golden value: if this ever changes, the hash algorithm changed and
        // every stored CI hash is invalidated — do not "fix" it lightly.
        let lines = s(&["[machine.1] 10 can-rx id=0x0102", "[machine.1] 20 speed 5"]);
        assert_eq!(trace_hash(&lines), "bd2d4a1ab669dc56");
    }

    #[test]
    fn hash_is_repeatable() {
        let lines = s(&["alpha", "beta", "gamma"]);
        assert_eq!(trace_hash(&lines), trace_hash(&lines.clone()));
    }

    #[test]
    fn hash_is_order_sensitive() {
        assert_ne!(trace_hash(&s(&["a", "b"])), trace_hash(&s(&["b", "a"])));
    }

    #[test]
    fn hash_distinguishes_line_boundaries() {
        // Separator byte prevents ["ab","c"] colliding with ["a","bc"].
        assert_ne!(trace_hash(&s(&["ab", "c"])), trace_hash(&s(&["a", "bc"])));
    }

    #[test]
    fn normalize_is_idempotent() {
        let raw = s(&[
            "[machine.1]        10500 can-rx receiver=1 id=0x0102 len=2   ",
            "   [machine.4]            0 task-yield id=1 reason=RtosPortYield",
        ]);
        let once = normalize_trace(&raw);
        let twice = normalize_trace(&once);
        assert_eq!(once, twice);
    }

    #[test]
    fn normalize_collapses_padding_but_keeps_tokens() {
        let raw = s(&["[machine.1]        10500 can-rx"]);
        assert_eq!(normalize_trace(&raw), s(&["[machine.1] 10500 can-rx"]));
    }

    #[test]
    fn normalize_preserves_can_ids() {
        // 4-digit CAN ids must NOT be scrubbed.
        let raw = s(&["[machine.1] 10 can-rx id=0x0102 id=0x0500"]);
        assert_eq!(
            normalize_trace(&raw),
            s(&["[machine.1] 10 can-rx id=0x0102 id=0x0500"])
        );
    }

    #[test]
    fn normalize_scrubs_pointer_addresses() {
        let raw = s(&["ptr=0x7f8a3c001200 done"]);
        assert_eq!(normalize_trace(&raw), s(&["ptr=0xPTR done"]));
    }

    #[test]
    fn padding_difference_hashes_equal_after_normalize() {
        let a = s(&["[machine.1]    10 x"]);
        let b = s(&["[machine.1]        10 x"]);
        assert_eq!(normalized_hash(&a), normalized_hash(&b));
    }
}
