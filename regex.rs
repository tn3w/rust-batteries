use std::cell::RefCell;
use std::collections::HashMap;
use std::mem;

#[derive(Clone)]
enum Inst {
    Byte(u8),
    Any,
    Class(Box<[u8; 32]>, bool),
    Match,
    Jmp(u32),
    Split(u32, u32),
    Save(u32),
    End,
}

enum Ast {
    Byte(u8),
    Any,
    Class(Box<[u8; 32]>, bool),
    Concat(Vec<Ast>),
    Alt(Vec<Ast>),
    Star(Box<Ast>),
    Plus(Box<Ast>),
    Opt(Box<Ast>),
    Group(Box<Ast>, usize),
    End,
}

pub struct Regex {
    code: Vec<Inst>,
    saves: usize,
    prefix: Vec<u8>,
    bmh_shift: Option<[u8; 256]>,
    required: Vec<u8>,
    req_shift: Option<[u8; 256]>,
    first_bytes: Option<FirstBytes>,
    literal_alts: Option<LiteralAlts>,
    anchored: bool,
    eclose: Vec<EClose>,
    start_state: EClose,
    use_bitset: bool,
    dfa: RefCell<Dfa>,
}

const ACCEPT_BIT: u32 = 0x8000_0000;
const ID_MASK: u32 = 0x7FFF_FFFF;
const UNKNOWN: u32 = u32::MAX;

struct Dfa {
    set_to_id: HashMap<u128, u32>,
    sets: Vec<u128>,
    trans: Vec<[u32; 256]>,
    start_accept: bool,
    disabled: bool,
}

enum FirstBytes {
    Two(u8, u8),
    Three(u8, u8, u8),
    Set(Box<[bool; 256]>),
}

struct LiteralAlts {
    literals: Vec<Vec<u8>>,
    first_bytes: FirstBytes,
}

impl Dfa {
    fn new() -> Self {
        Dfa { set_to_id: HashMap::new(), sets: Vec::new(), trans: Vec::new(), start_accept: false, disabled: false }
    }
    fn intern(&mut self, set: u128) -> u32 {
        if let Some(&id) = self.set_to_id.get(&set) { return id; }
        let id = self.sets.len() as u32;
        self.sets.push(set);
        self.set_to_id.insert(set, id);
        self.trans.push([UNKNOWN; 256]);
        id
    }
}

#[derive(Clone, Copy, Default)]
struct EClose {
    set: u128,
    accept: bool,
}

impl Regex {
    pub fn new(pattern: &str) -> Result<Self, String> {
        let mut parser = Parser::new(pattern.as_bytes());
        let anchored = parser.peek() == Some(b'^');
        if anchored { parser.bump(); }
        let ast = parser.parse_alt()?;
        if parser.pos < parser.src.len() {
            return Err(format!("trailing input at {}", parser.pos));
        }

        let mut code: Vec<Inst> = Vec::new();
        let body_start: u32;
        if anchored {
            body_start = 0;
        } else {
            code.push(Inst::Split(0, 0));
            code.push(Inst::Any);
            code.push(Inst::Jmp(0));
            body_start = code.len() as u32;
            if let Inst::Split(a, b) = &mut code[0] { *a = body_start; *b = 1; }
        }
        code.push(Inst::Save(0));
        compile(&ast, &mut code);
        code.push(Inst::Save(1));
        code.push(Inst::Match);

        let prefix = extract_prefix(&code[body_start as usize..]);
        let bmh_shift = if prefix.len() >= 2 { Some(build_bmh_shift(&prefix)) } else { None };
        let required = required_literal(&ast);
        let req_shift = if required.len() >= 2 && required.len() > prefix.len() {
            Some(build_bmh_shift(&required))
        } else { None };
        let has_end = code.iter().any(|i| matches!(i, Inst::End));
        let use_bitset = code.len() <= 128 && !has_end;
        let first_bytes = if prefix.is_empty() && required.is_empty() && !anchored {
            build_first_bytes(&ast)
        } else { None };
        let literal_alts = if !anchored { build_literal_alts(&ast) } else { None };
        let mut re = Regex {
            code, saves: (parser.groups + 1) * 2,
            prefix, bmh_shift, required, req_shift, first_bytes, literal_alts, anchored,
            eclose: Vec::new(),
            start_state: EClose::default(),
            use_bitset,
            dfa: RefCell::new(Dfa::new()),
        };
        if re.use_bitset {
            re.build_eclose();
            let mut dfa = re.dfa.borrow_mut();
            dfa.start_accept = re.start_state.accept;
            dfa.intern(re.start_state.set);
        }
        Ok(re)
    }

    fn build_eclose(&mut self) {
        let n = self.code.len();
        self.eclose = vec![EClose::default(); n + 1];
        for pc in 0..n {
            let mut ec = EClose::default();
            self.compute_eclose(pc as u32, &mut ec, &mut 0);
            self.eclose[pc] = ec;
        }
        self.start_state = self.eclose[0];
    }

    fn compute_eclose(&self, pc: u32, ec: &mut EClose, visited: &mut u128) {
        if (*visited >> pc) & 1 == 1 { return; }
        *visited |= 1u128 << pc;
        match &self.code[pc as usize] {
            Inst::Jmp(t) => self.compute_eclose(*t, ec, visited),
            Inst::Split(a, b) => {
                self.compute_eclose(*a, ec, visited);
                self.compute_eclose(*b, ec, visited);
            }
            Inst::Save(_) => self.compute_eclose(pc + 1, ec, visited),
            Inst::Match => ec.accept = true,
            Inst::End => {}
            _ => ec.set |= 1u128 << pc,
        }
    }

    pub fn is_match(&self, text: &str) -> bool {
        let bytes = text.as_bytes();
        if let Some(alts) = &self.literal_alts {
            return match_literal_alts(bytes, alts).is_some();
        }
        if !self.required_present(bytes) { return false; }
        if self.use_bitset { self.is_match_dfa(bytes) } else { self.find_slow(bytes).is_some() }
    }

    fn required_present(&self, input: &[u8]) -> bool {
        if let Some(fb) = &self.first_bytes {
            if first_byte_present(input, fb).is_none() { return false; }
        }
        if self.required.len() <= self.prefix.len() { return true; }
        if self.required.len() == 1 {
            return memchr_u8(input, self.required[0]).is_some();
        }
        if let Some(shift) = &self.req_shift {
            memmem_bmh(input, &self.required, shift).is_some()
        } else {
            memmem(input, &self.required).is_some()
        }
    }

    fn is_match_dfa(&self, input: &[u8]) -> bool {
        let start_pos = match self.find_start(input, 0) { Some(s) => s, None => return false };
        let mut dfa = self.dfa.borrow_mut();
        if dfa.start_accept { return true; }
        if dfa.disabled { drop(dfa); return self.is_match_bitset(input); }
        let mut state: u32 = 0;
        let mut pos = start_pos;
        while pos < input.len() {
            let byte = input[pos];
            let entry = dfa.trans[state as usize][byte as usize];
            let next = if entry != UNKNOWN {
                if entry & ACCEPT_BIT != 0 { return true; }
                entry & ID_MASK
            } else {
                let (n, accept) = step_dfa(&self.code, &self.eclose, &mut dfa, state, byte);
                let packed = n | if accept { ACCEPT_BIT } else { 0 };
                dfa.trans[state as usize][byte as usize] = packed;
                if accept { return true; }
                n
            };
            if dfa.sets[next as usize] == 0 { return false; }
            state = next;
            pos += 1;
        }
        false
    }

    pub fn find(&self, text: &str) -> Option<(usize, usize)> {
        let caps = self.find_slow(text.as_bytes())?;
        Some((caps[0]?, caps[1]?))
    }

    pub fn captures(&self, text: &str) -> Option<Vec<Option<usize>>> {
        self.find_slow(text.as_bytes())
    }

    fn find_start(&self, input: &[u8], from: usize) -> Option<usize> {
        if self.anchored { return if from == 0 { Some(0) } else { None }; }
        if self.prefix.is_empty() { return Some(from); }
        if let Some(shift) = &self.bmh_shift {
            memmem_bmh(&input[from..], &self.prefix, shift).map(|i| i + from)
        } else {
            memmem(&input[from..], &self.prefix).map(|i| i + from)
        }
    }

    fn is_match_bitset(&self, input: &[u8]) -> bool {
        let start_pos = match self.find_start(input, 0) { Some(s) => s, None => return false };
        let mut state = self.start_state;
        let mut pos = start_pos;
        if state.accept { return true; }
        while pos < input.len() {
            if state.set == 0 { return false; }
            let byte = input[pos];
            let mut next = EClose::default();
            let mut bits = state.set;
            while bits != 0 {
                let pc = bits.trailing_zeros();
                bits &= bits - 1;
                let advance = match &self.code[pc as usize] {
                    Inst::Byte(b) => *b == byte,
                    Inst::Any => true,
                    Inst::Class(c, n) => {
                        let hit = (c[(byte >> 3) as usize] >> (byte & 7)) & 1 == 1;
                        hit != *n
                    }
                    _ => false,
                };
                if advance {
                    let ec = self.eclose[pc as usize + 1];
                    next.set |= ec.set;
                    next.accept |= ec.accept;
                }
            }
            state = next;
            if state.accept { return true; }
            pos += 1;
        }
        state.accept
    }

    fn find_slow(&self, input: &[u8]) -> Option<Vec<Option<usize>>> {
        let start = self.find_start(input, 0)?;
        self.exec(input, start)
    }

    fn exec(&self, input: &[u8], start: usize) -> Option<Vec<Option<usize>>> {
        let mut current: Vec<Thread> = Vec::new();
        let mut next: Vec<Thread> = Vec::new();
        let mut seen = vec![false; self.code.len()];
        let initial = Thread { pc: 0, saves: vec![None; self.saves] };
        self.add(&mut current, initial, start, input, &mut seen);
        let mut matched: Option<Vec<Option<usize>>> = None;
        let mut pos = start;
        loop {
            let byte = input.get(pos).copied();
            for x in seen.iter_mut() { *x = false; }
            for thread in current.drain(..) {
                match &self.code[thread.pc as usize] {
                    Inst::Byte(b) if Some(*b) == byte => {
                        let mut t = thread; t.pc += 1;
                        self.add(&mut next, t, pos + 1, input, &mut seen);
                    }
                    Inst::Any if byte.is_some() => {
                        let mut t = thread; t.pc += 1;
                        self.add(&mut next, t, pos + 1, input, &mut seen);
                    }
                    Inst::Class(bits, negate) => {
                        if let Some(b) = byte {
                            let hit = (bits[(b >> 3) as usize] >> (b & 7)) & 1 == 1;
                            if hit != *negate {
                                let mut t = thread; t.pc += 1;
                                self.add(&mut next, t, pos + 1, input, &mut seen);
                            }
                        }
                    }
                    Inst::Match => { matched = Some(thread.saves); break; }
                    _ => {}
                }
            }
            mem::swap(&mut current, &mut next);
            next.clear();
            if byte.is_none() || current.is_empty() { break; }
            pos += 1;
        }
        matched
    }

    fn add(&self, list: &mut Vec<Thread>, thread: Thread, pos: usize, input: &[u8], seen: &mut [bool]) {
        if seen[thread.pc as usize] { return; }
        seen[thread.pc as usize] = true;
        match &self.code[thread.pc as usize] {
            Inst::Jmp(target) => {
                let mut t = thread; t.pc = *target;
                self.add(list, t, pos, input, seen);
            }
            Inst::Split(a, b) => {
                let (a, b) = (*a, *b);
                let mut t1 = thread.clone(); t1.pc = a;
                self.add(list, t1, pos, input, seen);
                let mut t2 = thread; t2.pc = b;
                self.add(list, t2, pos, input, seen);
            }
            Inst::Save(slot) => {
                let slot = *slot as usize;
                let mut t = thread; t.saves[slot] = Some(pos); t.pc += 1;
                self.add(list, t, pos, input, seen);
            }
            Inst::End if pos == input.len() => {
                let mut t = thread; t.pc += 1;
                self.add(list, t, pos, input, seen);
            }
            Inst::End => {}
            _ => list.push(thread),
        }
    }
}

#[derive(Clone)]
struct Thread {
    pc: u32,
    saves: Vec<Option<usize>>,
}

fn step_dfa(code: &[Inst], eclose: &[EClose], dfa: &mut Dfa, state: u32, byte: u8) -> (u32, bool) {
    let src = dfa.sets[state as usize];
    let mut new_set: u128 = 0;
    let mut new_accept = false;
    let mut bits = src;
    while bits != 0 {
        let pc = bits.trailing_zeros();
        bits &= bits - 1;
        let advance = match &code[pc as usize] {
            Inst::Byte(b) => *b == byte,
            Inst::Any => true,
            Inst::Class(c, n) => ((c[(byte >> 3) as usize] >> (byte & 7)) & 1 == 1) != *n,
            _ => false,
        };
        if advance {
            let ec = eclose[pc as usize + 1];
            new_set |= ec.set;
            new_accept |= ec.accept;
        }
    }
    if dfa.sets.len() >= 4096 && !dfa.set_to_id.contains_key(&new_set) {
        dfa.disabled = true;
        return (0, new_accept);
    }
    (dfa.intern(new_set), new_accept)
}

fn build_bmh_shift(needle: &[u8]) -> [u8; 256] {
    let n = needle.len().min(255);
    let mut shift = [n as u8; 256];
    for i in 0..n - 1 { shift[needle[i] as usize] = (n - 1 - i) as u8; }
    shift
}

fn extract_prefix(code: &[Inst]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < code.len() {
        match &code[i] {
            Inst::Save(_) => i += 1,
            Inst::Byte(b) => { out.push(*b); i += 1; }
            _ => break,
        }
    }
    out
}

#[inline]
fn memmem_bmh(haystack: &[u8], needle: &[u8], _shift: &[u8; 256]) -> Option<usize> {
    let n = needle.len();
    if haystack.len() < n { return None; }
    let last = n - 1;
    let last_byte = needle[last];
    let mut i = 0;
    while i + n <= haystack.len() {
        let scan = &haystack[i + last..];
        match memchr_u8(scan, last_byte) {
            None => return None,
            Some(off) => {
                let j = i + off;
                if j + n > haystack.len() { return None; }
                if haystack[j..j + last] == needle[..last] { return Some(j); }
                i = j + 1;
            }
        }
    }
    None
}

const SWAR_HI: u64 = 0x8080808080808080;
const SWAR_LO: u64 = 0x0101010101010101;

#[inline]
fn swar_zero_byte(x: u64) -> u64 { x.wrapping_sub(SWAR_LO) & !x & SWAR_HI }

#[inline]
fn memchr_u8(h: &[u8], b: u8) -> Option<usize> {
    let pat = (b as u64).wrapping_mul(SWAR_LO);
    let mut i = 0;
    while i + 8 <= h.len() {
        let chunk = u64::from_le_bytes(h[i..i + 8].try_into().unwrap());
        let z = swar_zero_byte(chunk ^ pat);
        if z != 0 { return Some(i + (z.trailing_zeros() / 8) as usize); }
        i += 8;
    }
    while i < h.len() {
        if h[i] == b { return Some(i); }
        i += 1;
    }
    None
}

#[inline]
fn memchr2(h: &[u8], a: u8, b: u8) -> Option<usize> {
    let pa = (a as u64).wrapping_mul(SWAR_LO);
    let pb = (b as u64).wrapping_mul(SWAR_LO);
    let mut i = 0;
    while i + 8 <= h.len() {
        let chunk = u64::from_le_bytes(h[i..i + 8].try_into().unwrap());
        let z = swar_zero_byte(chunk ^ pa) | swar_zero_byte(chunk ^ pb);
        if z != 0 { return Some(i + (z.trailing_zeros() / 8) as usize); }
        i += 8;
    }
    while i < h.len() {
        if h[i] == a || h[i] == b { return Some(i); }
        i += 1;
    }
    None
}

#[inline]
fn memchr3(h: &[u8], a: u8, b: u8, c: u8) -> Option<usize> {
    let pa = (a as u64).wrapping_mul(SWAR_LO);
    let pb = (b as u64).wrapping_mul(SWAR_LO);
    let pc = (c as u64).wrapping_mul(SWAR_LO);
    let mut i = 0;
    while i + 8 <= h.len() {
        let chunk = u64::from_le_bytes(h[i..i + 8].try_into().unwrap());
        let z = swar_zero_byte(chunk ^ pa) | swar_zero_byte(chunk ^ pb) | swar_zero_byte(chunk ^ pc);
        if z != 0 { return Some(i + (z.trailing_zeros() / 8) as usize); }
        i += 8;
    }
    while i < h.len() {
        if h[i] == a || h[i] == b || h[i] == c { return Some(i); }
        i += 1;
    }
    None
}

fn first_byte_present(h: &[u8], fb: &FirstBytes) -> Option<usize> {
    match fb {
        FirstBytes::Two(a, b) => memchr2(h, *a, *b),
        FirstBytes::Three(a, b, c) => memchr3(h, *a, *b, *c),
        FirstBytes::Set(s) => h.iter().position(|b| s[*b as usize]),
    }
}

fn build_literal_alts(ast: &Ast) -> Option<LiteralAlts> {
    let alts = match ast {
        Ast::Alt(xs) => xs,
        _ => return None,
    };
    let mut literals = Vec::with_capacity(alts.len());
    for a in alts {
        literals.push(ast_to_literal(a)?);
    }
    if literals.iter().any(|l| l.is_empty()) { return None; }
    let mut set = [false; 256];
    for l in &literals { set[l[0] as usize] = true; }
    let bytes: Vec<u8> = (0..=255u8).filter(|b| set[*b as usize]).collect();
    let first_bytes = match bytes.len() {
        1 => return None,
        2 => FirstBytes::Two(bytes[0], bytes[1]),
        3 => FirstBytes::Three(bytes[0], bytes[1], bytes[2]),
        _ => FirstBytes::Set(Box::new(set)),
    };
    Some(LiteralAlts { literals, first_bytes })
}

fn ast_to_literal(ast: &Ast) -> Option<Vec<u8>> {
    match ast {
        Ast::Byte(b) => Some(vec![*b]),
        Ast::Concat(xs) => {
            let mut out = Vec::new();
            for x in xs { out.extend(ast_to_literal(x)?); }
            Some(out)
        }
        Ast::Group(inner, _) => ast_to_literal(inner),
        _ => None,
    }
}

fn match_literal_alts(input: &[u8], alts: &LiteralAlts) -> Option<usize> {
    let mut start = 0;
    while start < input.len() {
        let off = first_byte_present(&input[start..], &alts.first_bytes)?;
        let j = start + off;
        for lit in &alts.literals {
            if j + lit.len() <= input.len() && input[j..j + lit.len()] == lit[..] {
                return Some(j);
            }
        }
        start = j + 1;
    }
    None
}

fn build_first_bytes(ast: &Ast) -> Option<FirstBytes> {
    let mut set = [false; 256];
    if !collect_first(ast, &mut set) { return None; }
    let bytes: Vec<u8> = (0..=255u8).filter(|b| set[*b as usize]).collect();
    if bytes.is_empty() || bytes.len() == 256 { return None; }
    match bytes.len() {
        1 => None,
        2 => Some(FirstBytes::Two(bytes[0], bytes[1])),
        3 => Some(FirstBytes::Three(bytes[0], bytes[1], bytes[2])),
        _ => Some(FirstBytes::Set(Box::new(set))),
    }
}

fn collect_first(ast: &Ast, set: &mut [bool; 256]) -> bool {
    match ast {
        Ast::Byte(b) => { set[*b as usize] = true; true }
        Ast::Class(bits, neg) => {
            for b in 0..=255u8 {
                let hit = (bits[(b >> 3) as usize] >> (b & 7)) & 1 == 1;
                if hit != *neg { set[b as usize] = true; }
            }
            true
        }
        Ast::Concat(xs) => {
            for x in xs {
                if !collect_first(x, set) { return false; }
                if !matches!(x, Ast::Star(_) | Ast::Opt(_)) { return true; }
            }
            true
        }
        Ast::Alt(xs) => {
            for x in xs { if !collect_first(x, set) { return false; } }
            true
        }
        Ast::Plus(inner) | Ast::Group(inner, _) => collect_first(inner, set),
        Ast::Star(inner) | Ast::Opt(inner) => { collect_first(inner, set); false }
        _ => false,
    }
}

fn required_literal(ast: &Ast) -> Vec<u8> {
    match ast {
        Ast::Byte(b) => vec![*b],
        Ast::Concat(xs) => {
            let mut best: Vec<u8> = Vec::new();
            let mut cur: Vec<u8> = Vec::new();
            for x in xs {
                let r = required_literal(x);
                if r.is_empty() {
                    if cur.len() > best.len() { best = mem::take(&mut cur); }
                    cur.clear();
                } else {
                    cur.extend(&r);
                }
            }
            if cur.len() > best.len() { best = cur; }
            best
        }
        Ast::Plus(inner) | Ast::Group(inner, _) => required_literal(inner),
        _ => Vec::new(),
    }
}

#[inline]
fn memmem(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() { return Some(0); }
    if needle.len() == 1 { return memchr_u8(haystack, needle[0]); }
    let first = needle[0];
    let nlen = needle.len();
    if haystack.len() < nlen { return None; }
    let limit = haystack.len() - nlen + 1;
    let mut i = 0;
    while i < limit {
        if let Some(off) = haystack[i..limit].iter().position(|b| *b == first) {
            let j = i + off;
            if haystack[j..j + nlen] == *needle { return Some(j); }
            i = j + 1;
        } else { return None; }
    }
    None
}

fn compile(ast: &Ast, code: &mut Vec<Inst>) {
    match ast {
        Ast::Byte(b) => code.push(Inst::Byte(*b)),
        Ast::Any => code.push(Inst::Any),
        Ast::Class(bits, neg) => code.push(Inst::Class(bits.clone(), *neg)),
        Ast::End => code.push(Inst::End),
        Ast::Concat(xs) => xs.iter().for_each(|x| compile(x, code)),
        Ast::Alt(xs) => compile_alt(xs, code),
        Ast::Star(inner) => {
            let l1 = code.len();
            code.push(Inst::Split(0, 0));
            let l2 = code.len() as u32;
            compile(inner, code);
            code.push(Inst::Jmp(l1 as u32));
            let l3 = code.len() as u32;
            if let Inst::Split(a, b) = &mut code[l1] { *a = l2; *b = l3; }
        }
        Ast::Plus(inner) => {
            let l1 = code.len() as u32;
            compile(inner, code);
            let split = code.len();
            code.push(Inst::Split(l1, 0));
            let l2 = code.len() as u32;
            if let Inst::Split(_, b) = &mut code[split] { *b = l2; }
        }
        Ast::Opt(inner) => {
            let split = code.len();
            code.push(Inst::Split(0, 0));
            let l1 = code.len() as u32;
            compile(inner, code);
            let l2 = code.len() as u32;
            if let Inst::Split(a, b) = &mut code[split] { *a = l1; *b = l2; }
        }
        Ast::Group(inner, idx) => {
            code.push(Inst::Save((idx * 2) as u32));
            compile(inner, code);
            code.push(Inst::Save((idx * 2 + 1) as u32));
        }
    }
}

fn compile_alt(alts: &[Ast], code: &mut Vec<Inst>) {
    if alts.len() == 1 { compile(&alts[0], code); return; }
    let split = code.len();
    code.push(Inst::Split(0, 0));
    let l1 = code.len() as u32;
    compile(&alts[0], code);
    let jmp = code.len();
    code.push(Inst::Jmp(0));
    let l2 = code.len() as u32;
    if let Inst::Split(a, b) = &mut code[split] { *a = l1; *b = l2; }
    compile_alt(&alts[1..], code);
    let end = code.len() as u32;
    if let Inst::Jmp(t) = &mut code[jmp] { *t = end; }
}

struct Parser<'a> {
    src: &'a [u8],
    pos: usize,
    groups: usize,
}

impl<'a> Parser<'a> {
    fn new(src: &'a [u8]) -> Self { Parser { src, pos: 0, groups: 0 } }
    fn peek(&self) -> Option<u8> { self.src.get(self.pos).copied() }
    fn bump(&mut self) -> Option<u8> {
        let c = self.peek();
        if c.is_some() { self.pos += 1; }
        c
    }

    fn parse_alt(&mut self) -> Result<Ast, String> {
        let mut alts = vec![self.parse_concat()?];
        while self.peek() == Some(b'|') {
            self.bump();
            alts.push(self.parse_concat()?);
        }
        Ok(if alts.len() == 1 { alts.pop().unwrap() } else { Ast::Alt(alts) })
    }

    fn parse_concat(&mut self) -> Result<Ast, String> {
        let mut parts = Vec::new();
        while let Some(c) = self.peek() {
            if c == b'|' || c == b')' { break; }
            parts.push(self.parse_repeat()?);
        }
        Ok(if parts.len() == 1 { parts.pop().unwrap() } else { Ast::Concat(parts) })
    }

    fn parse_repeat(&mut self) -> Result<Ast, String> {
        let atom = self.parse_atom()?;
        Ok(match self.peek() {
            Some(b'*') => { self.bump(); Ast::Star(Box::new(atom)) }
            Some(b'+') => { self.bump(); Ast::Plus(Box::new(atom)) }
            Some(b'?') => { self.bump(); Ast::Opt(Box::new(atom)) }
            _ => atom,
        })
    }

    fn parse_atom(&mut self) -> Result<Ast, String> {
        let c = self.bump().ok_or("unexpected end")?;
        match c {
            b'(' => {
                self.groups += 1;
                let idx = self.groups;
                let inner = self.parse_alt()?;
                if self.bump() != Some(b')') { return Err("expected )".into()); }
                Ok(Ast::Group(Box::new(inner), idx))
            }
            b'[' => self.parse_class(),
            b'.' => Ok(Ast::Any),
            b'$' => Ok(Ast::End),
            b'\\' => self.parse_escape(),
            b')' | b'|' | b'*' | b'+' | b'?' | b'^' => Err(format!("unexpected '{}'", c as char)),
            other => Ok(Ast::Byte(other)),
        }
    }

    fn parse_escape(&mut self) -> Result<Ast, String> {
        let c = self.bump().ok_or("bad escape")?;
        Ok(match c {
            b'd' => class_from_ranges(&[(b'0', b'9')], false),
            b'D' => class_from_ranges(&[(b'0', b'9')], true),
            b'w' => class_from_ranges(&[(b'0', b'9'), (b'A', b'Z'), (b'_', b'_'), (b'a', b'z')], false),
            b'W' => class_from_ranges(&[(b'0', b'9'), (b'A', b'Z'), (b'_', b'_'), (b'a', b'z')], true),
            b's' => class_from_ranges(&[(b'\t', b'\n'), (b'\r', b'\r'), (b' ', b' ')], false),
            b'S' => class_from_ranges(&[(b'\t', b'\n'), (b'\r', b'\r'), (b' ', b' ')], true),
            b'n' => Ast::Byte(b'\n'),
            b't' => Ast::Byte(b'\t'),
            b'r' => Ast::Byte(b'\r'),
            other => Ast::Byte(other),
        })
    }

    fn parse_class(&mut self) -> Result<Ast, String> {
        let negate = if self.peek() == Some(b'^') { self.bump(); true } else { false };
        let mut bits = Box::new([0u8; 32]);
        while let Some(c) = self.peek() {
            if c == b']' { self.bump(); return Ok(Ast::Class(bits, negate)); }
            self.bump();
            let start = if c == b'\\' { decode_escape(self.bump())? } else { c };
            let end = if self.peek() == Some(b'-') && self.src.get(self.pos + 1) != Some(&b']') {
                self.bump();
                let next = self.bump().ok_or("bad range")?;
                if next == b'\\' { decode_escape(self.bump())? } else { next }
            } else { start };
            for b in start..=end { bits[(b >> 3) as usize] |= 1 << (b & 7); }
        }
        Err("unterminated class".into())
    }
}

fn class_from_ranges(ranges: &[(u8, u8)], negate: bool) -> Ast {
    let mut bits = Box::new([0u8; 32]);
    for (lo, hi) in ranges {
        for b in *lo..=*hi { bits[(b >> 3) as usize] |= 1 << (b & 7); }
    }
    Ast::Class(bits, negate)
}

fn decode_escape(c: Option<u8>) -> Result<u8, String> {
    match c {
        Some(b'n') => Ok(b'\n'),
        Some(b't') => Ok(b'\t'),
        Some(b'r') => Ok(b'\r'),
        Some(x) => Ok(x),
        None => Err("bad escape".into()),
    }
}
