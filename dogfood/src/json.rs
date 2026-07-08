//! Minimal, dependency-free JSON emitter.
//!
//! serde_json *is* present in microcar's Cargo.lock (transitively), but pulling
//! it in would add serde + proc-macro build deps to this otherwise std-only
//! crate and couple our output format to serde's derive machinery. The summary
//! we emit is small and flat, so a ~100-line hand-rolled writer is the lighter,
//! more self-contained choice. Output is valid, RFC 8259-compliant JSON.

use std::fmt::Write as _;

/// A JSON value. Object key order is preserved (insertion order) for stable,
/// diff-friendly output.
#[derive(Debug, Clone)]
pub enum Json {
    Null,
    Bool(bool),
    /// Integer-valued number.
    Int(i64),
    /// Unsigned integer-valued number (for values that can exceed i64, e.g. ms).
    UInt(u128),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

impl Json {
    pub fn str(s: impl Into<String>) -> Json {
        Json::Str(s.into())
    }

    /// Serialize with 2-space indentation and a trailing newline.
    pub fn to_pretty(&self) -> String {
        let mut out = String::new();
        self.write(&mut out, 0);
        out.push('\n');
        out
    }

    fn write(&self, out: &mut String, indent: usize) {
        match self {
            Json::Null => out.push_str("null"),
            Json::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Json::Int(n) => {
                let _ = write!(out, "{n}");
            }
            Json::UInt(n) => {
                let _ = write!(out, "{n}");
            }
            Json::Str(s) => write_escaped(out, s),
            Json::Arr(items) => {
                if items.is_empty() {
                    out.push_str("[]");
                    return;
                }
                out.push_str("[\n");
                for (i, item) in items.iter().enumerate() {
                    push_indent(out, indent + 1);
                    item.write(out, indent + 1);
                    if i + 1 < items.len() {
                        out.push(',');
                    }
                    out.push('\n');
                }
                push_indent(out, indent);
                out.push(']');
            }
            Json::Obj(fields) => {
                if fields.is_empty() {
                    out.push_str("{}");
                    return;
                }
                out.push_str("{\n");
                for (i, (k, v)) in fields.iter().enumerate() {
                    push_indent(out, indent + 1);
                    write_escaped(out, k);
                    out.push_str(": ");
                    v.write(out, indent + 1);
                    if i + 1 < fields.len() {
                        out.push(',');
                    }
                    out.push('\n');
                }
                push_indent(out, indent);
                out.push('}');
            }
        }
    }
}

fn push_indent(out: &mut String, level: usize) {
    for _ in 0..level {
        out.push_str("  ");
    }
}

fn write_escaped(out: &mut String, s: &str) {
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_special_chars() {
        let mut s = String::new();
        write_escaped(&mut s, "a\"b\\c\nd\te\u{01}");
        assert_eq!(s, r#""a\"b\\c\nd\te\u0001""#);
    }

    #[test]
    fn empty_containers() {
        assert_eq!(Json::Arr(vec![]).to_pretty(), "[]\n");
        assert_eq!(Json::Obj(vec![]).to_pretty(), "{}\n");
    }

    #[test]
    fn nested_object_roundtrips_shape() {
        let j = Json::Obj(vec![
            ("name".into(), Json::str("demo")),
            ("count".into(), Json::Int(2)),
            ("ok".into(), Json::Bool(true)),
            (
                "items".into(),
                Json::Arr(vec![Json::str("a"), Json::str("b")]),
            ),
        ]);
        let out = j.to_pretty();
        assert!(out.contains("\"name\": \"demo\""));
        assert!(out.contains("\"count\": 2"));
        assert!(out.contains("\"ok\": true"));
        assert!(out.contains("\"items\": [\n"));
        // Well-formed: balanced braces/brackets.
        assert_eq!(out.matches('{').count(), out.matches('}').count());
        assert_eq!(out.matches('[').count(), out.matches(']').count());
    }

    #[test]
    fn preserves_key_order() {
        let j = Json::Obj(vec![("z".into(), Json::Int(1)), ("a".into(), Json::Int(2))]);
        let out = j.to_pretty();
        let zpos = out.find("\"z\"").unwrap();
        let apos = out.find("\"a\"").unwrap();
        assert!(zpos < apos);
    }
}
