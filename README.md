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
