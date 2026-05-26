use std::cmp::Ordering;
use std::fmt;
use std::str::FromStr;

#[derive(Debug, Clone, PartialEq)]
pub struct Error(pub String);

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

fn err<T>(msg: impl Into<String>) -> Result<T, Error> {
    Err(Error(msg.into()))
}

#[derive(Debug, Clone, Eq, Hash)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
    pub pre: Prerelease,
    pub build: BuildMetadata,
}

#[derive(Debug, Clone, Default, Eq, Hash)]
pub struct Prerelease(String);

#[derive(Debug, Clone, Default, Eq, Hash)]
pub struct BuildMetadata(String);

impl Prerelease {
    pub const EMPTY: Self = Prerelease(String::new());
    pub fn new(text: &str) -> Result<Self, Error> {
        if text.is_empty() { return Ok(Self::EMPTY); }
        validate_identifiers(text, true)?;
        Ok(Prerelease(text.into()))
    }
    pub fn is_empty(&self) -> bool { self.0.is_empty() }
    pub fn as_str(&self) -> &str { &self.0 }
}

impl BuildMetadata {
    pub const EMPTY: Self = BuildMetadata(String::new());
    pub fn new(text: &str) -> Result<Self, Error> {
        if text.is_empty() { return Ok(Self::EMPTY); }
        validate_identifiers(text, false)?;
        Ok(BuildMetadata(text.into()))
    }
    pub fn is_empty(&self) -> bool { self.0.is_empty() }
    pub fn as_str(&self) -> &str { &self.0 }
}

fn validate_identifiers(text: &str, numeric_no_leading_zero: bool) -> Result<(), Error> {
    if text.is_empty() { return err("empty identifier"); }
    for ident in text.split('.') {
        if ident.is_empty() { return err("empty identifier"); }
        if !ident.bytes().all(is_alnum_hyphen) {
            return err("invalid identifier char");
        }
        if numeric_no_leading_zero && ident.bytes().all(|b| b.is_ascii_digit())
            && ident.len() > 1 && ident.starts_with('0') {
            return err("leading zero in numeric identifier");
        }
    }
    Ok(())
}

fn is_alnum_hyphen(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'-'
}

impl PartialEq for Prerelease {
    fn eq(&self, other: &Self) -> bool { self.0 == other.0 }
}

impl PartialEq for BuildMetadata {
    fn eq(&self, other: &Self) -> bool { self.0 == other.0 }
}

impl PartialEq for Version {
    fn eq(&self, other: &Self) -> bool {
        self.major == other.major && self.minor == other.minor
            && self.patch == other.patch && self.pre == other.pre
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.major, self.minor, self.patch)
            .cmp(&(other.major, other.minor, other.patch))
            .then_with(|| cmp_pre(&self.pre, &other.pre))
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> { Some(self.cmp(other)) }
}

impl PartialOrd for Prerelease {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> { Some(cmp_pre(self, other)) }
}

impl Ord for Prerelease {
    fn cmp(&self, other: &Self) -> Ordering { cmp_pre(self, other) }
}

fn cmp_pre(a: &Prerelease, b: &Prerelease) -> Ordering {
    match (a.is_empty(), b.is_empty()) {
        (true, true) => Ordering::Equal,
        (true, false) => Ordering::Greater,
        (false, true) => Ordering::Less,
        (false, false) => cmp_identifiers(&a.0, &b.0),
    }
}

fn cmp_identifiers(a: &str, b: &str) -> Ordering {
    let mut ai = a.split('.');
    let mut bi = b.split('.');
    loop {
        match (ai.next(), bi.next()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) => {
                let ord = cmp_one_identifier(x, y);
                if ord != Ordering::Equal { return ord; }
            }
        }
    }
}

fn cmp_one_identifier(a: &str, b: &str) -> Ordering {
    let an = a.bytes().all(|b| b.is_ascii_digit());
    let bn = b.bytes().all(|b| b.is_ascii_digit());
    match (an, bn) {
        (true, true) => a.parse::<u64>().unwrap().cmp(&b.parse().unwrap()),
        (true, false) => Ordering::Less,
        (false, true) => Ordering::Greater,
        (false, false) => a.cmp(b),
    }
}

impl Version {
    pub fn new(major: u64, minor: u64, patch: u64) -> Self {
        Version {
            major, minor, patch,
            pre: Prerelease::EMPTY,
            build: BuildMetadata::EMPTY,
        }
    }
    pub fn parse(text: &str) -> Result<Self, Error> { parse_version(text) }
}

impl FromStr for Version {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self, Error> { Self::parse(s) }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)?;
        if !self.pre.is_empty() { write!(f, "-{}", self.pre.0)?; }
        if !self.build.is_empty() { write!(f, "+{}", self.build.0)?; }
        Ok(())
    }
}

impl fmt::Display for Prerelease {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result { f.write_str(&self.0) }
}

impl fmt::Display for BuildMetadata {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result { f.write_str(&self.0) }
}

fn parse_version(text: &str) -> Result<Version, Error> {
    let (major, rest) = parse_num(text)?;
    let rest = expect(rest, '.')?;
    let (minor, rest) = parse_num(rest)?;
    let rest = expect(rest, '.')?;
    let (patch, rest) = parse_num(rest)?;
    let (pre, rest) = if let Some(r) = rest.strip_prefix('-') {
        let end = r.find(|c: char| c == '+').unwrap_or(r.len());
        if end == 0 { return err("empty pre-release"); }
        (Prerelease::new(&r[..end])?, &r[end..])
    } else {
        (Prerelease::EMPTY, rest)
    };
    let build = if let Some(r) = rest.strip_prefix('+') {
        if r.is_empty() { return err("empty build metadata"); }
        BuildMetadata::new(r)?
    } else if !rest.is_empty() {
        return err(format!("unexpected trailing data: {rest:?}"));
    } else {
        BuildMetadata::EMPTY
    };
    Ok(Version { major, minor, patch, pre, build })
}

fn parse_num(text: &str) -> Result<(u64, &str), Error> {
    let end = text.bytes().take_while(|b| b.is_ascii_digit()).count();
    if end == 0 { return err("expected number"); }
    if end > 1 && text.starts_with('0') {
        return err("leading zero in numeric");
    }
    let n: u64 = text[..end].parse().map_err(|e: std::num::ParseIntError| Error(e.to_string()))?;
    Ok((n, &text[end..]))
}

fn expect(text: &str, c: char) -> Result<&str, Error> {
    text.strip_prefix(c).ok_or_else(|| Error(format!("expected {c:?}")))
}

#[derive(Debug, Clone, PartialEq)]
pub struct VersionReq {
    pub comparators: Vec<Comparator>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Comparator {
    pub op: Op,
    pub major: u64,
    pub minor: Option<u64>,
    pub patch: Option<u64>,
    pub pre: Prerelease,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    Exact,
    Greater,
    GreaterEq,
    Less,
    LessEq,
    Tilde,
    Caret,
    Wildcard,
}

impl VersionReq {
    pub const STAR: VersionReq = VersionReq { comparators: Vec::new() };
    pub fn parse(text: &str) -> Result<Self, Error> { parse_req(text) }
    pub fn matches(&self, version: &Version) -> bool {
        if self.comparators.is_empty() { return version.pre.is_empty(); }
        if !self.comparators.iter().all(|c| c.matches(version)) { return false; }
        if version.pre.is_empty() { return true; }
        self.comparators.iter().any(|c| c.pre_allows(version))
    }
}

impl FromStr for VersionReq {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self, Error> { Self::parse(s) }
}

impl fmt::Display for VersionReq {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        if self.comparators.is_empty() { return f.write_str("*"); }
        for (i, c) in self.comparators.iter().enumerate() {
            if i > 0 { f.write_str(", ")?; }
            write!(f, "{c}")?;
        }
        Ok(())
    }
}

impl fmt::Display for Comparator {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let prefix = match self.op {
            Op::Exact => "=", Op::Greater => ">", Op::GreaterEq => ">=",
            Op::Less => "<", Op::LessEq => "<=", Op::Tilde => "~",
            Op::Caret => "^", Op::Wildcard => "",
        };
        write!(f, "{prefix}{}", self.major)?;
        if let Some(m) = self.minor {
            write!(f, ".{m}")?;
            if let Some(p) = self.patch {
                write!(f, ".{p}")?;
                if !self.pre.is_empty() { write!(f, "-{}", self.pre.0)?; }
            } else if self.op == Op::Wildcard {
                f.write_str(".*")?;
            }
        } else if self.op == Op::Wildcard {
            f.write_str(".*")?;
        }
        Ok(())
    }
}

fn parse_req(text: &str) -> Result<VersionReq, Error> {
    let text = text.trim();
    if text == "*" || text.is_empty() {
        return Ok(VersionReq { comparators: Vec::new() });
    }
    let mut comparators = Vec::new();
    for part in text.split(',') {
        comparators.push(parse_comparator(part.trim())?);
    }
    Ok(VersionReq { comparators })
}

fn parse_comparator(text: &str) -> Result<Comparator, Error> {
    let (op, rest) = take_op(text);
    let (major_raw, rest) = take_part(rest);
    if major_raw == "*" || major_raw == "x" || major_raw == "X" {
        if op != Op::Caret && op != Op::Wildcard {
            return err("wildcard with operator");
        }
        return Ok(Comparator { op: Op::Wildcard, major: 0, minor: None, patch: None, pre: Prerelease::EMPTY });
    }
    let major: u64 = major_raw.parse().map_err(|_| Error(format!("bad major: {major_raw:?}")))?;
    let rest = rest.trim_start();
    if !rest.starts_with('.') {
        return Ok(Comparator { op, major, minor: None, patch: None, pre: Prerelease::EMPTY });
    }
    let rest = &rest[1..];
    let (minor_raw, rest) = take_part(rest);
    if minor_raw == "*" || minor_raw == "x" || minor_raw == "X" {
        return Ok(Comparator { op: Op::Wildcard, major, minor: None, patch: None, pre: Prerelease::EMPTY });
    }
    let minor: u64 = minor_raw.parse().map_err(|_| Error(format!("bad minor: {minor_raw:?}")))?;
    let rest = rest.trim_start();
    if !rest.starts_with('.') {
        return Ok(Comparator { op, major, minor: Some(minor), patch: None, pre: Prerelease::EMPTY });
    }
    let rest = &rest[1..];
    let (patch_raw, rest) = take_part(rest);
    if patch_raw == "*" || patch_raw == "x" || patch_raw == "X" {
        return Ok(Comparator { op: Op::Wildcard, major, minor: Some(minor), patch: None, pre: Prerelease::EMPTY });
    }
    let patch: u64 = patch_raw.parse().map_err(|_| Error(format!("bad patch: {patch_raw:?}")))?;
    let pre = if let Some(r) = rest.strip_prefix('-') {
        let end = r.find(|c: char| c == '+' || c == ',' || c.is_whitespace()).unwrap_or(r.len());
        Prerelease::new(&r[..end])?
    } else {
        Prerelease::EMPTY
    };
    Ok(Comparator { op, major, minor: Some(minor), patch: Some(patch), pre })
}

fn take_op(text: &str) -> (Op, &str) {
    let t = text.trim_start();
    if let Some(r) = t.strip_prefix(">=") { return (Op::GreaterEq, r); }
    if let Some(r) = t.strip_prefix("<=") { return (Op::LessEq, r); }
    if let Some(r) = t.strip_prefix('>') { return (Op::Greater, r); }
    if let Some(r) = t.strip_prefix('<') { return (Op::Less, r); }
    if let Some(r) = t.strip_prefix('=') { return (Op::Exact, r); }
    if let Some(r) = t.strip_prefix('~') { return (Op::Tilde, r); }
    if let Some(r) = t.strip_prefix('^') { return (Op::Caret, r); }
    (Op::Caret, t)
}

fn take_part(text: &str) -> (&str, &str) {
    let t = text.trim_start();
    let end = t.bytes()
        .take_while(|b| b.is_ascii_alphanumeric() || *b == b'*')
        .count();
    (&t[..end], &t[end..])
}

impl Comparator {
    pub fn matches(&self, version: &Version) -> bool {
        match self.op {
            Op::Exact => self.matches_exact(version),
            Op::Greater => self.matches_greater(version),
            Op::GreaterEq => self.matches_exact(version) || self.matches_greater(version),
            Op::Less => !self.matches_exact(version) && !self.matches_greater(version),
            Op::LessEq => !self.matches_greater(version),
            Op::Tilde => self.matches_tilde(version),
            Op::Caret => self.matches_caret(version),
            Op::Wildcard => self.matches_wildcard(version),
        }
    }

    fn matches_exact(&self, v: &Version) -> bool {
        if v.major != self.major { return false; }
        if let Some(m) = self.minor { if v.minor != m { return false; } }
        if let Some(p) = self.patch { if v.patch != p { return false; } }
        v.pre == self.pre
    }

    fn matches_greater(&self, v: &Version) -> bool {
        if v.major != self.major { return v.major > self.major; }
        let Some(minor) = self.minor else { return false; };
        if v.minor != minor { return v.minor > minor; }
        let Some(patch) = self.patch else { return false; };
        if v.patch != patch { return v.patch > patch; }
        v.pre > self.pre
    }

    fn matches_tilde(&self, v: &Version) -> bool {
        if v.major != self.major { return false; }
        let Some(minor) = self.minor else { return true; };
        if v.minor != minor { return false; }
        let Some(patch) = self.patch else { return true; };
        if v.patch != patch { return v.patch > patch; }
        v.pre >= self.pre
    }

    fn matches_caret(&self, v: &Version) -> bool {
        if v.major != self.major { return false; }
        let Some(minor) = self.minor else { return true; };
        let Some(patch) = self.patch else {
            if self.major > 0 { return v.minor >= minor; }
            return v.minor == minor;
        };
        if self.major > 0 {
            if v.minor != minor { return v.minor > minor; }
            if v.patch != patch { return v.patch > patch; }
        } else if minor > 0 {
            if v.minor != minor { return false; }
            if v.patch != patch { return v.patch > patch; }
        } else if v.minor != minor || v.patch != patch {
            return false;
        }
        v.pre >= self.pre
    }

    fn matches_wildcard(&self, v: &Version) -> bool {
        if v.major != self.major { return false; }
        match self.minor {
            None => true,
            Some(m) => v.minor == m,
        }
    }

    fn pre_allows(&self, v: &Version) -> bool {
        !self.pre.is_empty() && self.major == v.major
            && self.minor == Some(v.minor) && self.patch == Some(v.patch)
    }
}
