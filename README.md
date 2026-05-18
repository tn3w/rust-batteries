# rust-batteries
Single-file Rust implementations of common 'should-be-std' functionality (regex, JSON, serde, ryu…). Drop into your project, no dependencies, ≤1000 lines each.

## regex.rs (796 LOC, safe, no deps)

Linear-time. Pike VM + lazy DFA + literal prefilters.

```rust
let re = Regex::new(r"\d+")?;
re.is_match("answer 42");   // bool
re.find("answer 42");       // Option<(start, end)>
re.captures("answer 42");   // Option<Vec<Option<usize>>>
```

Syntax: `. * + ? | ( ) [a-z] [^a-z] ^ $`, escapes `\d \D \w \W \s \S \n \t \r`. ASCII bytes only. No `{n,m}`, backrefs, or lookaround. Captures: group 0 = whole match.

Engine: Thompson NFA bytecode, unanchored via `.*?` wrap. Lazy DFA over u128 PC bitsets when program ≤128 insts and no `$`, else Pike VM. Prefilters: literal prefix (BMH), required-literal-anywhere (skip engine on absent), first-byte set, pure-literal-alternation multi-scan. SWAR memchr/2/3 (8 bytes/op).

Perf vs `regex` crate over 20 patterns: geomean 1.17x, total time 0.74x (we win on cheap patterns, ~3x gap on rare-absent-literal where regex uses unsafe SIMD).

Limits: programs >128 insts or `$` fall back to Pike VM (still linear).

## serde.rs (548 LOC, safe, no deps)

Format-agnostic Serialize/Deserialize traits + declarative derive. Functional parity with `serde` + `serde_derive` for owned and borrowed data, structs, enums, and field/variant rename.

```rust
use serde::*;
serde_struct! {
    pub struct Config {
        pub host: String,
        pub api_key as "api-key": String,   // rename
        pub plain: bool,
    }
}
serde_struct! {
    pub struct Borrowed<'a> { pub name: &'a str }  // zero-copy
}
serde_enum! {
    pub enum Status {
        Active,                                // unit → "Active"
        Failed as "failed",                    // variant rename
        Retry(u32),                            // newtype → {"Retry": 3}
        Error { code: i32, msg: String },      // struct → {"Error": {...}}
    }
}
```

Traits: `Serialize`, `Deserialize<'de>`, `Serializer`, `Deserializer<'de>`. Built-in impls: bool, i8..i64/isize, u8..u64/usize, f32, f64, String, `&'de str`, `Cow<'de, str>`, Option, Box, Vec, [T], BTreeMap/HashMap<String,V>, tuples up to 6. Macros: `serde_struct!` (named fields, optional `<'lt>`, `as "rename"` per field); `serde_enum!` (externally-tagged: unit/newtype/struct variants, `as "rename"` per variant or struct field). Unknown fields/variants skip; missing required fields error.

`num_kind()` extension reports I64/U64/F64 without consuming → polymorphic targets (e.g. `json::Value`) keep number precision. `read_str_borrowed()` returns `&'de str` slice when the source has no escapes, errors otherwise; `read_str_cow()` borrows when possible, allocates on escapes.

Output matches `serde`'s default tagged representation for structs and externally-tagged enums.

## ryu.rs (315 LOC, safe, no deps)

Shortest round-trip f64-to-string. Dragon4 with stack-allocated BigInts (24×u64 = 1536 bits, covers full f64 range including subnormals down to 5e-324).

```rust
ryu::f64_to_string(3.14);              // "3.14"
ryu::write_f64(&mut buf, 1.5e-10);     // "1.5e-10"
```

Algorithm: Burger-Dybvig Dragon4. Boundary-asymmetric for `m == 2^52` normals. Round-half-to-even ties. k estimated via `log10`, fix-up via single ×10 step. Format matches `serde_json`/ryu rules: scientific when scientific-exp `< -5` or `>= 16`, signed exponent (`e+16`, `e-10`), else decimal. Integral floats get `.0` suffix.

Perf: ~10-20x slower than `ryu` crate (no table lookup; pays for BigInt allocs on every call). Acceptable for JSON where floats are a fraction of total bytes; on float-heavy payloads expect 4-5x serde_json.

## json.rs (795 LOC, safe, no deps)

`serde_json::Value` parity. Linear-time parse, BTreeMap objects (alphabetical output matches serde_json default). Implements `serde::Serialize`/`Deserialize<'de>` (above) for typed round-trips including zero-copy `&'de str`. Float output via `ryu.rs` above.

```rust
let v = json::from_str(r#"{"id":42,"name":"alice"}"#)?;
v["name"].as_str();           // Some("alice")
v["id"].as_i64();             // Some(42)
json::to_string(&v);          // {"id":42,"name":"alice"}
json::to_string_pretty(&v);
```

Types: `Null Bool Number(PosInt|NegInt|Float) String Array Object`. Index by `&str` or `usize` returns `Null` on miss. Accessors `as_str as_bool as_i64 as_u64 as_f64 as_array as_object get is_null`.

Parser: hand-rolled byte recursive descent. Strings: chunked fast scan that copies plain runs in one push, escape path handles `\" \\ \/ \b \f \n \r \t \uXXXX` with surrogate pairs. Numbers: branch by integer/float, parse via std. Line/column computed lazily on error. Pre-allocated output buffer based on Value size estimate.

Perf vs `serde_json` over 10 inputs (small to big-array): parse total 1.03x (essentially matches serde_json). Serialize total 2.14x bounded by float formatting (Dragon4 BigInt vs serde_json's table-based ryu). Typed struct: 1.43x parse, 4.40x serialize vs `serde_json` + `serde_derive`. f64 shortest-round-trip matches ryu output (decimal vs scientific notation, signed exponent).

Limits: No streaming.

## csv.rs (216 LOC, safe, no deps)

RFC 4180 reader + writer with parity vs `csv` crate (headerless mode).

```rust
let recs = csv::parse_str("a,b\n\"c,d\",\"e\"\"f\"\n")?;
csv::to_string(&recs);
let mut r = csv::Reader::new(input).delimiter(b';');
for rec in r { let row = rec?; /* Cow<str> per field */ }
```

Reader returns `Vec<Cow<'a, str>>` borrows unquoted/escapeless fields directly from input, allocates only on `""` escapes. Skips blank lines (matches `csv` crate `has_headers(false)`). Configurable delimiter and quote byte. Writer auto-quotes when field contains delimiter, quote, `\n`, or `\r`.

Perf vs `csv` crate over 5 inputs (small, medium, quoted, wide, realistic): parse total 1.03x (essentially matches), write 1.13x. Wins on small + quoted (Cow borrow path); loses on wide-many-fields (50 columns) where csv-core's SIMD field-scanner pulls ahead.

## toml.rs (987 LOC, safe, no deps)

TOML v1.0 parser + serializer with parity vs `toml` crate's `Value`. BTreeMap tables → canonical alphabetical output. `Value` implements `serde::Serialize`/`Deserialize<'de>` (above) → round-trip through any format.

```rust
let v = toml::from_str("[server]\nport = 8080\n")?;
toml::to_string(&v);
```

Types: `String Integer(i64) Float(f64) Boolean Datetime(String) Array Table`. Datetimes stored verbatim as RFC 3339 string.

Syntax: comments `#`, bare/quoted/dotted keys, basic+literal strings, multi-line `"""..."""` and `'''...'''` (with `\<newline>` line-ending continuation), integers (decimal/hex `0x`/oct `0o`/bin `0b`, `_` separators), floats (`inf nan` ±, exp), booleans, `[hdr]` sections, `[[hdr]]` arrays of tables, inline tables `{ k = v }`, mixed-type arrays. Duplicate table definitions rejected; implicit-vs-explicit table state tracked.

Parser: byte recursive descent. Header path navigates `Array(Table)` → last-element on each `[[hdr]]`. Dotted keys open intermediate implicit tables. Token-based value scan handles datetime detection via positional digit/colon/dash checks. Numbers: branch by `0x/0o/0b` prefix, else stdlib parse with `_` stripped.

Perf vs `toml` crate over 5 inputs (small kv, nested, arrays of tables, strings, realistic): parse total 0.63x, serialize total 0.10x. Wins on serialize because no `Display` formatting machinery.

Limits: Datetimes deserialize as `String` (serde trait has no datetime kind).

## hex.rs (59 LOC, safe, no deps)

Hex encode/decode with parity vs `hex` crate.

```rust
hex::encode(b"\xde\xad\xbe\xef");     // "deadbeef"
hex::encode_upper(b"\xff");           // "FF"
hex::decode("DeAdBeEf")?;             // mixed-case accepted
```

Encode: lookup-table nibble→ASCII, 2 bytes out per input byte, `unsafe` UTF-8 wrap (output ASCII by construction). Decode: lower/upper/digit branch per nibble, rejects odd length and non-hex.

Perf vs `hex` crate over 16B/256B/4KiB: encode 0.24x (4x faster, no SIMD on either side; `hex` allocates+formats), decode 1.13x (within noise; both scalar).

## base64.rs (142 LOC, safe, no deps)

Standard + URL-safe alphabets, parity vs `base64` crate (`STANDARD` / `URL_SAFE_NO_PAD` engines).

```rust
base64::encode(b"hello");             // "aGVsbG8="
base64::decode("aGVsbG8=")?;          // padded, strict
base64::encode_url(b"hello");         // "aGVsbG8" (no pad)
base64::decode_url("aGVsbG8")?;       // rejects `=`
```

Encode: 3-byte chunks → 4 chars via 64-entry table; trailing 1/2 bytes padded with `=` (standard) or unpadded (URL). Decode: 256-entry signed lookup (`-1` sentinel = invalid), validates trailing-bit zeros on partial groups. `decode` requires length % 4 == 0; `decode_url` rejects any `=`.

Perf vs `base64` crate (16B/256B/4KiB): encode 2.13x, decode 1.78x. `base64` crate has SIMD-accelerated decode/encode paths; this is scalar.

## anyhow.rs (199 LOC, safe, no deps)

App-error type with context chaining; parity vs `anyhow` crate.

```rust
use anyhow::{Result, Context, anyhow, bail, ensure};

fn load(path: &str) -> Result<String> {
    std::fs::read_to_string(path).context("read config")?;
    ensure!(path.ends_with(".toml"), "need .toml: {}", path);
    bail!("not implemented");
}
let e = anyhow!("oops {}", 42);
for cause in e.chain() { /* contexts then source chain */ }
```

`Error` = `Box<dyn StdError + Send + Sync>` + `Vec<String>` context stack. Blanket `From<E: StdError + Send + Sync + 'static>` → `?` works on any std error. `Context` trait extends `Result` and `Option` with `.context(msg)` / `.with_context(|| msg)`. `Display` shows top context; `Debug` shows top + `Caused by:` numbered chain (matches anyhow output exactly). Macros: `anyhow!` (msg/format/wrap), `bail!` (early return), `ensure!` (guard).

Perf vs `anyhow` 1.0 (construct+display, result-chain, debug full-chain): total 0.65x. Wins because no specialized backtrace machinery / single-word repr trickery.

## thiserror.rs (159 LOC, safe, no deps)

Declarative `thiserror!` macro generating `Display` + `Error` + `From` for enum errors. Functional parity vs `thiserror` derive (no proc-macro dep).

```rust
thiserror! {
    pub enum MyError {
        "io error: {0}" Io(from std::io::Error),       // #[from] → From + source
        "wrapped: {0}" Wrapped(source std::io::Error), // #[source] only
        "not found" NotFound,
        "bad {code}: {msg}" Bad { code: i32, msg: String },
        "wrap {code}: {msg}" Wrap { code: i32, msg: String, source: std::io::Error },
        "pair {0}/{1}" Pair(i32, String),
        "six {0}/{1}/{2}/{3}/{4}/{5}" Six(u8, u8, u8, u8, u8, u8),
        transparent Trans(std::io::Error),             // #[error(transparent)]
    }
}
```

Variant syntax: format-string literal prefixes each variant (replaces `#[error("…")]`). Tuple keywords in field position `from` (auto-`From<T>` + source), `source` (source only), `transparent` (forward `Display` + source to inner). Tuple variants 1–16 fields (ident pool); named variants any count, and a field literally named `source` auto-wires `Error::source()`. Named uses Rust 2021 implicit captures (`{code}` → bound field). Tuple uses `{0}`/`{1}`/…

Perf vs `thiserror` 1.0 derive (unit/named/from+Display): total 0.99x matches the proc-macro output (Display goes through the same `write!` codegen).

Limits: no `Backtrace` capture (`Error::provide` is nightly-only `error_generic_member_access`).

## rand.rs (592 LOC, safe core + `unsafe` for syscalls/SIMD, no deps)

ChaCha20 CSPRNG + OS entropy + distributions + slice helpers. Bit-exact stream parity with `rand_chacha::ChaCha20Rng`.

```rust
let mut r = ChaCha20Rng::from_os();
r.next_u64(); r.gen_range_u32(0, 100); r.gen_f64();
UniformF64::new(-1.0, 1.0).sample(&mut r);
WeightedIndex::new([1.0, 2.0, 7.0])?.sample(&mut r);
let mut v = vec![1, 2, 3, 4]; v.shuffle(&mut r); v.choose(&mut r);
thread_rng().fill_bytes(&mut buf);  fill_os(&mut buf)?;
```

`Rng SeedableRng Distribution<T> SliceRandom`. Distributions: `Standard UniformU32/64 UniformF64 WeightedIndex` (cumulative + `partition_point`). Range: unbiased rejection. Shuffle: Fisher-Yates. Seed: `from_seed from_os seed_from_u64`(SplitMix64).

ChaCha: 20-round, 64-bit ctr + 64-bit stream (rand_chacha layout). x86_64 runtime dispatch: AVX2 8-way (`[__m256i; 16]`, 8 blocks/refill, 4×4 transpose per 128-bit lane + `extracti128` for high half) → SSE2 4-way (`[__m128i; 16]`, ×2 to fill buffer) → scalar. 8-block (512-byte) buffer. `fill_bytes` bulk-copies words via `to_le_bytes`.

OS entropy: Linux → direct `getrandom(2)` syscall (inline asm x86_64/aarch64, `/dev/urandom` fallback on ENOSYS); *BSD/macOS → `getentropy(3)`; Windows → `BCryptGenRandom`; else `/dev/urandom`.

Perf vs `rand_chacha` 0.3 + `getrandom` 0.2 (AVX2 host): `next_u64` 0.74x, `fill_bytes` 1KiB 0.98x, `gen_range_u32` 0.74x, OS entropy 0.99x matches or beats across the board.

## uuid.rs (205 LOC, safe core + `unsafe` UTF-8 wrap, no deps)

UUID v4 + v7 with parity vs `uuid` crate.

```rust
let id = Uuid::new_v4();              // random
let id = Uuid::now_v7();              // ms-timestamp + random
let parsed: Uuid = "550e8400-e29b-41d4-a716-446655440000".parse()?;
let s = id.to_string();                // hyphenated lowercase
id.simple().to_string();               // 32 chars no hyphens
id.braced().to_string();               // {…}
id.urn().to_string();                  // urn:uuid:…
Uuid::nil(); Uuid::max();
id.as_bytes(); id.into_bytes();
```

Layout: `[u8; 16]` big-endian. v4 sets version=4 + RFC 4122 variant; v7 packs 48-bit Unix-ms timestamp into bytes 0–5 then 74 random bits + version/variant. Parser accepts hyphenated (36), simple (32), `{…}` braced (38), `urn:uuid:` prefix; rejects other lengths or non-hex.

RNG: thread-local 1 KiB buffer refilled from `/dev/urandom` (no `getrandom` syscall fancy stuff; Linux/Unix only).

Perf vs `uuid` 1.x (`v4`+`v7` features): total 0.98x — `new_v4` 1.31x (uuid uses `getrandom` syscall direct), `now_v7` 0.79x, parse 0.79–0.93x, display 1.13x.

## dotenvy.rs (258 LOC, safe, no deps)

`.env` parser + loader, parity vs `dotenvy` crate's `Iter`.

```rust
dotenvy::dotenv()?;                   // walk up for .env, set unset vars
dotenvy::dotenv_override()?;          // overwrite existing
dotenvy::from_path("/etc/app.env")?;
dotenvy::from_filename(".env.local")?;
let pairs = dotenvy::parse("KEY=value\nFOO=\"$KEY-bar\"\n")?;
let pairs = dotenvy::from_read(reader)?;
```

Syntax: `KEY=VALUE` (alnum/`_`/`.` in keys); optional `export ` prefix; `#` line/inline comments; blank lines; double-quoted strings with `\n \r \t \\ \" \' \$` escapes and `$VAR`/`${VAR}` interpolation; single-quoted literals (no escapes, no interpolation); bare values trimmed, terminated by newline or `#`. Variable lookup: already-parsed keys first, then process env. `find_up` walks parent dirs.

Parser: byte recursive descent, single pass; `Vec<(String, String)>` output preserves insertion order.

Perf vs `dotenvy` 0.15 (3 inputs: small/quoted/big-200-keys): total 0.39x — ~2.6× faster across the board (dotenvy goes through a state machine + per-line allocations; this is one linear walk).
