//! Minimal, high-assurance, zero-dependency JSON parser and serializer.

use std::fmt;

#[derive(Clone, PartialEq, Debug)]
pub enum JsonValue {
    Null,
    Bool(bool),
    Number(f64),
    Integer(i64),
    String(String),
    Array(Vec<JsonValue>),
    Object(Vec<(String, JsonValue)>),
}

impl JsonValue {
    pub fn as_str(&self) -> Option<&str> {
        match self {
            JsonValue::String(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            JsonValue::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            JsonValue::Integer(i) => Some(*i),
            JsonValue::Number(n) => Some(*n as i64),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            JsonValue::Number(n) => Some(*n),
            JsonValue::Integer(i) => Some(*i as f64),
            _ => None,
        }
    }

    pub fn as_u64(&self) -> Option<u64> {
        self.as_i64()
            .and_then(|i| if i >= 0 { Some(i as u64) } else { None })
    }

    pub fn as_array(&self) -> Option<&[JsonValue]> {
        match self {
            JsonValue::Array(arr) => Some(arr),
            _ => None,
        }
    }

    pub fn get(&self, key: &str) -> Option<&JsonValue> {
        match self {
            JsonValue::Object(entries) => {
                for (k, v) in entries {
                    if k == key {
                        return Some(v);
                    }
                }
                None
            }
            _ => None,
        }
    }

    pub fn serialize(&self) -> String {
        let mut out = String::new();
        self.format_to(&mut out);
        out
    }

    fn format_to(&self, out: &mut String) {
        match self {
            JsonValue::Null => out.push_str("null"),
            JsonValue::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            JsonValue::Integer(i) => out.push_str(&i.to_string()),
            JsonValue::Number(n) => {
                if n.is_finite() {
                    out.push_str(&n.to_string());
                } else {
                    out.push_str("null");
                }
            }
            JsonValue::String(s) => {
                out.push('"');
                for c in s.chars() {
                    match c {
                        '"' => out.push_str("\\\""),
                        '\\' => out.push_str("\\\\"),
                        '\n' => out.push_str("\\n"),
                        '\r' => out.push_str("\\r"),
                        '\t' => out.push_str("\\t"),
                        _ => out.push(c),
                    }
                }
                out.push('"');
            }
            JsonValue::Array(items) => {
                out.push('[');
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    item.format_to(out);
                }
                out.push(']');
            }
            JsonValue::Object(entries) => {
                out.push('{');
                for (i, (k, v)) in entries.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    out.push('"');
                    out.push_str(k);
                    out.push_str("\":");
                    v.format_to(out);
                }
                out.push('}');
            }
        }
    }

    pub fn parse(s: &str) -> Result<JsonValue, String> {
        let chars: Vec<char> = s.chars().collect();
        let mut idx = 0;
        skip_whitespace(&chars, &mut idx);
        let val = parse_value(&chars, &mut idx)?;
        skip_whitespace(&chars, &mut idx);
        if idx < chars.len() {
            return Err(format!(
                "Trailing characters after JSON payload at position {}",
                idx
            ));
        }
        Ok(val)
    }
}

impl fmt::Display for JsonValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.serialize())
    }
}

fn skip_whitespace(chars: &[char], idx: &mut usize) {
    while *idx < chars.len() && chars[*idx].is_whitespace() {
        *idx += 1;
    }
}

fn parse_value(chars: &[char], idx: &mut usize) -> Result<JsonValue, String> {
    skip_whitespace(chars, idx);
    if *idx >= chars.len() {
        return Err("Unexpected end of JSON input".into());
    }

    match chars[*idx] {
        'n' => parse_null(chars, idx),
        't' | 'f' => parse_bool(chars, idx),
        '"' => parse_string(chars, idx).map(JsonValue::String),
        '[' => parse_array(chars, idx),
        '{' => parse_object(chars, idx),
        '-' | '0'..='9' => parse_number(chars, idx),
        c => Err(format!("Unexpected character '{}' at position {}", c, idx)),
    }
}

fn parse_null(chars: &[char], idx: &mut usize) -> Result<JsonValue, String> {
    if *idx + 4 <= chars.len() && &chars[*idx..*idx + 4] == &['n', 'u', 'l', 'l'] {
        *idx += 4;
        Ok(JsonValue::Null)
    } else {
        Err(format!("Invalid token starting at position {}", idx))
    }
}

fn parse_bool(chars: &[char], idx: &mut usize) -> Result<JsonValue, String> {
    if *idx + 4 <= chars.len() && &chars[*idx..*idx + 4] == &['t', 'r', 'u', 'e'] {
        *idx += 4;
        Ok(JsonValue::Bool(true))
    } else if *idx + 5 <= chars.len() && &chars[*idx..*idx + 5] == &['f', 'a', 'l', 's', 'e'] {
        *idx += 5;
        Ok(JsonValue::Bool(false))
    } else {
        Err(format!("Invalid boolean token at position {}", idx))
    }
}

fn parse_string(chars: &[char], idx: &mut usize) -> Result<String, String> {
    *idx += 1; // skip opening quote
    let mut out = String::new();

    while *idx < chars.len() {
        let c = chars[*idx];
        *idx += 1;
        if c == '"' {
            return Ok(out);
        } else if c == '\\' {
            if *idx >= chars.len() {
                return Err("Unterminated escape sequence".into());
            }
            let esc = chars[*idx];
            *idx += 1;
            match esc {
                '"' => out.push('"'),
                '\\' => out.push('\\'),
                '/' => out.push('/'),
                'b' => out.push('\x08'),
                'f' => out.push('\x0c'),
                'n' => out.push('\n'),
                'r' => out.push('\r'),
                't' => out.push('\t'),
                'u' => {
                    if *idx + 4 > chars.len() {
                        return Err("Invalid unicode escape".into());
                    }
                    let hex_str: String = chars[*idx..*idx + 4].iter().collect();
                    *idx += 4;
                    let code = u32::from_str_radix(&hex_str, 16)
                        .map_err(|e| format!("Invalid unicode escape: {}", e))?;
                    if let Some(ch) = char::from_u32(code) {
                        out.push(ch);
                    } else {
                        out.push('?');
                    }
                }
                _ => out.push(esc),
            }
        } else {
            out.push(c);
        }
    }

    Err("Unterminated string literal".into())
}

fn parse_number(chars: &[char], idx: &mut usize) -> Result<JsonValue, String> {
    let start = *idx;
    let mut is_float = false;

    if chars[*idx] == '-' {
        *idx += 1;
    }

    while *idx < chars.len() {
        let c = chars[*idx];
        if c.is_ascii_digit() {
            *idx += 1;
        } else if c == '.' || c == 'e' || c == 'E' || c == '+' || c == '-' {
            is_float = true;
            *idx += 1;
        } else {
            break;
        }
    }

    let num_str: String = chars[start..*idx].iter().collect();
    if is_float {
        let val = num_str
            .parse::<f64>()
            .map_err(|e| format!("Invalid float '{}': {}", num_str, e))?;
        Ok(JsonValue::Number(val))
    } else {
        let val = num_str
            .parse::<i64>()
            .map_err(|e| format!("Invalid integer '{}': {}", num_str, e))?;
        Ok(JsonValue::Integer(val))
    }
}

fn parse_array(chars: &[char], idx: &mut usize) -> Result<JsonValue, String> {
    *idx += 1; // skip '['
    skip_whitespace(chars, idx);

    let mut items = Vec::new();
    if *idx < chars.len() && chars[*idx] == ']' {
        *idx += 1;
        return Ok(JsonValue::Array(items));
    }

    loop {
        let val = parse_value(chars, idx)?;
        items.push(val);

        skip_whitespace(chars, idx);
        if *idx >= chars.len() {
            return Err("Unterminated array".into());
        }

        if chars[*idx] == ']' {
            *idx += 1;
            break;
        } else if chars[*idx] == ',' {
            *idx += 1;
            skip_whitespace(chars, idx);
        } else {
            return Err(format!("Expected ',' or ']' at position {}", idx));
        }
    }

    Ok(JsonValue::Array(items))
}

fn parse_object(chars: &[char], idx: &mut usize) -> Result<JsonValue, String> {
    *idx += 1; // skip '{'
    skip_whitespace(chars, idx);

    let mut entries = Vec::new();
    if *idx < chars.len() && chars[*idx] == '}' {
        *idx += 1;
        return Ok(JsonValue::Object(entries));
    }

    loop {
        skip_whitespace(chars, idx);
        if *idx >= chars.len() || chars[*idx] != '"' {
            return Err(format!("Expected string key in object at position {}", idx));
        }

        let key = parse_string(chars, idx)?;
        skip_whitespace(chars, idx);

        if *idx >= chars.len() || chars[*idx] != ':' {
            return Err(format!("Expected ':' after object key at position {}", idx));
        }
        *idx += 1; // skip ':'

        let val = parse_value(chars, idx)?;
        entries.push((key, val));

        skip_whitespace(chars, idx);
        if *idx >= chars.len() {
            return Err("Unterminated object".into());
        }

        if chars[*idx] == '}' {
            *idx += 1;
            break;
        } else if chars[*idx] == ',' {
            *idx += 1;
            skip_whitespace(chars, idx);
        } else {
            return Err(format!("Expected ',' or '}}' at position {}", idx));
        }
    }

    Ok(JsonValue::Object(entries))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_json_parse_and_serialize_roundtrip() {
        let json = r#"{"jsonrpc":"2.0","id":1,"method":"eth_blockNumber","params":["latest",true,123,null]}"#;
        let parsed = JsonValue::parse(json).unwrap();

        assert_eq!(parsed.get("jsonrpc").unwrap().as_str(), Some("2.0"));
        assert_eq!(parsed.get("id").unwrap().as_i64(), Some(1));
        assert_eq!(
            parsed.get("method").unwrap().as_str(),
            Some("eth_blockNumber")
        );

        let params = parsed.get("params").unwrap().as_array().unwrap();
        assert_eq!(params.len(), 4);
        assert_eq!(params[0].as_str(), Some("latest"));
        assert_eq!(params[1].as_bool(), Some(true));
        assert_eq!(params[2].as_i64(), Some(123));
        assert_eq!(params[3], JsonValue::Null);

        let serialized = parsed.serialize();
        let reparsed = JsonValue::parse(&serialized).unwrap();
        assert_eq!(parsed, reparsed);
    }
}
