use crate::serde::{
    self as sd, Deserialize, Deserializer, Kind, NumKind, Serialize, Serializer,
};
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

fn to_se(e: Error) -> sd::Error { sd::err(e.to_string()) }

pub struct JsonSer {
    pub out: String,
    stack: Vec<(u8, bool)>,
}

impl JsonSer {
    pub fn new() -> Self { JsonSer { out: String::new(), stack: Vec::new() } }
    pub fn with_capacity(c: usize) -> Self {
        JsonSer { out: String::with_capacity(c), stack: Vec::new() }
    }
    fn before_value(&mut self) {
        if let Some(top) = self.stack.last_mut() {
            if top.0 == 0 {
                if top.1 { self.out.push(','); } else { top.1 = true; }
            }
        }
    }
}

impl Default for JsonSer { fn default() -> Self { Self::new() } }

impl Serializer for JsonSer {
    fn emit_null(&mut self) -> Result<(), sd::Error> {
        self.before_value(); self.out.push_str("null"); Ok(())
    }
    fn emit_bool(&mut self, v: bool) -> Result<(), sd::Error> {
        self.before_value();
        self.out.push_str(if v { "true" } else { "false" });
        Ok(())
    }
    fn emit_i64(&mut self, v: i64) -> Result<(), sd::Error> {
        self.before_value(); write_i64(&mut self.out, v); Ok(())
    }
    fn emit_u64(&mut self, v: u64) -> Result<(), sd::Error> {
        self.before_value(); write_u64(&mut self.out, v); Ok(())
    }
    fn emit_f64(&mut self, v: f64) -> Result<(), sd::Error> {
        self.before_value(); write_float(&mut self.out, v); Ok(())
    }
    fn emit_str(&mut self, v: &str) -> Result<(), sd::Error> {
        self.before_value(); write_string(&mut self.out, v); Ok(())
    }
    fn begin_seq(&mut self) -> Result<(), sd::Error> {
        self.before_value();
        self.out.push('[');
        self.stack.push((0, false));
        Ok(())
    }
    fn end_seq(&mut self) -> Result<(), sd::Error> {
        self.out.push(']'); self.stack.pop(); Ok(())
    }
    fn begin_map(&mut self) -> Result<(), sd::Error> {
        self.before_value();
        self.out.push('{');
        self.stack.push((1, false));
        Ok(())
    }
    fn key(&mut self, k: &str) -> Result<(), sd::Error> {
        let top = self.stack.last_mut().ok_or_else(|| sd::err("key outside map"))?;
        if top.1 { self.out.push(','); } else { top.1 = true; }
        write_string(&mut self.out, k);
        self.out.push(':');
        Ok(())
    }
    fn end_map(&mut self) -> Result<(), sd::Error> {
        self.out.push('}'); self.stack.pop(); Ok(())
    }
}

#[derive(Clone, Copy)]
enum Frame { Seq(bool), Map(bool) }

pub struct JsonDe<'a> {
    p: Parser<'a>,
    stack: Vec<Frame>,
}

impl<'a> JsonDe<'a> {
    pub fn new(s: &'a str) -> Self { JsonDe { p: Parser::new(s.as_bytes()), stack: Vec::new() } }
    pub fn finish(&mut self) -> Result<(), sd::Error> {
        self.p.skip_ws();
        if self.p.pos < self.p.bytes.len() {
            return Err(to_se(self.p.error("trailing characters")));
        }
        Ok(())
    }
}

impl<'a> Deserializer for JsonDe<'a> {
    fn peek(&mut self) -> Result<Kind, sd::Error> {
        self.p.skip_ws();
        match self.p.peek() {
            Some(b'n') => Ok(Kind::Null),
            Some(b't') | Some(b'f') => Ok(Kind::Bool),
            Some(b'"') => Ok(Kind::Str),
            Some(b'[') => Ok(Kind::Seq),
            Some(b'{') => Ok(Kind::Map),
            Some(c) if c == b'-' || c.is_ascii_digit() => Ok(Kind::Num),
            Some(_) => Err(to_se(self.p.error("expected value"))),
            None => Err(to_se(self.p.error("unexpected end of input"))),
        }
    }
    fn num_kind(&mut self) -> Result<NumKind, sd::Error> {
        self.p.skip_ws();
        let mut i = self.p.pos;
        let neg = self.p.bytes.get(i) == Some(&b'-');
        if neg { i += 1; }
        while let Some(&b) = self.p.bytes.get(i) {
            if b == b'.' || b == b'e' || b == b'E' { return Ok(NumKind::F64); }
            if !b.is_ascii_digit() { break; }
            i += 1;
        }
        Ok(if neg { NumKind::I64 } else { NumKind::U64 })
    }
    fn read_null(&mut self) -> Result<(), sd::Error> {
        self.p.skip_ws();
        self.p.parse_literal(b"null", Value::Null).map_err(to_se)?;
        Ok(())
    }
    fn read_bool(&mut self) -> Result<bool, sd::Error> {
        self.p.skip_ws();
        match self.p.peek() {
            Some(b't') => { self.p.parse_literal(b"true", Value::Null).map_err(to_se)?; Ok(true) }
            Some(b'f') => { self.p.parse_literal(b"false", Value::Null).map_err(to_se)?; Ok(false) }
            _ => Err(to_se(self.p.error("expected bool"))),
        }
    }
    fn read_i64(&mut self) -> Result<i64, sd::Error> {
        self.p.skip_ws();
        match self.p.parse_number().map_err(to_se)? {
            Value::Number(Number::NegInt(i)) => Ok(i),
            Value::Number(Number::PosInt(u)) => i64::try_from(u).map_err(|_| sd::err("i64 overflow")),
            Value::Number(Number::Float(f)) => Ok(f as i64),
            _ => Err(sd::err("expected number")),
        }
    }
    fn read_u64(&mut self) -> Result<u64, sd::Error> {
        self.p.skip_ws();
        match self.p.parse_number().map_err(to_se)? {
            Value::Number(Number::PosInt(u)) => Ok(u),
            Value::Number(Number::NegInt(i)) if i >= 0 => Ok(i as u64),
            Value::Number(Number::Float(f)) if f >= 0.0 => Ok(f as u64),
            _ => Err(sd::err("u64 out of range")),
        }
    }
    fn read_f64(&mut self) -> Result<f64, sd::Error> {
        self.p.skip_ws();
        match self.p.parse_number().map_err(to_se)? {
            Value::Number(Number::Float(f)) => Ok(f),
            Value::Number(Number::PosInt(u)) => Ok(u as f64),
            Value::Number(Number::NegInt(i)) => Ok(i as f64),
            _ => Err(sd::err("expected number")),
        }
    }
    fn read_str(&mut self) -> Result<String, sd::Error> {
        self.p.skip_ws();
        if self.p.peek() != Some(b'"') {
            return Err(to_se(self.p.error("expected string")));
        }
        self.p.parse_string().map_err(to_se)
    }
    fn begin_seq(&mut self) -> Result<(), sd::Error> {
        self.p.skip_ws();
        if self.p.peek() != Some(b'[') {
            return Err(to_se(self.p.error("expected `[`")));
        }
        self.p.bump();
        self.stack.push(Frame::Seq(true));
        Ok(())
    }
    fn seq_next(&mut self) -> Result<bool, sd::Error> {
        self.p.skip_ws();
        let first = match self.stack.last_mut() {
            Some(Frame::Seq(f)) => f,
            _ => return Err(sd::err("not in seq")),
        };
        if self.p.peek() == Some(b']') {
            self.p.bump();
            self.stack.pop();
            return Ok(false);
        }
        if *first { *first = false; return Ok(true); }
        if self.p.peek() == Some(b',') { self.p.bump(); Ok(true) }
        else { Err(to_se(self.p.error("expected `,` or `]`"))) }
    }
    fn begin_map(&mut self) -> Result<(), sd::Error> {
        self.p.skip_ws();
        if self.p.peek() != Some(b'{') {
            return Err(to_se(self.p.error("expected `{`")));
        }
        self.p.bump();
        self.stack.push(Frame::Map(true));
        Ok(())
    }
    fn map_next(&mut self) -> Result<Option<String>, sd::Error> {
        self.p.skip_ws();
        let first = match self.stack.last_mut() {
            Some(Frame::Map(f)) => f,
            _ => return Err(sd::err("not in map")),
        };
        if self.p.peek() == Some(b'}') {
            self.p.bump();
            self.stack.pop();
            return Ok(None);
        }
        if *first { *first = false; }
        else if self.p.peek() == Some(b',') { self.p.bump(); self.p.skip_ws(); }
        else { return Err(to_se(self.p.error("expected `,` or `}`"))); }
        if self.p.peek() != Some(b'"') {
            return Err(to_se(self.p.error("expected key")));
        }
        let k = self.p.parse_string().map_err(to_se)?;
        self.p.skip_ws();
        if self.p.peek() != Some(b':') {
            return Err(to_se(self.p.error("expected `:`")));
        }
        self.p.bump();
        Ok(Some(k))
    }
    fn skip(&mut self) -> Result<(), sd::Error> {
        self.p.skip_ws();
        let _ = self.p.parse_value().map_err(to_se)?;
        Ok(())
    }
}

impl Serialize for Value {
    fn serialize<S: Serializer>(&self, s: &mut S) -> Result<(), sd::Error> {
        match self {
            Value::Null => s.emit_null(),
            Value::Bool(b) => s.emit_bool(*b),
            Value::Number(Number::PosInt(u)) => s.emit_u64(*u),
            Value::Number(Number::NegInt(i)) => s.emit_i64(*i),
            Value::Number(Number::Float(f)) => s.emit_f64(*f),
            Value::String(v) => s.emit_str(v),
            Value::Array(a) => {
                s.begin_seq()?;
                for x in a { x.serialize(s)?; }
                s.end_seq()
            }
            Value::Object(o) => {
                s.begin_map()?;
                for (k, v) in o { s.key(k)?; v.serialize(s)?; }
                s.end_map()
            }
        }
    }
}

impl Deserialize for Value {
    fn deserialize<D: Deserializer>(d: &mut D) -> Result<Self, sd::Error> {
        match d.peek()? {
            Kind::Null => { d.read_null()?; Ok(Value::Null) }
            Kind::Bool => Ok(Value::Bool(d.read_bool()?)),
            Kind::Num => match d.num_kind()? {
                NumKind::U64 => Ok(Value::Number(Number::PosInt(d.read_u64()?))),
                NumKind::I64 => Ok(Value::Number(Number::NegInt(d.read_i64()?))),
                NumKind::F64 => Ok(Value::Number(Number::Float(d.read_f64()?))),
            },
            Kind::Str => Ok(Value::String(d.read_str()?)),
            Kind::Seq => {
                d.begin_seq()?;
                let mut arr = Vec::new();
                while d.seq_next()? { arr.push(Value::deserialize(d)?); }
                Ok(Value::Array(arr))
            }
            Kind::Map => {
                d.begin_map()?;
                let mut obj = BTreeMap::new();
                while let Some(k) = d.map_next()? { obj.insert(k, Value::deserialize(d)?); }
                Ok(Value::Object(obj))
            }
        }
    }
}

pub fn to_string_se<T: Serialize>(v: &T) -> Result<String, sd::Error> {
    let mut s = JsonSer::with_capacity(64);
    v.serialize(&mut s)?;
    Ok(s.out)
}

pub fn from_str_de<T: Deserialize>(s: &str) -> Result<T, sd::Error> {
    let mut d = JsonDe::new(s);
    let v = T::deserialize(&mut d)?;
    d.finish()?;
    Ok(v)
}
