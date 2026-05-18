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
