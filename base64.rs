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

const STD: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
const URL: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

pub fn encode(input: &[u8]) -> String { encode_with(input, STD, true) }
pub fn encode_url(input: &[u8]) -> String { encode_with(input, URL, false) }

pub fn decode(s: impl AsRef<[u8]>) -> Result<Vec<u8>, Error> {
    decode_with(s.as_ref(), &decode_table(STD), true)
}

pub fn decode_url(s: impl AsRef<[u8]>) -> Result<Vec<u8>, Error> {
    decode_with(s.as_ref(), &decode_table(URL), false)
}

fn encode_with(input: &[u8], table: &[u8; 64], pad: bool) -> String {
    let n = input.len();
    let groups = n / 3;
    let rem = n % 3;
    let out_len = groups * 4 + if rem == 0 { 0 } else if pad { 4 } else { rem + 1 };
    let mut out = Vec::with_capacity(out_len);

    for chunk in input[..groups * 3].chunks_exact(3) {
        let v = ((chunk[0] as u32) << 16) | ((chunk[1] as u32) << 8) | (chunk[2] as u32);
        out.push(table[((v >> 18) & 0x3f) as usize]);
        out.push(table[((v >> 12) & 0x3f) as usize]);
        out.push(table[((v >> 6) & 0x3f) as usize]);
        out.push(table[(v & 0x3f) as usize]);
    }

    if rem == 1 {
        let v = (input[n - 1] as u32) << 16;
        out.push(table[((v >> 18) & 0x3f) as usize]);
        out.push(table[((v >> 12) & 0x3f) as usize]);
        if pad { out.push(b'='); out.push(b'='); }
    } else if rem == 2 {
        let v = ((input[n - 2] as u32) << 16) | ((input[n - 1] as u32) << 8);
        out.push(table[((v >> 18) & 0x3f) as usize]);
        out.push(table[((v >> 12) & 0x3f) as usize]);
        out.push(table[((v >> 6) & 0x3f) as usize]);
        if pad { out.push(b'='); }
    }

    unsafe { String::from_utf8_unchecked(out) }
}

fn decode_table(alphabet: &[u8; 64]) -> [i8; 256] {
    let mut t = [-1i8; 256];
    let mut i = 0;
    while i < 64 {
        t[alphabet[i] as usize] = i as i8;
        i += 1;
    }
    t
}

fn decode_with(input: &[u8], table: &[i8; 256], require_pad: bool) -> Result<Vec<u8>, Error> {
    if require_pad && input.len() % 4 != 0 {
        return Err(Error { index: input.len(), msg: "invalid padded length" });
    }
    let trimmed = trim_padding(input);
    let n = trimmed.len();
    if !require_pad && contains_pad(input) {
        return Err(Error { index: n, msg: "unexpected padding" });
    }
    if n % 4 == 1 {
        return Err(Error { index: n, msg: "invalid length" });
    }
    let full_groups = n / 4;
    let rem = n % 4;
    let out_len = full_groups * 3 + match rem { 2 => 1, 3 => 2, _ => 0 };
    let mut out = Vec::with_capacity(out_len);

    for g in 0..full_groups {
        let i = g * 4;
        let v = pack4(&trimmed[i..i + 4], table, i)?;
        out.push((v >> 16) as u8);
        out.push((v >> 8) as u8);
        out.push(v as u8);
    }

    if rem == 2 {
        let i = full_groups * 4;
        let a = sym(trimmed[i], table, i)?;
        let b = sym(trimmed[i + 1], table, i + 1)?;
        if b & 0x0f != 0 {
            return Err(Error { index: i + 1, msg: "invalid trailing bits" });
        }
        out.push((a << 2) | (b >> 4));
    } else if rem == 3 {
        let i = full_groups * 4;
        let a = sym(trimmed[i], table, i)?;
        let b = sym(trimmed[i + 1], table, i + 1)?;
        let c = sym(trimmed[i + 2], table, i + 2)?;
        if c & 0x03 != 0 {
            return Err(Error { index: i + 2, msg: "invalid trailing bits" });
        }
        out.push((a << 2) | (b >> 4));
        out.push((b << 4) | (c >> 2));
    }

    Ok(out)
}

fn trim_padding(input: &[u8]) -> &[u8] {
    let mut end = input.len();
    while end > 0 && input[end - 1] == b'=' { end -= 1; }
    &input[..end]
}

fn contains_pad(input: &[u8]) -> bool {
    input.iter().any(|&b| b == b'=')
}

fn pack4(chunk: &[u8], table: &[i8; 256], base: usize) -> Result<u32, Error> {
    let a = sym(chunk[0], table, base)? as u32;
    let b = sym(chunk[1], table, base + 1)? as u32;
    let c = sym(chunk[2], table, base + 2)? as u32;
    let d = sym(chunk[3], table, base + 3)? as u32;
    Ok((a << 18) | (b << 12) | (c << 6) | d)
}

fn sym(b: u8, table: &[i8; 256], index: usize) -> Result<u8, Error> {
    let v = table[b as usize];
    if v < 0 { return Err(Error { index, msg: "invalid base64 char" }); }
    Ok(v as u8)
}
