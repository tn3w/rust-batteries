use std::cell::UnsafeCell;
use std::io;
use std::rc::Rc;

pub fn fill_os(dst: &mut [u8]) -> io::Result<()> {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    return linux_getrandom(dst);
    #[cfg(any(target_os = "macos", target_os = "ios", target_os = "freebsd",
              target_os = "openbsd", target_os = "netbsd"))]
    return bsd_getentropy(dst);
    #[cfg(target_os = "windows")]
    return windows_bcrypt(dst);
    #[cfg(not(any(target_os = "linux", target_os = "android", target_os = "macos",
                  target_os = "ios", target_os = "freebsd", target_os = "openbsd",
                  target_os = "netbsd", target_os = "windows")))]
    return urandom_fallback(dst);
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn linux_getrandom(dst: &mut [u8]) -> io::Result<()> {
    const SYS_GETRANDOM: LibcLong = 318;
    let mut filled = 0;
    while filled < dst.len() {
        let chunk = (dst.len() - filled).min(1 << 25);
        let ret = unsafe {
            syscall3(SYS_GETRANDOM, dst.as_mut_ptr().add(filled) as usize, chunk, 0)
        };
        if ret < 0 {
            let err = io::Error::last_os_error();
            if err.raw_os_error() == Some(4) { continue; }
            return urandom_fallback(&mut dst[filled..]);
        }
        filled += ret as usize;
    }
    Ok(())
}

#[cfg(any(target_os = "macos", target_os = "ios", target_os = "freebsd",
          target_os = "openbsd", target_os = "netbsd"))]
fn bsd_getentropy(dst: &mut [u8]) -> io::Result<()> {
    extern "C" { fn getentropy(buf: *mut u8, len: usize) -> i32; }
    for chunk in dst.chunks_mut(256) {
        let ret = unsafe { getentropy(chunk.as_mut_ptr(), chunk.len()) };
        if ret != 0 { return Err(io::Error::last_os_error()); }
    }
    Ok(())
}

#[cfg(target_os = "windows")]
fn windows_bcrypt(dst: &mut [u8]) -> io::Result<()> {
    #[link(name = "bcrypt")]
    extern "system" {
        fn BCryptGenRandom(h: *mut u8, buf: *mut u8, len: u32, flags: u32) -> i32;
    }
    let ret = unsafe { BCryptGenRandom(std::ptr::null_mut(), dst.as_mut_ptr(), dst.len() as u32, 2) };
    if ret < 0 { return Err(io::Error::new(io::ErrorKind::Other, "BCryptGenRandom failed")); }
    Ok(())
}

#[allow(dead_code)]
fn urandom_fallback(dst: &mut [u8]) -> io::Result<()> {
    use std::io::Read;
    let mut f = std::fs::File::open("/dev/urandom")?;
    f.read_exact(dst)
}

#[cfg(any(target_os = "linux", target_os = "android"))]
type LibcLong = isize;

#[cfg(all(any(target_os = "linux", target_os = "android"), target_arch = "x86_64"))]
unsafe fn syscall3(nr: isize, a: usize, b: usize, c: usize) -> isize {
    let r: isize;
    std::arch::asm!(
        "syscall",
        inlateout("rax") nr => r,
        in("rdi") a, in("rsi") b, in("rdx") c,
        out("rcx") _, out("r11") _,
        options(nostack),
    );
    r
}

#[cfg(all(any(target_os = "linux", target_os = "android"), target_arch = "aarch64"))]
unsafe fn syscall3(nr: isize, a: usize, b: usize, c: usize) -> isize {
    let r: isize;
    std::arch::asm!(
        "svc 0",
        in("x8") nr, inlateout("x0") a => r,
        in("x1") b, in("x2") c,
        options(nostack),
    );
    r
}

#[cfg(all(any(target_os = "linux", target_os = "android"),
          not(any(target_arch = "x86_64", target_arch = "aarch64"))))]
unsafe fn syscall3(_nr: isize, _a: usize, _b: usize, _c: usize) -> isize { -1 }

pub trait Rng {
    fn next_u32(&mut self) -> u32;
    fn next_u64(&mut self) -> u64 {
        (self.next_u32() as u64) | ((self.next_u32() as u64) << 32)
    }
    fn fill_bytes(&mut self, dst: &mut [u8]) {
        let mut i = 0;
        while i + 4 <= dst.len() {
            dst[i..i + 4].copy_from_slice(&self.next_u32().to_le_bytes());
            i += 4;
        }
        let rem = dst.len() - i;
        if rem > 0 {
            let last = self.next_u32().to_le_bytes();
            dst[i..].copy_from_slice(&last[..rem]);
        }
    }
    fn gen_range_u32(&mut self, low: u32, high: u32) -> u32 {
        assert!(low < high, "empty range");
        let span = (high - low) as u64;
        let zone = (u32::MAX as u64 + 1) - ((u32::MAX as u64 + 1) % span);
        loop {
            let v = self.next_u32() as u64;
            if v < zone { return low + (v % span) as u32; }
        }
    }
    fn gen_range_u64(&mut self, low: u64, high: u64) -> u64 {
        assert!(low < high, "empty range");
        let span = high - low;
        let zone = u64::MAX - (u64::MAX % span);
        loop {
            let v = self.next_u64();
            if v < zone { return low + v % span; }
        }
    }
    fn gen_bool(&mut self, p: f64) -> bool {
        assert!((0.0..=1.0).contains(&p));
        self.gen_f64() < p
    }
    fn gen_f64(&mut self) -> f64 {
        let bits = self.next_u64() >> 11;
        bits as f64 * (1.0 / (1u64 << 53) as f64)
    }
}

pub trait SeedableRng: Sized {
    fn from_seed(seed: [u8; 32]) -> Self;
    fn from_os() -> Self {
        let mut seed = [0u8; 32];
        fill_os(&mut seed).expect("OS entropy");
        Self::from_seed(seed)
    }
    fn seed_from_u64(state: u64) -> Self {
        let mut s = [0u8; 32];
        let mut x = state.wrapping_add(0x9E3779B97F4A7C15);
        for c in s.chunks_mut(8) {
            x = (x ^ (x >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
            x = (x ^ (x >> 27)).wrapping_mul(0x94D049BB133111EB);
            let y = x ^ (x >> 31);
            c.copy_from_slice(&y.to_le_bytes()[..c.len()]);
        }
        Self::from_seed(s)
    }
}

const BLOCKS_PER_BUFFER: usize = 8;
const BUF_WORDS: usize = 16 * BLOCKS_PER_BUFFER;

#[derive(Clone)]
pub struct ChaCha20Rng {
    state: [u32; 16],
    buffer: [u32; BUF_WORDS],
    idx: usize,
}

const CHACHA_CONSTANTS: [u32; 4] = [0x61707865, 0x3320646e, 0x79622d32, 0x6b206574];

impl ChaCha20Rng {
    fn refill(&mut self) {
        #[cfg(target_arch = "x86_64")]
        unsafe {
            if std::is_x86_feature_detected!("avx2") {
                self.refill_avx2();
            } else {
                self.refill_sse2(0);
                self.refill_sse2(64);
            }
            return;
        }
        #[cfg(not(target_arch = "x86_64"))]
        self.refill_scalar();
    }

    #[allow(dead_code)]
    fn refill_scalar(&mut self) {
        let (mut c_lo, mut c_hi) = (self.state[12], self.state[13]);
        for b in 0..BLOCKS_PER_BUFFER {
            let mut s = self.state;
            s[12] = c_lo;
            s[13] = c_hi;
            let mut x = s;
            for _ in 0..10 {
                quarter(&mut x, 0, 4, 8, 12);
                quarter(&mut x, 1, 5, 9, 13);
                quarter(&mut x, 2, 6, 10, 14);
                quarter(&mut x, 3, 7, 11, 15);
                quarter(&mut x, 0, 5, 10, 15);
                quarter(&mut x, 1, 6, 11, 12);
                quarter(&mut x, 2, 7, 8, 13);
                quarter(&mut x, 3, 4, 9, 14);
            }
            let off = b * 16;
            for i in 0..16 { self.buffer[off + i] = x[i].wrapping_add(s[i]); }
            let new_lo = c_lo.wrapping_add(1);
            if new_lo == 0 { c_hi = c_hi.wrapping_add(1); }
            c_lo = new_lo;
        }
        self.state[12] = c_lo;
        self.state[13] = c_hi;
        self.idx = 0;
    }

    #[cfg(target_arch = "x86_64")]
    #[target_feature(enable = "sse2")]
    unsafe fn refill_sse2(&mut self, buf_off: usize) {
        use std::arch::x86_64::*;
        let mut clo = [0u32; 4];
        let mut chi = [0u32; 4];
        let mut cur_lo = self.state[12];
        let mut cur_hi = self.state[13];
        for i in 0..4 {
            clo[i] = cur_lo;
            chi[i] = cur_hi;
            let nl = cur_lo.wrapping_add(1);
            if nl == 0 { cur_hi = cur_hi.wrapping_add(1); }
            cur_lo = nl;
        }
        let splat = |i: usize| _mm_set1_epi32(self.state[i] as i32);
        let mut v: [__m128i; 16] = [
            splat(0), splat(1), splat(2), splat(3),
            splat(4), splat(5), splat(6), splat(7),
            splat(8), splat(9), splat(10), splat(11),
            _mm_loadu_si128(clo.as_ptr() as *const __m128i),
            _mm_loadu_si128(chi.as_ptr() as *const __m128i),
            splat(14), splat(15),
        ];
        let initial = v;
        for _ in 0..10 {
            qr_simd(&mut v, 0, 4, 8, 12);
            qr_simd(&mut v, 1, 5, 9, 13);
            qr_simd(&mut v, 2, 6, 10, 14);
            qr_simd(&mut v, 3, 7, 11, 15);
            qr_simd(&mut v, 0, 5, 10, 15);
            qr_simd(&mut v, 1, 6, 11, 12);
            qr_simd(&mut v, 2, 7, 8, 13);
            qr_simd(&mut v, 3, 4, 9, 14);
        }
        for i in 0..16 { v[i] = _mm_add_epi32(v[i], initial[i]); }
        for g in (0..16).step_by(4) {
            let a = v[g]; let b = v[g + 1]; let c = v[g + 2]; let d = v[g + 3];
            let t0 = _mm_unpacklo_epi32(a, b);
            let t1 = _mm_unpackhi_epi32(a, b);
            let t2 = _mm_unpacklo_epi32(c, d);
            let t3 = _mm_unpackhi_epi32(c, d);
            let r0 = _mm_unpacklo_epi64(t0, t2);
            let r1 = _mm_unpackhi_epi64(t0, t2);
            let r2 = _mm_unpacklo_epi64(t1, t3);
            let r3 = _mm_unpackhi_epi64(t1, t3);
            let p = self.buffer.as_mut_ptr().add(buf_off);
            _mm_storeu_si128(p.add(g) as *mut __m128i, r0);
            _mm_storeu_si128(p.add(16 + g) as *mut __m128i, r1);
            _mm_storeu_si128(p.add(32 + g) as *mut __m128i, r2);
            _mm_storeu_si128(p.add(48 + g) as *mut __m128i, r3);
        }
        self.state[12] = cur_lo;
        self.state[13] = cur_hi;
        if buf_off + 64 >= BUF_WORDS { self.idx = 0; }
    }

    #[cfg(target_arch = "x86_64")]
    #[target_feature(enable = "avx2")]
    unsafe fn refill_avx2(&mut self) {
        use std::arch::x86_64::*;
        let mut clo = [0u32; 8];
        let mut chi = [0u32; 8];
        let mut cur_lo = self.state[12];
        let mut cur_hi = self.state[13];
        for i in 0..8 {
            clo[i] = cur_lo;
            chi[i] = cur_hi;
            let nl = cur_lo.wrapping_add(1);
            if nl == 0 { cur_hi = cur_hi.wrapping_add(1); }
            cur_lo = nl;
        }
        let splat = |i: usize| _mm256_set1_epi32(self.state[i] as i32);
        let mut v: [__m256i; 16] = [
            splat(0), splat(1), splat(2), splat(3),
            splat(4), splat(5), splat(6), splat(7),
            splat(8), splat(9), splat(10), splat(11),
            _mm256_loadu_si256(clo.as_ptr() as *const __m256i),
            _mm256_loadu_si256(chi.as_ptr() as *const __m256i),
            splat(14), splat(15),
        ];
        let initial = v;
        for _ in 0..10 {
            qr_avx2(&mut v, 0, 4, 8, 12);
            qr_avx2(&mut v, 1, 5, 9, 13);
            qr_avx2(&mut v, 2, 6, 10, 14);
            qr_avx2(&mut v, 3, 7, 11, 15);
            qr_avx2(&mut v, 0, 5, 10, 15);
            qr_avx2(&mut v, 1, 6, 11, 12);
            qr_avx2(&mut v, 2, 7, 8, 13);
            qr_avx2(&mut v, 3, 4, 9, 14);
        }
        for i in 0..16 { v[i] = _mm256_add_epi32(v[i], initial[i]); }
        let p = self.buffer.as_mut_ptr();
        for g in (0..16).step_by(4) {
            let a = v[g]; let b = v[g + 1]; let c = v[g + 2]; let d = v[g + 3];
            let t0 = _mm256_unpacklo_epi32(a, b);
            let t1 = _mm256_unpackhi_epi32(a, b);
            let t2 = _mm256_unpacklo_epi32(c, d);
            let t3 = _mm256_unpackhi_epi32(c, d);
            let r0 = _mm256_unpacklo_epi64(t0, t2);
            let r1 = _mm256_unpackhi_epi64(t0, t2);
            let r2 = _mm256_unpacklo_epi64(t1, t3);
            let r3 = _mm256_unpackhi_epi64(t1, t3);
            _mm_storeu_si128(p.add(g) as *mut __m128i, _mm256_castsi256_si128(r0));
            _mm_storeu_si128(p.add(16 + g) as *mut __m128i, _mm256_castsi256_si128(r1));
            _mm_storeu_si128(p.add(32 + g) as *mut __m128i, _mm256_castsi256_si128(r2));
            _mm_storeu_si128(p.add(48 + g) as *mut __m128i, _mm256_castsi256_si128(r3));
            _mm_storeu_si128(p.add(64 + g) as *mut __m128i, _mm256_extracti128_si256::<1>(r0));
            _mm_storeu_si128(p.add(80 + g) as *mut __m128i, _mm256_extracti128_si256::<1>(r1));
            _mm_storeu_si128(p.add(96 + g) as *mut __m128i, _mm256_extracti128_si256::<1>(r2));
            _mm_storeu_si128(p.add(112 + g) as *mut __m128i, _mm256_extracti128_si256::<1>(r3));
        }
        self.state[12] = cur_lo;
        self.state[13] = cur_hi;
        self.idx = 0;
    }

    pub fn set_stream(&mut self, stream: u64) {
        self.state[14] = stream as u32;
        self.state[15] = (stream >> 32) as u32;
        self.idx = BUF_WORDS;
    }

    pub fn set_word_pos(&mut self, pos: u128) {
        let block = (pos >> 4) as u64;
        let group = block & !(BLOCKS_PER_BUFFER as u64 - 1);
        self.state[12] = group as u32;
        self.state[13] = (group >> 32) as u32;
        self.refill();
        self.idx = ((block - group) as usize) * 16 + (pos & 0xF) as usize;
    }
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "sse2")]
unsafe fn qr_simd(v: &mut [std::arch::x86_64::__m128i; 16],
                  a: usize, b: usize, c: usize, d: usize) {
    use std::arch::x86_64::*;
    macro_rules! rotl {
        ($x:expr, $n:literal, $m:literal) => {
            _mm_or_si128(_mm_slli_epi32::<$n>($x), _mm_srli_epi32::<$m>($x))
        };
    }
    v[a] = _mm_add_epi32(v[a], v[b]);
    v[d] = rotl!(_mm_xor_si128(v[d], v[a]), 16, 16);
    v[c] = _mm_add_epi32(v[c], v[d]);
    v[b] = rotl!(_mm_xor_si128(v[b], v[c]), 12, 20);
    v[a] = _mm_add_epi32(v[a], v[b]);
    v[d] = rotl!(_mm_xor_si128(v[d], v[a]), 8, 24);
    v[c] = _mm_add_epi32(v[c], v[d]);
    v[b] = rotl!(_mm_xor_si128(v[b], v[c]), 7, 25);
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "avx2")]
unsafe fn qr_avx2(v: &mut [std::arch::x86_64::__m256i; 16],
                  a: usize, b: usize, c: usize, d: usize) {
    use std::arch::x86_64::*;
    macro_rules! rotl {
        ($x:expr, $n:literal, $m:literal) => {
            _mm256_or_si256(_mm256_slli_epi32::<$n>($x), _mm256_srli_epi32::<$m>($x))
        };
    }
    v[a] = _mm256_add_epi32(v[a], v[b]);
    v[d] = rotl!(_mm256_xor_si256(v[d], v[a]), 16, 16);
    v[c] = _mm256_add_epi32(v[c], v[d]);
    v[b] = rotl!(_mm256_xor_si256(v[b], v[c]), 12, 20);
    v[a] = _mm256_add_epi32(v[a], v[b]);
    v[d] = rotl!(_mm256_xor_si256(v[d], v[a]), 8, 24);
    v[c] = _mm256_add_epi32(v[c], v[d]);
    v[b] = rotl!(_mm256_xor_si256(v[b], v[c]), 7, 25);
}

fn quarter(x: &mut [u32; 16], a: usize, b: usize, c: usize, d: usize) {
    x[a] = x[a].wrapping_add(x[b]); x[d] = (x[d] ^ x[a]).rotate_left(16);
    x[c] = x[c].wrapping_add(x[d]); x[b] = (x[b] ^ x[c]).rotate_left(12);
    x[a] = x[a].wrapping_add(x[b]); x[d] = (x[d] ^ x[a]).rotate_left(8);
    x[c] = x[c].wrapping_add(x[d]); x[b] = (x[b] ^ x[c]).rotate_left(7);
}

impl SeedableRng for ChaCha20Rng {
    fn from_seed(seed: [u8; 32]) -> Self {
        let mut state = [0u32; 16];
        state[..4].copy_from_slice(&CHACHA_CONSTANTS);
        for i in 0..8 {
            state[4 + i] = u32::from_le_bytes(seed[i * 4..i * 4 + 4].try_into().unwrap());
        }
        ChaCha20Rng { state, buffer: [0; BUF_WORDS], idx: BUF_WORDS }
    }
}

impl Rng for ChaCha20Rng {
    fn next_u32(&mut self) -> u32 {
        if self.idx >= BUF_WORDS { self.refill(); }
        let v = self.buffer[self.idx];
        self.idx += 1;
        v
    }
    fn fill_bytes(&mut self, dst: &mut [u8]) {
        let mut written = 0;
        let whole = dst.len() & !3;
        while written < whole {
            if self.idx >= BUF_WORDS { self.refill(); }
            let words = ((whole - written) / 4).min(BUF_WORDS - self.idx);
            for k in 0..words {
                let off = written + k * 4;
                dst[off..off + 4].copy_from_slice(&self.buffer[self.idx + k].to_le_bytes());
            }
            self.idx += words;
            written += words * 4;
        }
        let rem = dst.len() - written;
        if rem > 0 {
            if self.idx >= BUF_WORDS { self.refill(); }
            let last = self.buffer[self.idx].to_le_bytes();
            self.idx += 1;
            dst[written..].copy_from_slice(&last[..rem]);
        }
    }
}

pub struct ThreadRng { inner: Rc<UnsafeCell<ChaCha20Rng>> }

thread_local! {
    static THREAD_RNG: Rc<UnsafeCell<ChaCha20Rng>> =
        Rc::new(UnsafeCell::new(ChaCha20Rng::from_os()));
}

pub fn thread_rng() -> ThreadRng {
    ThreadRng { inner: THREAD_RNG.with(|r| r.clone()) }
}

impl Rng for ThreadRng {
    fn next_u32(&mut self) -> u32 { unsafe { (*self.inner.get()).next_u32() } }
    fn next_u64(&mut self) -> u64 { unsafe { (*self.inner.get()).next_u64() } }
    fn fill_bytes(&mut self, d: &mut [u8]) { unsafe { (*self.inner.get()).fill_bytes(d) } }
}

pub fn random_u32() -> u32 { thread_rng().next_u32() }
pub fn random_u64() -> u64 { thread_rng().next_u64() }
pub fn random_bytes(dst: &mut [u8]) { thread_rng().fill_bytes(dst) }

pub trait Distribution<T> {
    fn sample<R: Rng + ?Sized>(&self, rng: &mut R) -> T;
}

pub struct Standard;

impl Distribution<u32> for Standard {
    fn sample<R: Rng + ?Sized>(&self, r: &mut R) -> u32 { r.next_u32() }
}
impl Distribution<u64> for Standard {
    fn sample<R: Rng + ?Sized>(&self, r: &mut R) -> u64 { r.next_u64() }
}
impl Distribution<f64> for Standard {
    fn sample<R: Rng + ?Sized>(&self, r: &mut R) -> f64 { r.gen_f64() }
}
impl Distribution<bool> for Standard {
    fn sample<R: Rng + ?Sized>(&self, r: &mut R) -> bool { r.next_u32() & 1 == 1 }
}

pub struct UniformU32 { low: u32, span: u32 }
impl UniformU32 {
    pub fn new(low: u32, high: u32) -> Self {
        assert!(low < high, "empty range");
        UniformU32 { low, span: high - low }
    }
}
impl Distribution<u32> for UniformU32 {
    fn sample<R: Rng + ?Sized>(&self, r: &mut R) -> u32 {
        self.low + r.gen_range_u32(0, self.span)
    }
}

pub struct UniformU64 { low: u64, span: u64 }
impl UniformU64 {
    pub fn new(low: u64, high: u64) -> Self {
        assert!(low < high, "empty range");
        UniformU64 { low, span: high - low }
    }
}
impl Distribution<u64> for UniformU64 {
    fn sample<R: Rng + ?Sized>(&self, r: &mut R) -> u64 {
        self.low + r.gen_range_u64(0, self.span)
    }
}

pub struct UniformF64 { low: f64, span: f64 }
impl UniformF64 {
    pub fn new(low: f64, high: f64) -> Self {
        assert!(low < high && high.is_finite() && low.is_finite(), "bad range");
        UniformF64 { low, span: high - low }
    }
}
impl Distribution<f64> for UniformF64 {
    fn sample<R: Rng + ?Sized>(&self, r: &mut R) -> f64 {
        self.low + self.span * r.gen_f64()
    }
}

#[derive(Debug)]
pub struct WeightedError(pub &'static str);

impl std::fmt::Display for WeightedError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result { f.write_str(self.0) }
}
impl std::error::Error for WeightedError {}

pub struct WeightedIndex {
    cumulative: Vec<f64>,
    total: f64,
}

impl WeightedIndex {
    pub fn new<I: IntoIterator<Item = f64>>(weights: I) -> Result<Self, WeightedError> {
        let mut cumulative = Vec::new();
        let mut total = 0.0;
        for w in weights {
            if !(w.is_finite() && w >= 0.0) { return Err(WeightedError("negative/NaN weight")); }
            total += w;
            cumulative.push(total);
        }
        if cumulative.is_empty() { return Err(WeightedError("empty weights")); }
        if total <= 0.0 { return Err(WeightedError("zero total weight")); }
        Ok(WeightedIndex { cumulative, total })
    }
}

impl Distribution<usize> for WeightedIndex {
    fn sample<R: Rng + ?Sized>(&self, r: &mut R) -> usize {
        let target = r.gen_f64() * self.total;
        self.cumulative.partition_point(|&c| c <= target).min(self.cumulative.len() - 1)
    }
}

pub trait SliceRandom {
    type Item;
    fn shuffle<R: Rng + ?Sized>(&mut self, rng: &mut R);
    fn partial_shuffle<R: Rng + ?Sized>(&mut self, rng: &mut R, amount: usize);
    fn choose<R: Rng + ?Sized>(&self, rng: &mut R) -> Option<&Self::Item>;
    fn choose_mut<R: Rng + ?Sized>(&mut self, rng: &mut R) -> Option<&mut Self::Item>;
}

impl<T> SliceRandom for [T] {
    type Item = T;
    fn shuffle<R: Rng + ?Sized>(&mut self, rng: &mut R) {
        let mut i = self.len();
        while i > 1 {
            i -= 1;
            let j = rng.gen_range_u64(0, (i + 1) as u64) as usize;
            self.swap(i, j);
        }
    }
    fn partial_shuffle<R: Rng + ?Sized>(&mut self, rng: &mut R, amount: usize) {
        let end = amount.min(self.len());
        for i in 0..end {
            let j = i + rng.gen_range_u64(0, (self.len() - i) as u64) as usize;
            self.swap(i, j);
        }
    }
    fn choose<R: Rng + ?Sized>(&self, rng: &mut R) -> Option<&T> {
        if self.is_empty() { return None; }
        let i = rng.gen_range_u64(0, self.len() as u64) as usize;
        Some(&self[i])
    }
    fn choose_mut<R: Rng + ?Sized>(&mut self, rng: &mut R) -> Option<&mut T> {
        if self.is_empty() { return None; }
        let i = rng.gen_range_u64(0, self.len() as u64) as usize;
        Some(&mut self[i])
    }
}
