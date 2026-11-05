//! JSON output tuned for readable git diffs: small objects and arrays stay
//! on one line, numbers are rounded to 1 nm.

use serde_json::Value;

const INLINE_WIDTH: usize = 76;

/// Round to 6 decimals (1 nm when the unit is mm).
pub fn round6(v: f64) -> f64 {
    let r = (v * 1e6).round() / 1e6;
    if r == 0.0 { 0.0 } else { r }
}

fn number(n: &serde_json::Number) -> String {
    if n.is_f64() {
        let v = round6(n.as_f64().unwrap_or(0.0));
        if v.fract() == 0.0 && v.abs() < 1e15 {
            format!("{}", v as i64)
        } else {
            let mut s = format!("{v}");
            if s.contains('e') {
                s = format!("{v:.6}");
                while s.ends_with('0') {
                    s.pop();
                }
            }
            s
        }
    } else {
        n.to_string()
    }
}

fn compact(v: &Value, out: &mut String) {
    match v {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => out.push_str(&number(n)),
        Value::String(s) => out.push_str(&serde_json::to_string(s).unwrap()),
        Value::Array(a) => {
            out.push('[');
            for (i, x) in a.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                compact(x, out);
            }
            out.push(']');
        }
        Value::Object(m) => {
            if m.is_empty() {
                out.push_str("{}");
                return;
            }
            out.push_str("{ ");
            for (i, (k, x)) in m.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                out.push_str(&serde_json::to_string(k).unwrap());
                out.push_str(": ");
                compact(x, out);
            }
            out.push_str(" }");
        }
    }
}

fn depth(v: &Value) -> usize {
    match v {
        Value::Array(a) => 1 + a.iter().map(depth).max().unwrap_or(0),
        Value::Object(m) => 1 + m.values().map(depth).max().unwrap_or(0),
        _ => 0,
    }
}

fn pretty(v: &Value, indent: usize, out: &mut String) {
    let mut one = String::new();
    compact(v, &mut one);
    // Keep short values inline, but never collapse deep structures.
    if one.len() + indent <= INLINE_WIDTH && depth(v) <= 2 {
        out.push_str(&one);
        return;
    }
    let pad = "  ".repeat(indent + 1);
    match v {
        Value::Array(a) => {
            out.push_str("[\n");
            for (i, x) in a.iter().enumerate() {
                out.push_str(&pad);
                pretty(x, indent + 1, out);
                if i + 1 < a.len() {
                    out.push(',');
                }
                out.push('\n');
            }
            out.push_str(&"  ".repeat(indent));
            out.push(']');
        }
        Value::Object(m) => {
            out.push_str("{\n");
            let n = m.len();
            for (i, (k, x)) in m.iter().enumerate() {
                out.push_str(&pad);
                out.push_str(&serde_json::to_string(k).unwrap());
                out.push_str(": ");
                pretty(x, indent + 1, out);
                if i + 1 < n {
                    out.push(',');
                }
                out.push('\n');
            }
            out.push_str(&"  ".repeat(indent));
            out.push('}');
        }
        _ => out.push_str(&one),
    }
}

/// Serialize to the project's JSON style, with a trailing newline.
pub fn to_string<T: serde::Serialize>(v: &T) -> serde_json::Result<String> {
    let value = serde_json::to_value(v)?;
    let mut out = String::new();
    pretty(&value, 0, &mut out);
    out.push('\n');
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn rounds_and_inlines() {
        let v = json!({"schema": 1, "p": {"x": 0.1 + 0.2, "y": 2.0}});
        let s = to_string(&v).unwrap();
        assert_eq!(s, "{ \"schema\": 1, \"p\": { \"x\": 0.3, \"y\": 2 } }\n");
    }

    #[test]
    fn long_arrays_break() {
        let pts: Vec<_> = (0..20).map(|i| json!({"x": i, "y": i})).collect();
        let s = to_string(&json!({"points": pts})).unwrap();
        assert!(s.lines().count() > 20);
        assert!(s.contains("    { \"x\": 3, \"y\": 3 },"));
        let back: Value = serde_json::from_str(&s).unwrap();
        assert_eq!(back["points"][19]["x"], 19);
    }

    #[test]
    fn small_floats() {
        assert_eq!(number(&serde_json::Number::from_f64(1e-7).unwrap()), "0");
        assert_eq!(
            number(&serde_json::Number::from_f64(3.2e-6).unwrap()),
            "0.000003"
        );
        assert_eq!(number(&serde_json::Number::from_f64(12.5).unwrap()), "12.5");
        assert_eq!(
            number(&serde_json::Number::from_f64(-0.0000004).unwrap()),
            "0"
        );
    }
}
