use std::env;
use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub enum Error {
    Io(std::io::Error),
    LineParse(String, usize),
    NotFound,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "io: {e}"),
            Error::LineParse(line, col) => {
                write!(f, "parse error on line {line:?} col {col}")
            }
            Error::NotFound => f.write_str(".env file not found"),
        }
    }
}
impl std::error::Error for Error {}
impl From<std::io::Error> for Error { fn from(e: std::io::Error) -> Self { Error::Io(e) } }

pub fn dotenv() -> Result<PathBuf, Error> { from_filename(".env") }
pub fn dotenv_override() -> Result<PathBuf, Error> { from_filename_override(".env") }

pub fn from_filename<P: AsRef<Path>>(name: P) -> Result<PathBuf, Error> {
    let path = find_up(name.as_ref())?;
    from_path(&path)?;
    Ok(path)
}

pub fn from_filename_override<P: AsRef<Path>>(name: P) -> Result<PathBuf, Error> {
    let path = find_up(name.as_ref())?;
    from_path_override(&path)?;
    Ok(path)
}

pub fn from_path<P: AsRef<Path>>(path: P) -> Result<(), Error> {
    apply(parse(&fs::read_to_string(path)?)?, false)
}

pub fn from_path_override<P: AsRef<Path>>(path: P) -> Result<(), Error> {
    apply(parse(&fs::read_to_string(path)?)?, true)
}

pub fn from_read<R: std::io::Read>(mut r: R) -> Result<Vec<(String, String)>, Error> {
    let mut s = String::new();
    r.read_to_string(&mut s)?;
    parse(&s)
}

fn find_up(name: &Path) -> Result<PathBuf, Error> {
    let mut cur = env::current_dir()?;
    loop {
        let candidate = cur.join(name);
        if candidate.is_file() { return Ok(candidate); }
        if !cur.pop() { return Err(Error::NotFound); }
    }
}

fn apply(vars: Vec<(String, String)>, ov: bool) -> Result<(), Error> {
    for (k, v) in vars {
        if ov || env::var(&k).is_err() { env::set_var(k, v); }
    }
    Ok(())
}

pub fn parse(input: &str) -> Result<Vec<(String, String)>, Error> {
    let mut out = Vec::new();
    let mut p = Parser { bytes: input.as_bytes(), pos: 0, locals: Vec::new() };
    while p.pos < p.bytes.len() {
        p.skip_blank_and_comments();
        if p.pos >= p.bytes.len() { break; }
        let (k, v) = p.parse_assignment()?;
        p.locals.push((k.clone(), v.clone()));
        out.push((k, v));
    }
    Ok(out)
}

struct Parser<'a> {
    bytes: &'a [u8],
    pos: usize,
    locals: Vec<(String, String)>,
}

impl<'a> Parser<'a> {
    fn peek(&self) -> Option<u8> { self.bytes.get(self.pos).copied() }

    fn skip_blank_and_comments(&mut self) {
        while let Some(c) = self.peek() {
            match c {
                b' ' | b'\t' | b'\r' | b'\n' => self.pos += 1,
                b'#' => while let Some(c) = self.peek() {
                    self.pos += 1;
                    if c == b'\n' { break; }
                },
                _ => break,
            }
        }
    }

    fn parse_assignment(&mut self) -> Result<(String, String), Error> {
        let line_start = self.pos;
        self.skip_inline_ws();
        if self.bytes[self.pos..].starts_with(b"export ")
            || self.bytes[self.pos..].starts_with(b"export\t")
        {
            self.pos += 7;
            self.skip_inline_ws();
        }
        let key_start = self.pos;
        while let Some(c) = self.peek() {
            if c.is_ascii_alphanumeric() || c == b'_' || c == b'.' { self.pos += 1; }
            else { break; }
        }
        if self.pos == key_start { return Err(self.err(line_start)); }
        let key = std::str::from_utf8(&self.bytes[key_start..self.pos])
            .map_err(|_| self.err(line_start))?.to_string();
        self.skip_inline_ws();
        if self.peek() != Some(b'=') { return Err(self.err(line_start)); }
        self.pos += 1;
        self.skip_inline_ws();
        let value = self.parse_value()?;
        Ok((key, value))
    }

    fn skip_inline_ws(&mut self) {
        while matches!(self.peek(), Some(b' ') | Some(b'\t')) { self.pos += 1; }
    }

    fn err(&self, line_start: usize) -> Error {
        let end = self.bytes[line_start..].iter()
            .position(|&b| b == b'\n').map(|i| line_start + i)
            .unwrap_or(self.bytes.len());
        let line = String::from_utf8_lossy(&self.bytes[line_start..end]).into_owned();
        Error::LineParse(line, self.pos - line_start)
    }

    fn parse_value(&mut self) -> Result<String, Error> {
        match self.peek() {
            Some(b'\'') => self.parse_single(),
            Some(b'"') => self.parse_double(),
            _ => self.parse_bare(),
        }
    }

    fn parse_single(&mut self) -> Result<String, Error> {
        self.pos += 1;
        let start = self.pos;
        while let Some(c) = self.peek() {
            if c == b'\'' {
                let s = String::from_utf8_lossy(&self.bytes[start..self.pos]).into_owned();
                self.pos += 1;
                self.skip_to_eol();
                return Ok(s);
            }
            self.pos += 1;
        }
        Err(self.err(start))
    }

    fn parse_double(&mut self) -> Result<String, Error> {
        self.pos += 1;
        let mut out = String::new();
        while let Some(c) = self.peek() {
            match c {
                b'"' => {
                    self.pos += 1;
                    self.skip_to_eol();
                    return Ok(out);
                }
                b'\\' => {
                    self.pos += 1;
                    match self.peek() {
                        Some(b'n') => out.push('\n'),
                        Some(b'r') => out.push('\r'),
                        Some(b't') => out.push('\t'),
                        Some(b'\\') => out.push('\\'),
                        Some(b'"') => out.push('"'),
                        Some(b'\'') => out.push('\''),
                        Some(b'$') => out.push('$'),
                        Some(b) => out.push(b as char),
                        None => return Err(self.err(self.pos)),
                    }
                    self.pos += 1;
                }
                b'$' => {
                    self.pos += 1;
                    out.push_str(&self.read_var());
                }
                _ => {
                    out.push(c as char);
                    self.pos += 1;
                }
            }
        }
        Err(self.err(self.pos))
    }

    fn parse_bare(&mut self) -> Result<String, Error> {
        let mut out = String::new();
        while let Some(c) = self.peek() {
            match c {
                b'\n' => break,
                b'#' => {
                    let trimmed: String = out.trim_end().to_string();
                    self.skip_to_eol();
                    return Ok(trimmed);
                }
                b'$' => {
                    self.pos += 1;
                    out.push_str(&self.read_var());
                }
                b'\\' => {
                    self.pos += 1;
                    if let Some(b) = self.peek() {
                        out.push(b as char);
                        self.pos += 1;
                    }
                }
                _ => { out.push(c as char); self.pos += 1; }
            }
        }
        Ok(out.trim_end().to_string())
    }

    fn read_var(&mut self) -> String {
        let braced = self.peek() == Some(b'{');
        if braced { self.pos += 1; }
        let start = self.pos;
        while let Some(c) = self.peek() {
            if c.is_ascii_alphanumeric() || c == b'_' { self.pos += 1; } else { break; }
        }
        let name = std::str::from_utf8(&self.bytes[start..self.pos])
            .unwrap_or("").to_string();
        if braced && self.peek() == Some(b'}') { self.pos += 1; }
        self.lookup(&name)
    }

    fn lookup(&self, name: &str) -> String {
        for (k, v) in self.locals.iter().rev() {
            if k == name { return v.clone(); }
        }
        env::var(name).unwrap_or_default()
    }

    fn skip_to_eol(&mut self) {
        while let Some(c) = self.peek() {
            self.pos += 1;
            if c == b'\n' { break; }
        }
    }
}
