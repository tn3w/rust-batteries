use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub struct Error {
    pub index: usize,
    pub msg: &'static str,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{} at index {}", self.msg, self.index)
    }
}

impl std::error::Error for Error {}

const LOWER: &[u8; 16] = b"0123456789abcdef";
const UPPER: &[u8; 16] = b"0123456789ABCDEF";

pub fn encode(input: &[u8]) -> String {
    encode_with(input, LOWER)
}

pub fn encode_upper(input: &[u8]) -> String {
    encode_with(input, UPPER)
}

fn encode_with(input: &[u8], table: &[u8; 16]) -> String {
    let mut out = vec![0u8; input.len() * 2];
    for (i, &b) in input.iter().enumerate() {
        out[i * 2] = table[(b >> 4) as usize];
        out[i * 2 + 1] = table[(b & 0x0f) as usize];
    }
    unsafe { String::from_utf8_unchecked(out) }
}

pub fn decode(s: impl AsRef<[u8]>) -> Result<Vec<u8>, Error> {
    let bytes = s.as_ref();
    if bytes.len() % 2 != 0 {
        return Err(Error { index: bytes.len(), msg: "odd length" });
    }
    let mut out = Vec::with_capacity(bytes.len() / 2);
    for i in (0..bytes.len()).step_by(2) {
        let hi = nibble(bytes[i]).ok_or(Error { index: i, msg: "invalid hex char" })?;
        let lo = nibble(bytes[i + 1])
            .ok_or(Error { index: i + 1, msg: "invalid hex char" })?;
        out.push((hi << 4) | lo);
    }
    Ok(out)
}

fn nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}
