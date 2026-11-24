//! Minimal JSON value with a pretty printer, so the tool stays dependency-free.

pub enum Json {
    Null,
    Bool(bool),
    Number(String),
    String(String),
    Array(Vec<Json>),
    Object(Vec<(&'static str, Json)>),
}

impl Json {
    pub fn str(value: impl Into<String>) -> Json {
        Json::String(value.into())
    }

    pub fn opt_str(value: Option<impl Into<String>>) -> Json {
        match value {
            Some(v) => Json::String(v.into()),
            None => Json::Null,
        }
    }

    pub fn num<T: ToString>(value: T) -> Json {
        Json::Number(value.to_string())
    }

    pub fn opt_num<T: ToString>(value: Option<T>) -> Json {
        match value {
            Some(v) => Json::Number(v.to_string()),
            None => Json::Null,
        }
    }

    pub fn array(items: impl IntoIterator<Item = Json>) -> Json {
        Json::Array(items.into_iter().collect())
    }

    pub fn object(fields: impl IntoIterator<Item = (&'static str, Json)>) -> Json {
        Json::Object(fields.into_iter().collect())
    }

    pub fn pretty(&self) -> String {
        let mut out = String::new();
        self.write(&mut out, 0);
        out
    }

    fn write(&self, out: &mut String, indent: usize) {
        match self {
            Json::Null => out.push_str("null"),
            Json::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Json::Number(n) => out.push_str(n),
            Json::String(s) => write_escaped(out, s),
            Json::Array(items) => {
                if items.is_empty() {
                    out.push_str("[]");
                    return;
                }
                out.push_str("[\n");
                for (i, item) in items.iter().enumerate() {
                    push_indent(out, indent + 1);
                    item.write(out, indent + 1);
                    if i + 1 != items.len() {
                        out.push(',');
                    }
                    out.push('\n');
                }
                push_indent(out, indent);
                out.push(']');
            }
            Json::Object(fields) => {
                if fields.is_empty() {
                    out.push_str("{}");
                    return;
                }
                out.push_str("{\n");
                for (i, (key, value)) in fields.iter().enumerate() {
                    push_indent(out, indent + 1);
                    write_escaped(out, key);
                    out.push_str(": ");
                    value.write(out, indent + 1);
                    if i + 1 != fields.len() {
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

fn write_escaped(out: &mut String, value: &str) {
    out.push('"');
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_strings() {
        assert_eq!(Json::str("a\"b\\c\nd").pretty(), "\"a\\\"b\\\\c\\nd\"");
    }

    #[test]
    fn renders_nested_object() {
        let json = Json::object([
            ("name", Json::str("md0")),
            ("size", Json::num(123)),
            ("ok", Json::Bool(true)),
            ("missing", Json::Null),
        ]);
        let text = json.pretty();
        assert!(text.contains("\"name\": \"md0\""));
        assert!(text.contains("\"size\": 123"));
        assert!(text.contains("\"ok\": true"));
        assert!(text.contains("\"missing\": null"));
    }
}
