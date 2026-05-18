use std::borrow::Cow;
use std::fmt;

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

pub struct Reader<'a> {
    bytes: &'a [u8],
    pos: usize,
    delim: u8,
    quote: u8,
    line: usize,
    line_start: usize,
}

impl<'a> Reader<'a> {
    pub fn new(s: &'a str) -> Self {
        Reader { bytes: s.as_bytes(), pos: 0, delim: b',', quote: b'"', line: 1, line_start: 0 }
    }

    pub fn delimiter(mut self, d: u8) -> Self { self.delim = d; self }
    pub fn quote(mut self, q: u8) -> Self { self.quote = q; self }

    fn at_end(&self) -> bool { self.pos >= self.bytes.len() }

    fn err(&self, msg: &str) -> Error {
        Error { line: self.line, col: self.pos - self.line_start + 1, msg: msg.into() }
    }

    pub fn read_record(&mut self) -> Option<Result<Vec<Cow<'a, str>>, Error>> {
        while let Some(&b) = self.bytes.get(self.pos) {
            if b == b'\n' {
                self.pos += 1; self.line += 1; self.line_start = self.pos;
            } else if b == b'\r' && self.bytes.get(self.pos + 1) == Some(&b'\n') {
                self.pos += 2; self.line += 1; self.line_start = self.pos;
            } else { break; }
        }
        if self.at_end() { return None; }
        let mut record = Vec::with_capacity(8);
        loop {
            match self.read_field() {
                Ok(field) => record.push(field),
                Err(e) => return Some(Err(e)),
            }
            match self.bytes.get(self.pos).copied() {
                None => return Some(Ok(record)),
                Some(c) if c == self.delim => { self.pos += 1; }
                Some(b'\r') => {
                    self.pos += 1;
                    if self.bytes.get(self.pos) == Some(&b'\n') { self.pos += 1; }
                    self.line += 1;
                    self.line_start = self.pos;
                    return Some(Ok(record));
                }
                Some(b'\n') => {
                    self.pos += 1;
                    self.line += 1;
                    self.line_start = self.pos;
                    return Some(Ok(record));
                }
                Some(_) => return Some(Err(self.err("expected delimiter or newline"))),
            }
        }
    }

    fn read_field(&mut self) -> Result<Cow<'a, str>, Error> {
        if self.bytes.get(self.pos) == Some(&self.quote) {
            self.read_quoted()
        } else {
            self.read_unquoted()
        }
    }

    fn read_unquoted(&mut self) -> Result<Cow<'a, str>, Error> {
        let start = self.pos;
        while let Some(&b) = self.bytes.get(self.pos) {
            if b == self.delim || b == b'\r' || b == b'\n' { break; }
            self.pos += 1;
        }
        let slice = std::str::from_utf8(&self.bytes[start..self.pos])
            .map_err(|_| self.err("invalid utf-8"))?;
        Ok(Cow::Borrowed(slice))
    }

    fn read_quoted(&mut self) -> Result<Cow<'a, str>, Error> {
        self.pos += 1;
        let start = self.pos;
        while let Some(&b) = self.bytes.get(self.pos) {
            if b == self.quote {
                if self.bytes.get(self.pos + 1) == Some(&self.quote) {
                    let mut out = String::with_capacity(self.pos - start + 1);
                    out.push_str(std::str::from_utf8(&self.bytes[start..self.pos])
                        .map_err(|_| self.err("invalid utf-8"))?);
                    return self.read_quoted_owned(out);
                }
                let slice = std::str::from_utf8(&self.bytes[start..self.pos])
                    .map_err(|_| self.err("invalid utf-8"))?;
                self.pos += 1;
                return Ok(Cow::Borrowed(slice));
            }
            if b == b'\n' { self.line += 1; self.line_start = self.pos + 1; }
            self.pos += 1;
        }
        Err(self.err("unterminated quoted field"))
    }

    fn read_quoted_owned(&mut self, mut out: String) -> Result<Cow<'a, str>, Error> {
        out.push(self.quote as char);
        self.pos += 2;
        let mut chunk = self.pos;
        while let Some(&b) = self.bytes.get(self.pos) {
            if b == self.quote {
                out.push_str(std::str::from_utf8(&self.bytes[chunk..self.pos])
                    .map_err(|_| self.err("invalid utf-8"))?);
                if self.bytes.get(self.pos + 1) == Some(&self.quote) {
                    out.push(self.quote as char);
                    self.pos += 2;
                    chunk = self.pos;
                    continue;
                }
                self.pos += 1;
                return Ok(Cow::Owned(out));
            }
            if b == b'\n' { self.line += 1; self.line_start = self.pos + 1; }
            self.pos += 1;
        }
        Err(self.err("unterminated quoted field"))
    }
}

impl<'a> Iterator for Reader<'a> {
    type Item = Result<Vec<Cow<'a, str>>, Error>;
    fn next(&mut self) -> Option<Self::Item> { self.read_record() }
}

pub fn parse_str(s: &str) -> Result<Vec<Vec<String>>, Error> {
    let mut out = Vec::new();
    let mut r = Reader::new(s);
    while let Some(rec) = r.read_record() {
        out.push(rec?.into_iter().map(|c| c.into_owned()).collect());
    }
    Ok(out)
}

pub struct Writer {
    pub out: String,
    delim: u8,
    quote: u8,
    needs_quote: [bool; 256],
}

impl Writer {
    pub fn new() -> Self { Writer::with_delim(b',') }

    pub fn with_delim(delim: u8) -> Self {
        let quote = b'"';
        let mut nq = [false; 256];
        nq[delim as usize] = true;
        nq[quote as usize] = true;
        nq[b'\n' as usize] = true;
        nq[b'\r' as usize] = true;
        Writer { out: String::new(), delim, quote, needs_quote: nq }
    }

    pub fn into_string(self) -> String { self.out }

    pub fn write_record<I, F>(&mut self, fields: I)
    where I: IntoIterator<Item = F>, F: AsRef<str>
    {
        let mut first = true;
        for f in fields {
            if !first { self.out.push(self.delim as char); }
            first = false;
            self.write_field(f.as_ref());
        }
        self.out.push('\n');
    }

    fn write_field(&mut self, s: &str) {
        let bytes = s.as_bytes();
        let needs = bytes.iter().any(|&b| self.needs_quote[b as usize]);
        if !needs { self.out.push_str(s); return; }
        self.out.push(self.quote as char);
        let mut chunk = 0;
        for (i, &b) in bytes.iter().enumerate() {
            if b == self.quote {
                self.out.push_str(std::str::from_utf8(&bytes[chunk..i]).unwrap());
                self.out.push(self.quote as char);
                self.out.push(self.quote as char);
                chunk = i + 1;
            }
        }
        self.out.push_str(std::str::from_utf8(&bytes[chunk..]).unwrap());
        self.out.push(self.quote as char);
    }
}

impl Default for Writer { fn default() -> Self { Self::new() } }

pub fn to_string(records: &[Vec<String>]) -> String {
    let mut w = Writer::new();
    for r in records { w.write_record(r); }
    w.into_string()
}
