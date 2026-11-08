// json.rs — a compact JSON value, parser, and serializer.
// Shared by the runtime server and its demo client. Numbers are
// f64; object key order is preserved (insertion order), which the
// registry seal and the trace both rely on.

use std::fmt::Write as _;

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Value>),
    Obj(Vec<(String, Value)>),
}

impl Value {
    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Obj(pairs) => pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }
    pub fn as_str(&self) -> Option<&str> {
        match self { Value::Str(s) => Some(s), _ => None }
    }
    pub fn as_f64(&self) -> Option<f64> {
        match self { Value::Num(n) => Some(*n), _ => None }
    }
    pub fn as_arr(&self) -> Option<&Vec<Value>> {
        match self { Value::Arr(a) => Some(a), _ => None }
    }
    pub fn as_obj(&self) -> Option<&Vec<(String, Value)>> {
        match self { Value::Obj(o) => Some(o), _ => None }
    }
    pub fn is_null(&self) -> bool { matches!(self, Value::Null) }
    pub fn type_name(&self) -> &'static str {
        match self {
            Value::Null => "null",
            Value::Bool(_) => "boolean",
            Value::Num(_) => "number",
            Value::Str(_) => "string",
            Value::Arr(_) => "array",
            Value::Obj(_) => "object",
        }
    }
    pub fn to_json(&self) -> String {
        let mut out = String::new();
        self.write(&mut out);
        out
    }
    fn write(&self, out: &mut String) {
        match self {
            Value::Null => out.push_str("null"),
            Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            Value::Num(n) => {
                if n.is_finite() && *n == n.trunc() && n.abs() < 9.007_199_254_740_992e15 {
                    let _ = write!(out, "{}", *n as i64);
                } else {
                    let _ = write!(out, "{}", n);
                }
            }
            Value::Str(s) => write_json_string(out, s),
            Value::Arr(items) => {
                out.push('[');
                for (i, v) in items.iter().enumerate() {
                    if i > 0 { out.push(','); }
                    v.write(out);
                }
                out.push(']');
            }
            Value::Obj(pairs) => {
                out.push('{');
                for (i, (k, v)) in pairs.iter().enumerate() {
                    if i > 0 { out.push(','); }
                    write_json_string(out, k);
                    out.push(':');
                    v.write(out);
                }
                out.push('}');
            }
        }
    }
}

pub fn write_json_string(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => { let _ = write!(out, "\\u{:04x}", c as u32); }
            c => out.push(c),
        }
    }
    out.push('"');
}

pub fn parse(text: &str) -> Result<Value, String> {
    let bytes = text.as_bytes();
    let mut pos = 0usize;
    let v = parse_value(bytes, &mut pos)?;
    skip_ws(bytes, &mut pos);
    if pos != bytes.len() { return Err(format!("trailing input at byte {}", pos)); }
    Ok(v)
}

fn skip_ws(b: &[u8], pos: &mut usize) {
    while *pos < b.len() && matches!(b[*pos], b' ' | b'\t' | b'\n' | b'\r') { *pos += 1; }
}

fn parse_value(b: &[u8], pos: &mut usize) -> Result<Value, String> {
    skip_ws(b, pos);
    if *pos >= b.len() { return Err("unexpected end of input".into()); }
    match b[*pos] {
        b'{' => {
            *pos += 1;
            let mut pairs = Vec::new();
            skip_ws(b, pos);
            if *pos < b.len() && b[*pos] == b'}' { *pos += 1; return Ok(Value::Obj(pairs)); }
            loop {
                skip_ws(b, pos);
                let key = match parse_value(b, pos)? {
                    Value::Str(s) => s,
                    _ => return Err("object key must be a string".into()),
                };
                skip_ws(b, pos);
                if *pos >= b.len() || b[*pos] != b':' { return Err("expected ':'".into()); }
                *pos += 1;
                let val = parse_value(b, pos)?;
                pairs.push((key, val));
                skip_ws(b, pos);
                match b.get(*pos) {
                    Some(b',') => { *pos += 1; }
                    Some(b'}') => { *pos += 1; return Ok(Value::Obj(pairs)); }
                    _ => return Err("expected ',' or '}'".into()),
                }
            }
        }
        b'[' => {
            *pos += 1;
            let mut items = Vec::new();
            skip_ws(b, pos);
            if *pos < b.len() && b[*pos] == b']' { *pos += 1; return Ok(Value::Arr(items)); }
            loop {
                items.push(parse_value(b, pos)?);
                skip_ws(b, pos);
                match b.get(*pos) {
                    Some(b',') => { *pos += 1; }
                    Some(b']') => { *pos += 1; return Ok(Value::Arr(items)); }
                    _ => return Err("expected ',' or ']'".into()),
                }
            }
        }
        b'"' => Ok(Value::Str(parse_string(b, pos)?)),
        b't' => { expect(b, pos, "true")?; Ok(Value::Bool(true)) }
        b'f' => { expect(b, pos, "false")?; Ok(Value::Bool(false)) }
        b'n' => { expect(b, pos, "null")?; Ok(Value::Null) }
        _ => parse_number(b, pos),
    }
}

fn expect(b: &[u8], pos: &mut usize, lit: &str) -> Result<(), String> {
    if b[*pos..].starts_with(lit.as_bytes()) {
        *pos += lit.len();
        Ok(())
    } else {
        Err(format!("expected '{}'", lit))
    }
}

fn parse_string(b: &[u8], pos: &mut usize) -> Result<String, String> {
    // b[*pos] == '"'
    *pos += 1;
    let mut out = String::new();
    while *pos < b.len() {
        let c = b[*pos];
        *pos += 1;
        match c {
            b'"' => return Ok(out),
            b'\\' => {
                if *pos >= b.len() { break; }
                let e = b[*pos];
                *pos += 1;
                match e {
                    b'n' => out.push('\n'),
                    b't' => out.push('\t'),
                    b'r' => out.push('\r'),
                    b'b' => out.push('\u{0008}'),
                    b'f' => out.push('\u{000C}'),
                    b'"' => out.push('"'),
                    b'\\' => out.push('\\'),
                    b'/' => out.push('/'),
                    b'u' => {
                        let hex = |b: &[u8], pos: &mut usize| -> Result<u32, String> {
                            if *pos + 4 > b.len() { return Err("bad \\u escape".into()); }
                            let s = std::str::from_utf8(&b[*pos..*pos + 4]).map_err(|_| "bad \\u")?;
                            *pos += 4;
                            u32::from_str_radix(s, 16).map_err(|_| "bad \\u".to_string())
                        };
                        let mut cp = hex(b, pos)?;
                        if (0xD800..0xDC00).contains(&cp) {
                            // high surrogate: expect a low one
                            if b[*pos] == b'\\' && b.get(*pos + 1) == Some(&b'u') {
                                *pos += 2;
                                let lo = hex(b, pos)?;
                                cp = 0x10000 + ((cp - 0xD800) << 10) + (lo - 0xDC00);
                            }
                        }
                        out.push(char::from_u32(cp).unwrap_or('\u{FFFD}'));
                    }
                    _ => return Err("bad escape".into()),
                }
            }
            _ => {
                // copy the full UTF-8 sequence
                let start = *pos - 1;
                let len = utf8_len(b[start]);
                if len > 1 {
                    out.push_str(std::str::from_utf8(&b[start..start + len]).map_err(|_| "bad utf8")?);
                    *pos = start + len;
                } else {
                    out.push(c as char);
                }
            }
        }
    }
    Err("unterminated string".into())
}

fn utf8_len(first: u8) -> usize {
    if first < 0x80 { 1 } else if first >> 5 == 0b110 { 2 } else if first >> 4 == 0b1110 { 3 } else { 4 }
}

fn parse_number(b: &[u8], pos: &mut usize) -> Result<Value, String> {
    let start = *pos;
    while *pos < b.len() && matches!(b[*pos], b'0'..=b'9' | b'-' | b'+' | b'.' | b'e' | b'E') { *pos += 1; }
    if start == *pos { return Err(format!("unexpected character at byte {}", start)); }
    let s = std::str::from_utf8(&b[start..*pos]).map_err(|_| "bad number")?;
    s.parse::<f64>().map(Value::Num).map_err(|_| format!("bad number '{}'", s))
}

pub fn obj(pairs: Vec<(&str, Value)>) -> Value {
    Value::Obj(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}
pub fn s(v: &str) -> Value { Value::Str(v.to_string()) }
pub fn n(v: f64) -> Value { Value::Num(v) }
