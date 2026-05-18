use std::cell::RefCell;
use std::fmt;
use std::io::Read;
use std::str::FromStr;

#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Default)]
pub struct Uuid(pub [u8; 16]);

#[derive(Debug, PartialEq, Eq)]
pub enum Error {
    InvalidLength(usize),
    InvalidGroupLength,
    InvalidChar(u8),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Error::InvalidLength(n) => write!(f, "invalid length {n}"),
            Error::InvalidGroupLength => f.write_str("invalid group layout"),
            Error::InvalidChar(c) => write!(f, "invalid character {:?}", *c as char),
        }
    }
}
impl std::error::Error for Error {}

impl Uuid {
    pub const fn nil() -> Self { Uuid([0; 16]) }
    pub const fn max() -> Self { Uuid([0xff; 16]) }
    pub const fn from_bytes(b: [u8; 16]) -> Self { Uuid(b) }
    pub const fn as_bytes(&self) -> &[u8; 16] { &self.0 }
    pub const fn into_bytes(self) -> [u8; 16] { self.0 }

    pub const fn is_nil(&self) -> bool {
        let mut i = 0;
        while i < 16 { if self.0[i] != 0 { return false; } i += 1; }
        true
    }

    pub const fn get_version_num(&self) -> usize { (self.0[6] >> 4) as usize }

    pub fn new_v4() -> Self {
        let mut b = [0u8; 16];
        fill_random(&mut b);
        b[6] = (b[6] & 0x0f) | 0x40;
        b[8] = (b[8] & 0x3f) | 0x80;
        Uuid(b)
    }

    pub fn now_v7() -> Self {
        let ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u64;
        let mut b = [0u8; 16];
        let t = ms.to_be_bytes();
        b[..6].copy_from_slice(&t[2..]);
        fill_random(&mut b[6..]);
        b[6] = (b[6] & 0x0f) | 0x70;
        b[8] = (b[8] & 0x3f) | 0x80;
        Uuid(b)
    }

    pub fn parse_str(input: &str) -> Result<Self, Error> {
        let mut s = input;
        if let Some(rest) = s.strip_prefix("urn:uuid:") { s = rest; }
        if s.starts_with('{') && s.ends_with('}') { s = &s[1..s.len() - 1]; }
        let bytes = s.as_bytes();
        let mut out = [0u8; 16];
        match bytes.len() {
            32 => {
                for i in 0..16 {
                    out[i] = (nibble(bytes[i * 2])? << 4) | nibble(bytes[i * 2 + 1])?;
                }
            }
            36 => {
                if bytes[8] != b'-' || bytes[13] != b'-'
                    || bytes[18] != b'-' || bytes[23] != b'-'
                {
                    return Err(Error::InvalidGroupLength);
                }
                let positions = [0, 2, 4, 6, 9, 11, 14, 16, 19, 21,
                    24, 26, 28, 30, 32, 34];
                for (i, &p) in positions.iter().enumerate() {
                    out[i] = (nibble(bytes[p])? << 4) | nibble(bytes[p + 1])?;
                }
            }
            n => return Err(Error::InvalidLength(n)),
        }
        Ok(Uuid(out))
    }

    pub fn hyphenated(&self) -> Hyphenated { Hyphenated(*self) }
    pub fn simple(&self) -> Simple { Simple(*self) }
    pub fn braced(&self) -> Braced { Braced(*self) }
    pub fn urn(&self) -> Urn { Urn(*self) }
}

pub struct Hyphenated(pub Uuid);
pub struct Simple(pub Uuid);
pub struct Braced(pub Uuid);
pub struct Urn(pub Uuid);

const HEX: &[u8; 16] = b"0123456789abcdef";

fn write_hex(b: &[u8; 16], buf: &mut [u8], positions: [usize; 16]) {
    for (i, &p) in positions.iter().enumerate() {
        buf[p] = HEX[(b[i] >> 4) as usize];
        buf[p + 1] = HEX[(b[i] & 0xf) as usize];
    }
}

fn hyphenated_into(b: &[u8; 16], buf: &mut [u8; 36]) {
    write_hex(b, buf, [0, 2, 4, 6, 9, 11, 14, 16, 19, 21,
        24, 26, 28, 30, 32, 34]);
    buf[8] = b'-'; buf[13] = b'-'; buf[18] = b'-'; buf[23] = b'-';
}

fn simple_into(b: &[u8; 16], buf: &mut [u8; 32]) {
    write_hex(b, buf, [0, 2, 4, 6, 8, 10, 12, 14, 16, 18,
        20, 22, 24, 26, 28, 30]);
}

impl fmt::Display for Uuid {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let mut buf = [0u8; 36];
        hyphenated_into(&self.0, &mut buf);
        f.write_str(unsafe { std::str::from_utf8_unchecked(&buf) })
    }
}
impl fmt::Debug for Uuid {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}
impl fmt::Display for Hyphenated {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}
impl fmt::Display for Simple {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let mut buf = [0u8; 32];
        simple_into(&self.0.0, &mut buf);
        f.write_str(unsafe { std::str::from_utf8_unchecked(&buf) })
    }
}
impl fmt::Display for Braced {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let mut buf = [0u8; 38];
        buf[0] = b'{';
        buf[37] = b'}';
        let mut inner = [0u8; 36];
        hyphenated_into(&self.0.0, &mut inner);
        buf[1..37].copy_from_slice(&inner);
        f.write_str(unsafe { std::str::from_utf8_unchecked(&buf) })
    }
}
impl fmt::Display for Urn {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "urn:uuid:{}", self.0)
    }
}

impl FromStr for Uuid {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self, Error> { Uuid::parse_str(s) }
}

fn nibble(b: u8) -> Result<u8, Error> {
    match b {
        b'0'..=b'9' => Ok(b - b'0'),
        b'a'..=b'f' => Ok(b - b'a' + 10),
        b'A'..=b'F' => Ok(b - b'A' + 10),
        _ => Err(Error::InvalidChar(b)),
    }
}

struct RandPool { buf: [u8; 1024], pos: usize }

impl RandPool {
    fn new() -> Self {
        let mut p = RandPool { buf: [0; 1024], pos: 1024 };
        p.refill();
        p
    }
    fn refill(&mut self) {
        let mut f = std::fs::File::open("/dev/urandom").expect("open urandom");
        f.read_exact(&mut self.buf).expect("read urandom");
        self.pos = 0;
    }
    fn fill(&mut self, out: &mut [u8]) {
        let mut written = 0;
        while written < out.len() {
            if self.pos == self.buf.len() { self.refill(); }
            let take = (out.len() - written).min(self.buf.len() - self.pos);
            out[written..written + take]
                .copy_from_slice(&self.buf[self.pos..self.pos + take]);
            self.pos += take;
            written += take;
        }
    }
}

thread_local! { static POOL: RefCell<RandPool> = RefCell::new(RandPool::new()); }

fn fill_random(out: &mut [u8]) { POOL.with(|p| p.borrow_mut().fill(out)); }
