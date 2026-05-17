use std::cmp::Ordering;

const LIMBS: usize = 24;
const MAX_DIGITS: usize = 17;

#[derive(Clone, Copy)]
struct Bn {
    l: [u64; LIMBS],
    n: usize,
}

impl Bn {
    fn zero() -> Self { Bn { l: [0; LIMBS], n: 0 } }

    fn from_u64(x: u64) -> Self {
        let mut b = Self::zero();
        if x != 0 { b.l[0] = x; b.n = 1; }
        b
    }

    fn is_zero(&self) -> bool { self.n == 0 }

    fn normalize(&mut self) {
        while self.n > 0 && self.l[self.n - 1] == 0 { self.n -= 1; }
    }

    fn shl(&mut self, bits: u32) {
        if self.is_zero() || bits == 0 { return; }
        let words = (bits / 64) as usize;
        let bs = bits % 64;
        if words > 0 {
            for i in (0..self.n).rev() { self.l[i + words] = self.l[i]; }
            for i in 0..words { self.l[i] = 0; }
            self.n += words;
        }
        if bs > 0 {
            let mut c = 0u64;
            for i in 0..self.n {
                let v = self.l[i];
                self.l[i] = (v << bs) | c;
                c = v >> (64 - bs);
            }
            if c != 0 { self.l[self.n] = c; self.n += 1; }
        }
    }

    fn mul_u32(&mut self, m: u32) {
        if self.is_zero() { return; }
        if m == 0 { *self = Self::zero(); return; }
        let mut c: u64 = 0;
        for i in 0..self.n {
            let p = self.l[i] as u128 * m as u128 + c as u128;
            self.l[i] = p as u64;
            c = (p >> 64) as u64;
        }
        if c != 0 { self.l[self.n] = c; self.n += 1; }
    }

    fn add(&mut self, o: &Bn) {
        let n = self.n.max(o.n);
        let mut c: u64 = 0;
        for i in 0..n {
            let a = if i < self.n { self.l[i] } else { 0 };
            let b = if i < o.n { o.l[i] } else { 0 };
            let s = a as u128 + b as u128 + c as u128;
            self.l[i] = s as u64;
            c = (s >> 64) as u64;
        }
        self.n = n;
        if c != 0 { self.l[self.n] = c; self.n += 1; }
    }

    fn sub(&mut self, o: &Bn) {
        let mut br: u64 = 0;
        for i in 0..self.n {
            let b = if i < o.n { o.l[i] } else { 0 };
            let (d1, b1) = self.l[i].overflowing_sub(b);
            let (d2, b2) = d1.overflowing_sub(br);
            self.l[i] = d2;
            br = b1 as u64 + b2 as u64;
        }
        self.normalize();
    }

    fn cmp(&self, o: &Bn) -> Ordering {
        if self.n != o.n { return self.n.cmp(&o.n); }
        for i in (0..self.n).rev() {
            match self.l[i].cmp(&o.l[i]) {
                Ordering::Equal => continue,
                ord => return ord,
            }
        }
        Ordering::Equal
    }
}

fn mul_pow10(b: &mut Bn, mut k: u32) {
    while k >= 9 { b.mul_u32(1_000_000_000); k -= 9; }
    if k > 0 {
        let mut m: u32 = 1;
        for _ in 0..k { m *= 10; }
        b.mul_u32(m);
    }
}

const MANTISSA_BITS: u32 = 52;
const EXP_BIAS: i32 = 1023;
const MIN_NORMAL_M: u64 = 1u64 << MANTISSA_BITS;

struct Decomposed {
    digits: [u8; MAX_DIGITS],
    n: usize,
    k: i32,
    neg: bool,
}

fn shortest(f: f64) -> Decomposed {
    let bits = f.to_bits();
    let neg = (bits >> 63) != 0;
    let raw_exp = ((bits >> 52) & 0x7FF) as i32;
    let raw_mant = bits & (MIN_NORMAL_M - 1);

    let (m, e) = if raw_exp == 0 {
        (raw_mant, 1 - EXP_BIAS - MANTISSA_BITS as i32)
    } else {
        (MIN_NORMAL_M | raw_mant, raw_exp - EXP_BIAS - MANTISSA_BITS as i32)
    };

    let boundary = raw_mant == 0 && raw_exp > 1;
    let even = m & 1 == 0;

    let mut r = Bn::from_u64(m);
    let mut s = Bn::from_u64(1);
    let mut mp = Bn::from_u64(1);
    let mut mm = Bn::from_u64(1);

    if e >= 0 {
        if boundary {
            r.shl(e as u32 + 2);
            s.shl(2);
            mp.shl(e as u32 + 1);
            mm.shl(e as u32);
        } else {
            r.shl(e as u32 + 1);
            s.shl(1);
            mp.shl(e as u32);
            mm.shl(e as u32);
        }
    } else if boundary {
        r.shl(2);
        s.shl((2 - e) as u32);
        mp.shl(1);
    } else {
        r.shl(1);
        s.shl((1 - e) as u32);
    }

    let v_log10 = f.abs().log10();
    let mut k = v_log10.ceil() as i32;

    if k >= 0 {
        mul_pow10(&mut s, k as u32);
    } else {
        let p = (-k) as u32;
        mul_pow10(&mut r, p);
        mul_pow10(&mut mp, p);
        mul_pow10(&mut mm, p);
    }

    let rpm_gt_s = |r: &Bn, mp: &Bn, s: &Bn| -> bool {
        let mut t = *r;
        t.add(mp);
        if even { t.cmp(s) != Ordering::Less } else { t.cmp(s) == Ordering::Greater }
    };

    while rpm_gt_s(&r, &mp, &s) {
        s.mul_u32(10);
        k += 1;
    }

    loop {
        let mut t = r;
        t.add(&mp);
        t.mul_u32(10);
        let pull = if even { t.cmp(&s) != Ordering::Greater } else { t.cmp(&s) == Ordering::Less };
        if pull {
            r.mul_u32(10);
            mp.mul_u32(10);
            mm.mul_u32(10);
            k -= 1;
        } else { break; }
    }

    let mut digits = [0u8; MAX_DIGITS];
    let mut count = 0;
    loop {
        r.mul_u32(10);
        mp.mul_u32(10);
        mm.mul_u32(10);

        let mut d: u8 = 0;
        while r.cmp(&s) != Ordering::Less {
            r.sub(&s);
            d += 1;
        }

        let low = if even { r.cmp(&mm) != Ordering::Greater } else { r.cmp(&mm) == Ordering::Less };
        let mut r_plus = r;
        r_plus.add(&mp);
        let high = if even { r_plus.cmp(&s) != Ordering::Less } else { r_plus.cmp(&s) == Ordering::Greater };

        if !low && !high {
            digits[count] = d;
            count += 1;
            if count >= MAX_DIGITS { break; }
            continue;
        }

        let final_d = if low && !high { d }
            else if !low && high { d + 1 }
            else {
                let mut r2 = r; r2.shl(1);
                match r2.cmp(&s) {
                    Ordering::Less => d,
                    Ordering::Greater => d + 1,
                    Ordering::Equal => if d & 1 == 0 { d } else { d + 1 },
                }
            };

        if final_d == 10 {
            digits[count] = 0;
            count += 1;
            let mut i = count;
            loop {
                if i == 0 { digits[0] = 1; k += 1; count = 1; break; }
                i -= 1;
                if digits[i] < 9 { digits[i] += 1; break; }
                digits[i] = 0;
                if i == 0 { digits[0] = 1; k += 1; count += 1; break; }
            }
        } else {
            digits[count] = final_d;
            count += 1;
        }
        break;
    }

    while count > 1 && digits[count - 1] == 0 { count -= 1; }

    Decomposed { digits, n: count, k, neg }
}

pub fn write_f64(out: &mut String, f: f64) {
    if f.is_nan() { out.push_str("NaN"); return; }
    if f.is_infinite() {
        if f < 0.0 { out.push('-'); }
        out.push_str("inf");
        return;
    }
    if f == 0.0 {
        if f.is_sign_negative() { out.push('-'); }
        out.push_str("0.0");
        return;
    }

    let d = shortest(f);
    format_digits(out, &d);
}

pub fn f64_to_string(f: f64) -> String {
    let mut s = String::with_capacity(24);
    write_f64(&mut s, f);
    s
}

fn format_digits(out: &mut String, d: &Decomposed) {
    if d.neg { out.push('-'); }
    let digits = &d.digits[..d.n];
    let nd = d.n as i32;
    let k = d.k;
    let exp = k - 1;

    if exp < -5 || exp >= 16 {
        out.push((b'0' + digits[0]) as char);
        if nd > 1 {
            out.push('.');
            for &b in &digits[1..] { out.push((b'0' + b) as char); }
        }
        out.push('e');
        if exp >= 0 { out.push('+'); }
        write_int(out, exp as i64);
    } else if k <= 0 {
        out.push_str("0.");
        for _ in 0..(-k) { out.push('0'); }
        for &b in digits { out.push((b'0' + b) as char); }
    } else if k >= nd {
        for &b in digits { out.push((b'0' + b) as char); }
        for _ in 0..(k - nd) { out.push('0'); }
        out.push_str(".0");
    } else {
        for i in 0..k { out.push((b'0' + digits[i as usize]) as char); }
        out.push('.');
        for i in k..nd { out.push((b'0' + digits[i as usize]) as char); }
    }
}

fn write_int(out: &mut String, n: i64) {
    if n < 0 { out.push('-'); }
    let mut x = n.unsigned_abs();
    if x == 0 { out.push('0'); return; }
    let mut buf = [0u8; 20];
    let mut i = 20;
    while x > 0 { i -= 1; buf[i] = b'0' + (x % 10) as u8; x /= 10; }
    out.push_str(std::str::from_utf8(&buf[i..]).unwrap());
}
