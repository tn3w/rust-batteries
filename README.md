# rust-batteries
Single-file Rust implementations of common 'should-be-std' functionality (regex, JSON, UUID, base64…). Drop into your project, no dependencies, ≤1000 lines each.

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

## json.rs (490 LOC, safe, no deps)

`serde_json::Value` parity. Linear-time parse, BTreeMap objects (alphabetical output matches serde_json default).

```rust
let v = json::from_str(r#"{"id":42,"name":"alice"}"#)?;
v["name"].as_str();           // Some("alice")
v["id"].as_i64();             // Some(42)
json::to_string(&v);          // {"id":42,"name":"alice"}
json::to_string_pretty(&v);
```

Types: `Null Bool Number(PosInt|NegInt|Float) String Array Object`. Index by `&str` or `usize` returns `Null` on miss. Accessors `as_str as_bool as_i64 as_u64 as_f64 as_array as_object get is_null`.

Parser: hand-rolled byte recursive descent. Strings: chunked fast scan that copies plain runs in one push, escape path handles `\" \\ \/ \b \f \n \r \t \uXXXX` with surrogate pairs. Numbers: branch by integer/float, parse via std. Line/column computed lazily on error. Pre-allocated output buffer based on Value size estimate.

Perf vs `serde_json` over 10 inputs (small to big-array): parse total 1.03x (string array 0.85x, nested 0.86x faster). Serialize total 2.04x bounded by float formatting (serde_json uses ryu; we use std Display with whole-number fast path).

Limits: not a serde `Deserialize` target. No streaming. f64 round-trip may differ from ryu in shortest-representation edge cases.
