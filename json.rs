use std::collections::BTreeMap;
use std::fmt::{self, Write};

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Number(Number),
    String(String),
    Array(Vec<Value>),
    Object(BTreeMap<String, Value>),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Number {
    PosInt(u64),
    NegInt(i64),
    Float(f64),
}

#[derive(Debug, Clone, PartialEq)]
pub struct Error {
    pub line: usize,
    pub col: usize,
    pub msg: String,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{} at line {} column {}", self.msg, self.line, self.col)
    }
}

impl std::error::Error for Error {}

pub fn from_str(s: &str) -> Result<Value, Error> {
    let mut p = Parser::new(s.as_bytes());
    p.skip_ws();
    let v = p.parse_value()?;
    p.skip_ws();
    if p.pos < p.bytes.len() {
        return Err(p.error("trailing characters"));
    }
    Ok(v)
}

pub fn to_string(v: &Value) -> String {
    let mut out = String::with_capacity(estimate_size(v));
    write_compact(&mut out, v);
    out
}

pub fn to_string_pretty(v: &Value) -> String {
    let mut out = String::with_capacity(estimate_size(v) * 2);
    write_pretty(&mut out, v, 0);
    out
}

fn estimate_size(v: &Value) -> usize {
    match v {
        Value::Null | Value::Bool(_) => 5,
        Value::Number(_) => 16,
        Value::String(s) => s.len() + 2,
        Value::Array(a) => 2 + a.iter().map(|x| estimate_size(x) + 1).sum::<usize>(),
        Value::Object(o) => 2 + o.iter().map(|(k, x)| k.len() + estimate_size(x) + 4).sum::<usize>(),
    }
}

const NULL: Value = Value::Null;

impl Value {
    pub fn is_null(&self) -> bool { matches!(self, Value::Null) }
    pub fn as_bool(&self) -> Option<bool> { if let Value::Bool(b) = self { Some(*b) } else { None } }
    pub fn as_str(&self) -> Option<&str> { if let Value::String(s) = self { Some(s) } else { None } }
    pub fn as_array(&self) -> Option<&Vec<Value>> { if let Value::Array(a) = self { Some(a) } else { None } }
    pub fn as_object(&self) -> Option<&BTreeMap<String, Value>> { if let Value::Object(o) = self { Some(o) } else { None } }
    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Value::Number(Number::NegInt(i)) => Some(*i),
            Value::Number(Number::PosInt(u)) => i64::try_from(*u).ok(),
            _ => None,
        }
    }
    pub fn as_u64(&self) -> Option<u64> {
        match self {
            Value::Number(Number::PosInt(u)) => Some(*u),
            Value::Number(Number::NegInt(i)) if *i >= 0 => Some(*i as u64),
            _ => None,
        }
    }
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Number(Number::Float(f)) => Some(*f),
            Value::Number(Number::NegInt(i)) => Some(*i as f64),
            Value::Number(Number::PosInt(u)) => Some(*u as f64),
            _ => None,
        }
    }
    pub fn get(&self, key: &str) -> Option<&Value> {
        if let Value::Object(o) = self { o.get(key) } else { None }
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let mut s = String::new();
        write_compact(&mut s, self);
        f.write_str(&s)
    }
}

impl std::ops::Index<&str> for Value {
    type Output = Value;
    fn index(&self, key: &str) -> &Value {
        match self {
            Value::Object(o) => o.get(key).unwrap_or(&NULL),
            _ => &NULL,
        }
    }
}

impl std::ops::Index<usize> for Value {
    type Output = Value;
    fn index(&self, i: usize) -> &Value {
        match self {
            Value::Array(a) => a.get(i).unwrap_or(&NULL),
            _ => &NULL,
        }
    }
}

struct Parser<'a> {
    bytes: &'a [u8],
    pos: usize,
}

impl<'a> Parser<'a> {
    fn new(bytes: &'a [u8]) -> Self { Parser { bytes, pos: 0 } }

    fn error(&self, msg: &str) -> Error {
        let mut line = 1usize;
        let mut last_nl = 0usize;
        let cap = self.pos.min(self.bytes.len());
        for i in 0..cap {
            if self.bytes[i] == b'\n' { line += 1; last_nl = i + 1; }
        }
        Error { line, col: self.pos - last_nl + 1, msg: msg.into() }
    }

    #[inline]
    fn peek(&self) -> Option<u8> { self.bytes.get(self.pos).copied() }

    #[inline]
    fn bump(&mut self) -> Option<u8> {
        let c = self.peek()?;
        self.pos += 1;
        Some(c)
    }

    fn skip_ws(&mut self) {
        while let Some(c) = self.peek() {
            if !matches!(c, b' ' | b'\t' | b'\n' | b'\r') { break; }
            self.bump();
        }
    }

    fn expect(&mut self, b: u8, name: &str) -> Result<(), Error> {
        if self.peek() == Some(b) { self.bump(); Ok(()) }
        else { Err(self.error(&format!("expected {}", name))) }
    }

    fn parse_value(&mut self) -> Result<Value, Error> {
        self.skip_ws();
        match self.peek() {
            Some(b'{') => self.parse_object(),
            Some(b'[') => self.parse_array(),
            Some(b'"') => self.parse_string().map(Value::String),
            Some(b't') => self.parse_literal(b"true", Value::Bool(true)),
            Some(b'f') => self.parse_literal(b"false", Value::Bool(false)),
            Some(b'n') => self.parse_literal(b"null", Value::Null),
            Some(c) if c == b'-' || c.is_ascii_digit() => self.parse_number(),
            Some(_) => Err(self.error("expected value")),
            None => Err(self.error("unexpected end of input")),
        }
    }

    fn parse_literal(&mut self, lit: &[u8], val: Value) -> Result<Value, Error> {
        if self.bytes.len() - self.pos < lit.len() || &self.bytes[self.pos..self.pos + lit.len()] != lit {
            return Err(self.error("invalid literal"));
        }
        for _ in 0..lit.len() { self.bump(); }
        Ok(val)
    }

    fn parse_object(&mut self) -> Result<Value, Error> {
        self.bump();
        let mut map = BTreeMap::new();
        self.skip_ws();
        if self.peek() == Some(b'}') { self.bump(); return Ok(Value::Object(map)); }
        loop {
            self.skip_ws();
            if self.peek() != Some(b'"') { return Err(self.error("expected string key")); }
            let key = self.parse_string()?;
            self.skip_ws();
            self.expect(b':', ":")?;
            let val = self.parse_value()?;
            map.insert(key, val);
            self.skip_ws();
            match self.peek() {
                Some(b',') => { self.bump(); }
                Some(b'}') => { self.bump(); return Ok(Value::Object(map)); }
                _ => return Err(self.error("expected `,` or `}`")),
            }
        }
    }

    fn parse_array(&mut self) -> Result<Value, Error> {
        self.bump();
        let mut arr = Vec::new();
        self.skip_ws();
        if self.peek() == Some(b']') { self.bump(); return Ok(Value::Array(arr)); }
        loop {
            arr.push(self.parse_value()?);
            self.skip_ws();
            match self.peek() {
                Some(b',') => { self.bump(); }
                Some(b']') => { self.bump(); return Ok(Value::Array(arr)); }
                _ => return Err(self.error("expected `,` or `]`")),
            }
        }
    }

    fn parse_string(&mut self) -> Result<String, Error> {
        self.pos += 1;
        let start = self.pos;
        while self.pos < self.bytes.len() {
            let b = self.bytes[self.pos];
            if b == b'"' {
                let s = std::str::from_utf8(&self.bytes[start..self.pos])
                    .map_err(|_| self.error("invalid utf-8"))?.to_string();
                self.pos += 1;
                return Ok(s);
            }
            if b == b'\\' || b < 0x20 { break; }
            self.pos += 1;
        }
        if self.pos == self.bytes.len() { return Err(self.error("unterminated string")); }
        if self.bytes[self.pos] < 0x20 { return Err(self.error("control character in string")); }
        let mut out = String::with_capacity(self.bytes.len() - start);
        out.push_str(std::str::from_utf8(&self.bytes[start..self.pos]).map_err(|_| self.error("invalid utf-8"))?);
        loop {
            self.pos += 1;
            let c = self.bytes.get(self.pos).copied().ok_or_else(|| self.error("invalid escape"))?;
            self.pos += 1;
            match c {
                b'"' => out.push('"'),
                b'\\' => out.push('\\'),
                b'/' => out.push('/'),
                b'b' => out.push('\u{0008}'),
                b'f' => out.push('\u{000C}'),
                b'n' => out.push('\n'),
                b'r' => out.push('\r'),
                b't' => out.push('\t'),
                b'u' => out.push(self.parse_unicode()?),
                _ => return Err(self.error("invalid escape")),
            }
            let chunk = self.pos;
            while self.pos < self.bytes.len() {
                let b = self.bytes[self.pos];
                if b == b'"' || b == b'\\' || b < 0x20 { break; }
                self.pos += 1;
            }
            out.push_str(std::str::from_utf8(&self.bytes[chunk..self.pos]).map_err(|_| self.error("invalid utf-8"))?);
            match self.bytes.get(self.pos) {
                None => return Err(self.error("unterminated string")),
                Some(b'"') => { self.pos += 1; return Ok(out); }
                Some(&c) if c < 0x20 => return Err(self.error("control character in string")),
                Some(_) => {}
            }
        }
    }

    fn parse_unicode(&mut self) -> Result<char, Error> {
        let high = self.parse_hex4()?;
        if (0xD800..0xDC00).contains(&high) {
            if self.bump() != Some(b'\\') || self.bump() != Some(b'u') {
                return Err(self.error("expected low surrogate"));
            }
            let low = self.parse_hex4()?;
            if !(0xDC00..0xE000).contains(&low) {
                return Err(self.error("invalid low surrogate"));
            }
            let cp = 0x10000 + ((high - 0xD800) << 10) + (low - 0xDC00);
            char::from_u32(cp).ok_or_else(|| self.error("invalid code point"))
        } else if (0xDC00..0xE000).contains(&high) {
            Err(self.error("unexpected low surrogate"))
        } else {
            char::from_u32(high).ok_or_else(|| self.error("invalid code point"))
        }
    }

    fn parse_hex4(&mut self) -> Result<u32, Error> {
        let mut v = 0u32;
        for _ in 0..4 {
            let c = self.bump().ok_or_else(|| self.error("invalid unicode escape"))?;
            let d = match c {
                b'0'..=b'9' => c - b'0',
                b'a'..=b'f' => c - b'a' + 10,
                b'A'..=b'F' => c - b'A' + 10,
                _ => return Err(self.error("invalid hex digit")),
            };
            v = v * 16 + d as u32;
        }
        Ok(v)
    }

    fn parse_number(&mut self) -> Result<Value, Error> {
        let start = self.pos;
        let neg = self.peek() == Some(b'-');
        if neg { self.bump(); }
        if self.peek() == Some(b'0') {
            self.bump();
        } else if matches!(self.peek(), Some(b'1'..=b'9')) {
            while matches!(self.peek(), Some(b'0'..=b'9')) { self.bump(); }
        } else {
            return Err(self.error("invalid number"));
        }
        let mut is_float = false;
        if self.peek() == Some(b'.') {
            is_float = true;
            self.bump();
            if !matches!(self.peek(), Some(b'0'..=b'9')) {
                return Err(self.error("expected digit after `.`"));
            }
            while matches!(self.peek(), Some(b'0'..=b'9')) { self.bump(); }
        }
        if matches!(self.peek(), Some(b'e') | Some(b'E')) {
            is_float = true;
            self.bump();
            if matches!(self.peek(), Some(b'+') | Some(b'-')) { self.bump(); }
            if !matches!(self.peek(), Some(b'0'..=b'9')) {
                return Err(self.error("expected digit in exponent"));
            }
            while matches!(self.peek(), Some(b'0'..=b'9')) { self.bump(); }
        }
        let text = std::str::from_utf8(&self.bytes[start..self.pos]).unwrap();
        if is_float {
            text.parse::<f64>().map(|f| Value::Number(Number::Float(f)))
                .map_err(|_| self.error("invalid number"))
        } else if neg {
            text.parse::<i64>().map(|i| Value::Number(Number::NegInt(i)))
                .map_err(|_| self.error("number out of range"))
        } else {
            text.parse::<u64>().map(|u| Value::Number(Number::PosInt(u)))
                .map_err(|_| self.error("number out of range"))
        }
    }
}

fn write_compact(out: &mut String, v: &Value) {
    match v {
        Value::Null => out.push_str("null"),
        Value::Bool(true) => out.push_str("true"),
        Value::Bool(false) => out.push_str("false"),
        Value::Number(Number::PosInt(u)) => write_u64(out, *u),
        Value::Number(Number::NegInt(i)) => write_i64(out, *i),
        Value::Number(Number::Float(f)) => write_float(out, *f),
        Value::String(s) => write_string(out, s),
        Value::Array(a) => {
            out.push('[');
            for (i, x) in a.iter().enumerate() {
                if i > 0 { out.push(','); }
                write_compact(out, x);
            }
            out.push(']');
        }
        Value::Object(o) => {
            out.push('{');
            for (i, (k, x)) in o.iter().enumerate() {
                if i > 0 { out.push(','); }
                write_string(out, k);
                out.push(':');
                write_compact(out, x);
            }
            out.push('}');
        }
    }
}

fn write_pretty(out: &mut String, v: &Value, depth: usize) {
    match v {
        Value::Array(a) if !a.is_empty() => {
            out.push('[');
            for (i, x) in a.iter().enumerate() {
                if i > 0 { out.push(','); }
                out.push('\n');
                indent(out, depth + 1);
                write_pretty(out, x, depth + 1);
            }
            out.push('\n');
            indent(out, depth);
            out.push(']');
        }
        Value::Object(o) if !o.is_empty() => {
            out.push('{');
            for (i, (k, x)) in o.iter().enumerate() {
                if i > 0 { out.push(','); }
                out.push('\n');
                indent(out, depth + 1);
                write_string(out, k);
                out.push_str(": ");
                write_pretty(out, x, depth + 1);
            }
            out.push('\n');
            indent(out, depth);
            out.push('}');
        }
        _ => write_compact(out, v),
    }
}

fn indent(out: &mut String, depth: usize) {
    for _ in 0..depth { out.push_str("  "); }
}

fn write_string(out: &mut String, s: &str) {
    out.push('"');
    let bytes = s.as_bytes();
    let mut chunk = 0;
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        let esc: Option<&'static str> = match b {
            b'"' => Some("\\\""),
            b'\\' => Some("\\\\"),
            b'\n' => Some("\\n"),
            b'\r' => Some("\\r"),
            b'\t' => Some("\\t"),
            0x08 => Some("\\b"),
            0x0C => Some("\\f"),
            0..=0x1F => None,
            _ => { i += 1; continue; }
        };
        out.push_str(std::str::from_utf8(&bytes[chunk..i]).unwrap());
        match esc {
            Some(e) => out.push_str(e),
            None => { let _ = write!(out, "\\u{:04x}", b); }
        }
        i += 1;
        chunk = i;
    }
    out.push_str(std::str::from_utf8(&bytes[chunk..]).unwrap());
    out.push('"');
}

fn write_u64(out: &mut String, mut n: u64) {
    let mut buf = [0u8; 20];
    let mut i = 20;
    if n == 0 { out.push('0'); return; }
    while n > 0 {
        i -= 1;
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
    }
    out.push_str(std::str::from_utf8(&buf[i..]).unwrap());
}

fn write_i64(out: &mut String, n: i64) {
    if n < 0 {
        out.push('-');
        write_u64(out, n.unsigned_abs());
    } else {
        write_u64(out, n as u64);
    }
}

fn write_float(out: &mut String, f: f64) {
    if !f.is_finite() { out.push_str("null"); return; }
    if f == 0.0 { out.push_str("0.0"); return; }
    if f.fract() == 0.0 && f.abs() < 1e16 {
        write_i64(out, f as i64);
        out.push_str(".0");
        return;
    }
    let s = format!("{}", f);
    out.push_str(&s);
    if !s.contains('.') && !s.contains('e') && !s.contains('E') {
        out.push_str(".0");
    }
}
