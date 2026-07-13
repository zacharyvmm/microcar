//! Minimal hand-rolled JSON emitter (pretty-printed).
//!
//! The dogfood harness is intentionally std-only (no `serde`). This module
//! provides just enough JSON to emit the CI summary: objects, arrays, strings,
//! unsigned integers, and booleans, with proper string escaping and 2-space
//! pretty indentation. Object key order is preserved (insertion order).

/// A minimal JSON value.
pub enum Json {
    /// A JSON string (escaped on render).
    Str(String),
    /// A JSON unsigned integer.
    UInt(u128),
    /// A JSON boolean.
    Bool(bool),
    /// A JSON array.
    Arr(Vec<Json>),
    /// A JSON object (insertion-ordered key/value pairs).
    Obj(Vec<(String, Json)>),
}

impl Json {
    /// Convenience constructor for a JSON string from anything string-like
    /// (`&str`, `&String`, `String`).
    pub fn str<S: AsRef<str>>(s: S) -> Json {
        Json::Str(s.as_ref().to_string())
    }

    /// Render as pretty-printed JSON with 2-space indentation.
    pub fn to_pretty(&self) -> String {
        let mut out = String::new();
        self.write_pretty(&mut out, 0);
        out
    }

    fn write_pretty(&self, out: &mut String, indent: usize) {
        match self {
            Json::Str(s) => {
                out.push('"');
                escape_into(s, out);
                out.push('"');
            }
            Json::UInt(n) => out.push_str(&n.to_string()),
            Json::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Json::Arr(items) => {
                if items.is_empty() {
                    out.push_str("[]");
                    return;
                }
                out.push_str("[\n");
                for (i, item) in items.iter().enumerate() {
                    push_indent(out, indent + 1);
                    item.write_pretty(out, indent + 1);
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
                    out.push('"');
                    escape_into(k, out);
                    out.push_str("\": ");
                    v.write_pretty(out, indent + 1);
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

fn push_indent(out: &mut String, indent: usize) {
    for _ in 0..indent {
        out.push_str("  ");
    }
}

fn escape_into(s: &str, out: &mut String) {
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Json;

    #[test]
    fn renders_object_and_escapes() {
        let j = Json::Obj(vec![
            ("name".into(), Json::str("a\"b")),
            ("n".into(), Json::UInt(7)),
            ("ok".into(), Json::Bool(true)),
            (
                "arr".into(),
                Json::Arr(vec![Json::str("x"), Json::str("y")]),
            ),
        ]);
        let s = j.to_pretty();
        assert!(s.contains("\"name\": \"a\\\"b\""));
        assert!(s.contains("\"n\": 7"));
        assert!(s.contains("\"ok\": true"));
        // Balanced delimiters => structurally valid.
        assert_eq!(s.matches('{').count(), s.matches('}').count());
        assert_eq!(s.matches('[').count(), s.matches(']').count());
    }

    #[test]
    fn empty_containers() {
        assert_eq!(Json::Arr(vec![]).to_pretty(), "[]");
        assert_eq!(Json::Obj(vec![]).to_pretty(), "{}");
    }

    #[test]
    fn str_accepts_str_and_string() {
        let owned = String::from("hi");
        assert!(matches!(Json::str(&owned), Json::Str(_)));
        assert!(matches!(Json::str("hi"), Json::Str(_)));
    }
}
