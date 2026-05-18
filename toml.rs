use crate::serde::{
    self as sd, Deserialize, Deserializer, Kind, NumKind, Serialize, Serializer,
};
use std::collections::BTreeMap;
use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    String(String),
    Integer(i64),
    Float(f64),
    Boolean(bool),
    Datetime(String),
    Array(Vec<Value>),
    Table(BTreeMap<String, Value>),
}

#[derive(Debug, Clone)]
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
    let mut parser = Parser::new(s);
    parser.parse_document()
}

pub fn to_string(v: &Value) -> String {
    let mut out = String::new();
    if let Value::Table(t) = v {
        serialize_table(t, "", &mut out);
    }
    out
}

struct Parser<'a> {
    bytes: &'a [u8],
    pos: usize,
    line: usize,
    line_start: usize,
    defined: std::collections::BTreeSet<String>,
    arrays: std::collections::BTreeSet<String>,
    implicit: std::collections::BTreeSet<String>,
}

impl<'a> Parser<'a> {
    fn new(s: &'a str) -> Self {
        Parser {
            bytes: s.as_bytes(),
            pos: 0,
            line: 1,
            line_start: 0,
            defined: Default::default(),
            arrays: Default::default(),
            implicit: Default::default(),
        }
    }

    fn err(&self, msg: &str) -> Error {
        Error {
            line: self.line,
            col: self.pos - self.line_start + 1,
            msg: msg.into(),
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn peek_at(&self, off: usize) -> Option<u8> {
        self.bytes.get(self.pos + off).copied()
    }

    fn bump(&mut self) -> Option<u8> {
        let b = self.peek()?;
        self.pos += 1;
        if b == b'\n' {
            self.line += 1;
            self.line_start = self.pos;
        }
        Some(b)
    }

    fn skip_ws(&mut self) {
        while let Some(b) = self.peek() {
            if b == b' ' || b == b'\t' {
                self.pos += 1;
            } else {
                break;
            }
        }
    }

    fn skip_ws_and_comments(&mut self) {
        loop {
            self.skip_ws();
            match self.peek() {
                Some(b'#') => {
                    while let Some(b) = self.peek() {
                        if b == b'\n' { break; }
                        self.pos += 1;
                    }
                }
                Some(b'\n') => { self.bump(); }
                Some(b'\r') if self.peek_at(1) == Some(b'\n') => {
                    self.pos += 1; self.bump();
                }
                _ => break,
            }
        }
    }

    fn skip_line_end(&mut self) -> Result<(), Error> {
        self.skip_ws();
        if self.peek() == Some(b'#') {
            while let Some(b) = self.peek() {
                if b == b'\n' { break; }
                self.pos += 1;
            }
        }
        match self.peek() {
            None => Ok(()),
            Some(b'\n') => { self.bump(); Ok(()) }
            Some(b'\r') if self.peek_at(1) == Some(b'\n') => {
                self.pos += 1; self.bump(); Ok(())
            }
            _ => Err(self.err("expected newline")),
        }
    }

    fn parse_document(&mut self) -> Result<Value, Error> {
        let mut root = BTreeMap::new();
        let mut current_path: Vec<String> = Vec::new();
        let mut current_is_array = false;

        loop {
            self.skip_ws_and_comments();
            if self.peek().is_none() { break; }
            if self.peek() == Some(b'[') {
                self.handle_header(&mut root, &mut current_path, &mut current_is_array)?;
                continue;
            }
            self.parse_keyval_into(&mut root, &current_path, current_is_array)?;
            self.skip_line_end()?;
        }
        Ok(Value::Table(root))
    }

    fn handle_header(
        &mut self,
        root: &mut BTreeMap<String, Value>,
        current_path: &mut Vec<String>,
        current_is_array: &mut bool,
    ) -> Result<(), Error> {
        self.pos += 1;
        let is_array = self.peek() == Some(b'[');
        if is_array { self.pos += 1; }
        self.skip_ws();
        let keys = self.parse_key_path()?;
        self.skip_ws();
        if self.peek() != Some(b']') {
            return Err(self.err("expected ]"));
        }
        self.pos += 1;
        if is_array {
            if self.peek() != Some(b']') {
                return Err(self.err("expected ]]"));
            }
            self.pos += 1;
        }
        self.skip_line_end()?;
        if is_array {
            self.define_array_header(root, &keys)?;
        } else {
            self.define_table_header(root, &keys)?;
        }
        *current_path = keys;
        *current_is_array = is_array;
        Ok(())
    }

    fn define_table_header(
        &mut self,
        root: &mut BTreeMap<String, Value>,
        keys: &[String],
    ) -> Result<(), Error> {
        let joined = keys.join("\x1f");
        if self.defined.contains(&joined) {
            return Err(self.err("duplicate table"));
        }
        self.defined.insert(joined.clone());
        self.implicit.remove(&joined);
        let mut cur = root;
        for (i, k) in keys.iter().enumerate() {
            let is_last = i == keys.len() - 1;
            let prefix = keys[..=i].join("\x1f");
            if !cur.contains_key(k) {
                cur.insert(k.clone(), Value::Table(BTreeMap::new()));
                if !is_last {
                    self.implicit.insert(prefix.clone());
                }
            }
            let arrays = &self.arrays;
            let entry = cur.get_mut(k).unwrap();
            match entry {
                Value::Table(t) => cur = t,
                Value::Array(a) => {
                    if !arrays.contains(&prefix) {
                        return Err(Error {
                            line: self.line,
                            col: self.pos - self.line_start + 1,
                            msg: "not a table".into(),
                        });
                    }
                    match a.last_mut() {
                        Some(Value::Table(t)) => cur = t,
                        _ => return Err(Error {
                            line: self.line,
                            col: self.pos - self.line_start + 1,
                            msg: "not a table".into(),
                        }),
                    }
                }
                _ => return Err(Error {
                    line: self.line,
                    col: self.pos - self.line_start + 1,
                    msg: "not a table".into(),
                }),
            }
        }
        Ok(())
    }

    fn define_array_header(
        &mut self,
        root: &mut BTreeMap<String, Value>,
        keys: &[String],
    ) -> Result<(), Error> {
        let joined = keys.join("\x1f");
        let line = self.line;
        let col = self.pos - self.line_start + 1;
        let mkerr = |m: &str| Error { line, col, msg: m.into() };
        let mut cur = root;
        for (i, k) in keys.iter().enumerate() {
            let is_last = i == keys.len() - 1;
            let prefix = keys[..=i].join("\x1f");
            if is_last {
                if !cur.contains_key(k) {
                    cur.insert(
                        k.clone(),
                        Value::Array(vec![Value::Table(BTreeMap::new())]),
                    );
                    self.arrays.insert(joined.clone());
                    return Ok(());
                }
                match cur.get_mut(k).unwrap() {
                    Value::Array(a) => {
                        if !self.arrays.contains(&joined) {
                            return Err(mkerr("not an array of tables"));
                        }
                        a.push(Value::Table(BTreeMap::new()));
                    }
                    _ => return Err(mkerr("not an array")),
                }
                return Ok(());
            }
            if !cur.contains_key(k) {
                cur.insert(k.clone(), Value::Table(BTreeMap::new()));
                self.implicit.insert(prefix.clone());
            }
            let arrays = &self.arrays;
            match cur.get_mut(k).unwrap() {
                Value::Table(t) => cur = t,
                Value::Array(a) => {
                    if !arrays.contains(&prefix) {
                        return Err(mkerr("not a table"));
                    }
                    match a.last_mut() {
                        Some(Value::Table(t)) => cur = t,
                        _ => return Err(mkerr("not a table")),
                    }
                }
                _ => return Err(mkerr("not a table")),
            }
        }
        Ok(())
    }

    fn parse_keyval_into(
        &mut self,
        root: &mut BTreeMap<String, Value>,
        current_path: &[String],
        current_is_array: bool,
    ) -> Result<(), Error> {
        let keys = self.parse_key_path()?;
        self.skip_ws();
        if self.peek() != Some(b'=') {
            return Err(self.err("expected ="));
        }
        self.pos += 1;
        self.skip_ws();
        let value = self.parse_value()?;
        let cur = self.navigate_to(root, current_path, current_is_array)?;
        self.insert_dotted(cur, &keys, value, current_path)
    }

    fn navigate_to<'b>(
        &mut self,
        root: &'b mut BTreeMap<String, Value>,
        path: &[String],
        is_array: bool,
    ) -> Result<&'b mut BTreeMap<String, Value>, Error> {
        if path.is_empty() { return Ok(root); }
        let mut cur = root;
        for (i, k) in path.iter().enumerate() {
            let is_last = i == path.len() - 1;
            if is_last && is_array {
                if let Value::Array(a) = cur.get_mut(k).unwrap() {
                    if let Value::Table(t) = a.last_mut().unwrap() {
                        return Ok(t);
                    }
                }
                return Err(self.err("internal: bad array"));
            }
            match cur.get_mut(k).unwrap() {
                Value::Table(t) => cur = t,
                Value::Array(a) => {
                    if let Value::Table(t) = a.last_mut().unwrap() {
                        cur = t;
                    } else {
                        return Err(self.err("internal"));
                    }
                }
                _ => return Err(self.err("internal")),
            }
        }
        Ok(cur)
    }

    fn insert_dotted(
        &mut self,
        table: &mut BTreeMap<String, Value>,
        keys: &[String],
        value: Value,
        prefix: &[String],
    ) -> Result<(), Error> {
        let mut cur = table;
        let mut full = prefix.to_vec();
        for (i, k) in keys.iter().enumerate() {
            full.push(k.clone());
            let is_last = i == keys.len() - 1;
            let joined = full.join("\x1f");
            if is_last {
                if cur.contains_key(k) {
                    return Err(self.err("duplicate key"));
                }
                cur.insert(k.clone(), value);
                self.defined.insert(joined);
                return Ok(());
            }
            if !cur.contains_key(k) {
                cur.insert(k.clone(), Value::Table(BTreeMap::new()));
                self.implicit.insert(joined.clone());
            } else if self.defined.contains(&joined) {
                return Err(self.err("cannot extend defined table"));
            }
            match cur.get_mut(k).unwrap() {
                Value::Table(t) => cur = t,
                _ => return Err(self.err("not a table")),
            }
        }
        Ok(())
    }

    fn parse_key_path(&mut self) -> Result<Vec<String>, Error> {
        let mut keys = vec![self.parse_key()?];
        loop {
            self.skip_ws();
            if self.peek() != Some(b'.') { break; }
            self.pos += 1;
            self.skip_ws();
            keys.push(self.parse_key()?);
        }
        Ok(keys)
    }

    fn parse_key(&mut self) -> Result<String, Error> {
        match self.peek() {
            Some(b'"') => self.parse_basic_string(),
            Some(b'\'') => self.parse_literal_string(),
            Some(b) if is_bare_key_char(b) => {
                let start = self.pos;
                while let Some(b) = self.peek() {
                    if !is_bare_key_char(b) { break; }
                    self.pos += 1;
                }
                Ok(std::str::from_utf8(&self.bytes[start..self.pos]).unwrap().into())
            }
            _ => Err(self.err("expected key")),
        }
    }

    fn parse_value(&mut self) -> Result<Value, Error> {
        match self.peek() {
            Some(b'"') => {
                if self.peek_at(1) == Some(b'"') && self.peek_at(2) == Some(b'"') {
                    return self.parse_multi_basic();
                }
                Ok(Value::String(self.parse_basic_string()?))
            }
            Some(b'\'') => {
                if self.peek_at(1) == Some(b'\'') && self.peek_at(2) == Some(b'\'') {
                    return self.parse_multi_literal();
                }
                Ok(Value::String(self.parse_literal_string()?))
            }
            Some(b'[') => self.parse_array(),
            Some(b'{') => self.parse_inline_table(),
            Some(b't') | Some(b'f') => self.parse_bool(),
            _ => self.parse_number_or_datetime(),
        }
    }

    fn parse_basic_string(&mut self) -> Result<String, Error> {
        self.pos += 1;
        let mut out = String::new();
        loop {
            match self.peek() {
                None => return Err(self.err("unterminated string")),
                Some(b'"') => { self.pos += 1; return Ok(out); }
                Some(b'\n') => return Err(self.err("newline in string")),
                Some(b'\\') => {
                    self.pos += 1;
                    self.parse_escape(&mut out)?;
                }
                Some(_) => self.consume_utf8(&mut out)?,
            }
        }
    }

    fn parse_literal_string(&mut self) -> Result<String, Error> {
        self.pos += 1;
        let start = self.pos;
        loop {
            match self.peek() {
                None => return Err(self.err("unterminated literal")),
                Some(b'\'') => {
                    let s = std::str::from_utf8(&self.bytes[start..self.pos])
                        .map_err(|_| self.err("invalid utf-8"))?
                        .to_string();
                    self.pos += 1;
                    return Ok(s);
                }
                Some(b'\n') => return Err(self.err("newline in literal")),
                Some(_) => self.pos += 1,
            }
        }
    }

    fn parse_multi_basic(&mut self) -> Result<Value, Error> {
        self.pos += 3;
        if self.peek() == Some(b'\n') { self.bump(); }
        else if self.peek() == Some(b'\r') && self.peek_at(1) == Some(b'\n') {
            self.pos += 1; self.bump();
        }
        let mut out = String::new();
        loop {
            if self.peek() == Some(b'"') && self.peek_at(1) == Some(b'"')
                && self.peek_at(2) == Some(b'"')
            {
                self.pos += 3;
                while self.peek() == Some(b'"') && out.ends_with('"') == false {
                    break;
                }
                if self.peek() == Some(b'"') {
                    out.push('"'); self.pos += 1;
                    if self.peek() == Some(b'"') { out.push('"'); self.pos += 1; }
                }
                return Ok(Value::String(out));
            }
            match self.peek() {
                None => return Err(self.err("unterminated multi-line string")),
                Some(b'\\') => {
                    if self.is_line_ending_backslash() {
                        self.pos += 1;
                        self.skip_ws_newlines();
                        continue;
                    }
                    self.pos += 1;
                    self.parse_escape(&mut out)?;
                }
                Some(_) => self.consume_utf8(&mut out)?,
            }
        }
    }

    fn is_line_ending_backslash(&self) -> bool {
        if self.peek() != Some(b'\\') { return false; }
        let mut i = self.pos + 1;
        while let Some(&b) = self.bytes.get(i) {
            if b == b' ' || b == b'\t' { i += 1; continue; }
            return b == b'\n' || (b == b'\r' && self.bytes.get(i + 1) == Some(&b'\n'));
        }
        false
    }

    fn skip_ws_newlines(&mut self) {
        while let Some(b) = self.peek() {
            if b == b' ' || b == b'\t' || b == b'\n' {
                self.bump();
            } else if b == b'\r' && self.peek_at(1) == Some(b'\n') {
                self.pos += 1; self.bump();
            } else {
                break;
            }
        }
    }

    fn parse_multi_literal(&mut self) -> Result<Value, Error> {
        self.pos += 3;
        if self.peek() == Some(b'\n') { self.bump(); }
        else if self.peek() == Some(b'\r') && self.peek_at(1) == Some(b'\n') {
            self.pos += 1; self.bump();
        }
        let start = self.pos;
        loop {
            if self.peek() == Some(b'\'') && self.peek_at(1) == Some(b'\'')
                && self.peek_at(2) == Some(b'\'')
            {
                let end = self.pos;
                self.pos += 3;
                let mut s = std::str::from_utf8(&self.bytes[start..end])
                    .map_err(|_| self.err("invalid utf-8"))?
                    .to_string();
                if self.peek() == Some(b'\'') {
                    s.push('\''); self.pos += 1;
                    if self.peek() == Some(b'\'') { s.push('\''); self.pos += 1; }
                }
                return Ok(Value::String(s));
            }
            if self.peek().is_none() {
                return Err(self.err("unterminated multi-line literal"));
            }
            self.bump();
        }
    }

    fn consume_utf8(&mut self, out: &mut String) -> Result<(), Error> {
        let b = self.peek().unwrap();
        let n = utf8_len(b);
        if self.pos + n > self.bytes.len() {
            return Err(self.err("invalid utf-8"));
        }
        let s = std::str::from_utf8(&self.bytes[self.pos..self.pos + n])
            .map_err(|_| self.err("invalid utf-8"))?;
        out.push_str(s);
        self.pos += n;
        Ok(())
    }

    fn parse_escape(&mut self, out: &mut String) -> Result<(), Error> {
        let b = self.bump().ok_or_else(|| self.err("bad escape"))?;
        match b {
            b'b' => out.push('\u{0008}'),
            b't' => out.push('\t'),
            b'n' => out.push('\n'),
            b'f' => out.push('\u{000c}'),
            b'r' => out.push('\r'),
            b'"' => out.push('"'),
            b'\\' => out.push('\\'),
            b'/' => out.push('/'),
            b'u' => self.parse_unicode(4, out)?,
            b'U' => self.parse_unicode(8, out)?,
            _ => return Err(self.err("invalid escape")),
        }
        Ok(())
    }

    fn parse_unicode(&mut self, n: usize, out: &mut String) -> Result<(), Error> {
        if self.pos + n > self.bytes.len() {
            return Err(self.err("bad unicode escape"));
        }
        let s = std::str::from_utf8(&self.bytes[self.pos..self.pos + n])
            .map_err(|_| self.err("bad unicode"))?;
        let cp = u32::from_str_radix(s, 16).map_err(|_| self.err("bad unicode"))?;
        let ch = char::from_u32(cp).ok_or_else(|| self.err("bad codepoint"))?;
        out.push(ch);
        self.pos += n;
        Ok(())
    }

    fn parse_bool(&mut self) -> Result<Value, Error> {
        if self.starts_with(b"true") {
            self.pos += 4;
            return Ok(Value::Boolean(true));
        }
        if self.starts_with(b"false") {
            self.pos += 5;
            return Ok(Value::Boolean(false));
        }
        Err(self.err("invalid bool"))
    }

    fn starts_with(&self, s: &[u8]) -> bool {
        self.bytes[self.pos..].starts_with(s)
    }

    fn parse_array(&mut self) -> Result<Value, Error> {
        self.pos += 1;
        let mut arr = Vec::new();
        loop {
            self.skip_ws_and_comments();
            if self.peek() == Some(b']') { self.pos += 1; return Ok(Value::Array(arr)); }
            let v = self.parse_value()?;
            arr.push(v);
            self.skip_ws_and_comments();
            match self.peek() {
                Some(b',') => { self.pos += 1; }
                Some(b']') => { self.pos += 1; return Ok(Value::Array(arr)); }
                _ => return Err(self.err("expected , or ]")),
            }
        }
    }

    fn parse_inline_table(&mut self) -> Result<Value, Error> {
        self.pos += 1;
        let mut t = BTreeMap::new();
        self.skip_ws();
        if self.peek() == Some(b'}') {
            self.pos += 1;
            return Ok(Value::Table(t));
        }
        loop {
            self.skip_ws();
            let keys = self.parse_key_path()?;
            self.skip_ws();
            if self.peek() != Some(b'=') {
                return Err(self.err("expected ="));
            }
            self.pos += 1;
            self.skip_ws();
            let v = self.parse_value()?;
            inline_insert(&mut t, &keys, v).map_err(|m| self.err(m))?;
            self.skip_ws();
            match self.peek() {
                Some(b',') => { self.pos += 1; }
                Some(b'}') => { self.pos += 1; return Ok(Value::Table(t)); }
                _ => return Err(self.err("expected , or }")),
            }
        }
    }

    fn parse_number_or_datetime(&mut self) -> Result<Value, Error> {
        let start = self.pos;
        if self.starts_with(b"inf") {
            self.pos += 3;
            return Ok(Value::Float(f64::INFINITY));
        }
        if self.starts_with(b"+inf") {
            self.pos += 4;
            return Ok(Value::Float(f64::INFINITY));
        }
        if self.starts_with(b"-inf") {
            self.pos += 4;
            return Ok(Value::Float(f64::NEG_INFINITY));
        }
        if self.starts_with(b"nan") || self.starts_with(b"+nan") {
            self.pos += if self.bytes[self.pos] == b'+' { 4 } else { 3 };
            return Ok(Value::Float(f64::NAN));
        }
        if self.starts_with(b"-nan") {
            self.pos += 4;
            return Ok(Value::Float(-f64::NAN));
        }

        let end = self.scan_value_token();
        let raw = std::str::from_utf8(&self.bytes[start..end])
            .map_err(|_| self.err("invalid utf-8"))?;
        self.pos = end;

        if is_datetime(raw) {
            return Ok(Value::Datetime(raw.to_string()));
        }
        parse_number(raw).map_err(|m| Error {
            line: self.line, col: start - self.line_start + 1, msg: m.into()
        })
    }

    fn scan_value_token(&self) -> usize {
        let mut i = self.pos;
        while let Some(&b) = self.bytes.get(i) {
            let stop = b == b'\n' || b == b'\r' || b == b',' || b == b']'
                || b == b'}' || b == b'#';
            if stop { break; }
            i += 1;
        }
        while i > self.pos {
            let b = self.bytes[i - 1];
            if b == b' ' || b == b'\t' { i -= 1; } else { break; }
        }
        i
    }
}

fn inline_insert(
    t: &mut BTreeMap<String, Value>,
    keys: &[String],
    value: Value,
) -> Result<(), &'static str> {
    let mut cur = t;
    for (i, k) in keys.iter().enumerate() {
        if i == keys.len() - 1 {
            if cur.contains_key(k) { return Err("duplicate key"); }
            cur.insert(k.clone(), value);
            return Ok(());
        }
        if !cur.contains_key(k) {
            cur.insert(k.clone(), Value::Table(BTreeMap::new()));
        }
        match cur.get_mut(k).unwrap() {
            Value::Table(inner) => cur = inner,
            _ => return Err("not a table"),
        }
    }
    Ok(())
}

fn is_bare_key_char(b: u8) -> bool {
    (b >= b'a' && b <= b'z') || (b >= b'A' && b <= b'Z')
        || (b >= b'0' && b <= b'9') || b == b'_' || b == b'-'
}

fn utf8_len(b: u8) -> usize {
    if b < 0x80 { 1 }
    else if b < 0xc0 { 1 }
    else if b < 0xe0 { 2 }
    else if b < 0xf0 { 3 }
    else { 4 }
}

fn is_datetime(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() >= 10 && b[4] == b'-' && b[7] == b'-'
        && b[0].is_ascii_digit() && b[1].is_ascii_digit()
        && b[2].is_ascii_digit() && b[3].is_ascii_digit()
    {
        return true;
    }
    if b.len() >= 8 && b[2] == b':' && b[5] == b':'
        && b[0].is_ascii_digit() && b[1].is_ascii_digit()
    {
        return true;
    }
    false
}

fn parse_number(raw: &str) -> Result<Value, &'static str> {
    let s = raw.trim();
    if s.is_empty() { return Err("empty number"); }
    let (sign, rest) = match s.as_bytes()[0] {
        b'+' => (1i64, &s[1..]),
        b'-' => (-1i64, &s[1..]),
        _ => (1i64, s),
    };
    if rest.starts_with("0x") {
        let cleaned: String = rest[2..].chars().filter(|c| *c != '_').collect();
        let n = i64::from_str_radix(&cleaned, 16).map_err(|_| "bad hex")?;
        return Ok(Value::Integer(sign * n));
    }
    if rest.starts_with("0o") {
        let cleaned: String = rest[2..].chars().filter(|c| *c != '_').collect();
        let n = i64::from_str_radix(&cleaned, 8).map_err(|_| "bad oct")?;
        return Ok(Value::Integer(sign * n));
    }
    if rest.starts_with("0b") {
        let cleaned: String = rest[2..].chars().filter(|c| *c != '_').collect();
        let n = i64::from_str_radix(&cleaned, 2).map_err(|_| "bad bin")?;
        return Ok(Value::Integer(sign * n));
    }
    let cleaned: String = s.chars().filter(|c| *c != '_').collect();
    let is_float = cleaned.contains('.') || cleaned.contains('e')
        || cleaned.contains('E') || cleaned == "inf" || cleaned == "nan";
    if is_float {
        let f: f64 = cleaned.parse().map_err(|_| "bad float")?;
        if cleaned.contains('.') {
            let parts: Vec<&str> = cleaned.split(|c: char| c == 'e' || c == 'E').collect();
            let main = parts[0];
            let dot = main.find('.').unwrap();
            if dot == 0 || dot == main.len() - 1 {
                return Err("bad float");
            }
            let after = &main[dot + 1..];
            if after.is_empty() || !after.chars().next().unwrap().is_ascii_digit() {
                return Err("bad float");
            }
        }
        return Ok(Value::Float(f));
    }
    let n: i64 = cleaned.parse().map_err(|_| "bad int")?;
    Ok(Value::Integer(n))
}

fn serialize_table(t: &BTreeMap<String, Value>, prefix: &str, out: &mut String) {
    for (k, v) in t {
        if !matches!(v, Value::Table(_) | Value::Array(_)) {
            write_key(k, out);
            out.push_str(" = ");
            write_value(v, out);
            out.push('\n');
        }
    }
    for (k, v) in t {
        if let Value::Array(items) = v {
            if items.iter().all(|x| matches!(x, Value::Table(_))) && !items.is_empty() {
                let path = join_path(prefix, k);
                for item in items {
                    out.push_str("[[");
                    out.push_str(&path);
                    out.push_str("]]\n");
                    if let Value::Table(inner) = item {
                        serialize_table(inner, &path, out);
                    }
                }
            } else {
                write_key(k, out);
                out.push_str(" = ");
                write_value(v, out);
                out.push('\n');
            }
        }
    }
    for (k, v) in t {
        if let Value::Table(inner) = v {
            let path = join_path(prefix, k);
            out.push_str("[");
            out.push_str(&path);
            out.push_str("]\n");
            serialize_table(inner, &path, out);
        }
    }
}

fn join_path(prefix: &str, key: &str) -> String {
    let k = key_repr(key);
    if prefix.is_empty() { k } else { format!("{}.{}", prefix, k) }
}

fn write_key(k: &str, out: &mut String) {
    out.push_str(&key_repr(k));
}

fn key_repr(k: &str) -> String {
    let bare = !k.is_empty() && k.bytes().all(is_bare_key_char);
    if bare { k.to_string() } else {
        let mut s = String::from("\"");
        for c in k.chars() { escape_char(c, &mut s); }
        s.push('"');
        s
    }
}

fn write_value(v: &Value, out: &mut String) {
    match v {
        Value::String(s) => {
            out.push('"');
            for c in s.chars() { escape_char(c, out); }
            out.push('"');
        }
        Value::Integer(n) => out.push_str(&n.to_string()),
        Value::Float(f) => write_float(*f, out),
        Value::Boolean(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Datetime(s) => out.push_str(s),
        Value::Array(a) => {
            out.push('[');
            for (i, item) in a.iter().enumerate() {
                if i > 0 { out.push_str(", "); }
                write_value(item, out);
            }
            out.push(']');
        }
        Value::Table(t) => {
            out.push_str("{ ");
            for (i, (k, v)) in t.iter().enumerate() {
                if i > 0 { out.push_str(", "); }
                write_key(k, out);
                out.push_str(" = ");
                write_value(v, out);
            }
            out.push_str(" }");
        }
    }
}

fn escape_char(c: char, out: &mut String) {
    match c {
        '"' => out.push_str("\\\""),
        '\\' => out.push_str("\\\\"),
        '\n' => out.push_str("\\n"),
        '\r' => out.push_str("\\r"),
        '\t' => out.push_str("\\t"),
        '\u{0008}' => out.push_str("\\b"),
        '\u{000c}' => out.push_str("\\f"),
        c if (c as u32) < 0x20 => {
            out.push_str(&format!("\\u{:04X}", c as u32));
        }
        c => out.push(c),
    }
}

impl Serialize for Value {
    fn serialize<S: Serializer>(&self, s: &mut S) -> Result<(), sd::Error> {
        match self {
            Value::String(v) => s.emit_str(v),
            Value::Integer(n) => s.emit_i64(*n),
            Value::Float(f) => s.emit_f64(*f),
            Value::Boolean(b) => s.emit_bool(*b),
            Value::Datetime(v) => s.emit_str(v),
            Value::Array(a) => {
                s.begin_seq()?;
                for x in a { x.serialize(s)?; }
                s.end_seq()
            }
            Value::Table(t) => {
                s.begin_map()?;
                for (k, v) in t { s.key(k)?; v.serialize(s)?; }
                s.end_map()
            }
        }
    }
}

impl<'de> Deserialize<'de> for Value {
    fn deserialize<D: Deserializer<'de>>(d: &mut D) -> Result<Self, sd::Error> {
        match d.peek()? {
            Kind::Null => Err(sd::err("toml has no null")),
            Kind::Bool => Ok(Value::Boolean(d.read_bool()?)),
            Kind::Num => match d.num_kind()? {
                NumKind::F64 => Ok(Value::Float(d.read_f64()?)),
                NumKind::I64 => Ok(Value::Integer(d.read_i64()?)),
                NumKind::U64 => {
                    let u = d.read_u64()?;
                    i64::try_from(u).map(Value::Integer)
                        .map_err(|_| sd::err("i64 overflow"))
                }
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
                let mut t = BTreeMap::new();
                while let Some(k) = d.map_next()? { t.insert(k, Value::deserialize(d)?); }
                Ok(Value::Table(t))
            }
        }
    }
}

fn write_float(f: f64, out: &mut String) {
    if f.is_nan() { out.push_str("nan"); return; }
    if f.is_infinite() {
        out.push_str(if f < 0.0 { "-inf" } else { "inf" });
        return;
    }
    let s = format!("{}", f);
    if !s.contains('.') && !s.contains('e') && !s.contains('E') && !s.contains("inf") {
        out.push_str(&s);
        out.push_str(".0");
    } else {
        out.push_str(&s);
    }
}
