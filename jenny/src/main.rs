/*
By Bob Jenkins, March 2003.  Public domain.
Rust translation.

jenny -- generate tests from m dimensions of features that cover all
  n-tuples of features, n <= m, with each feature chosen from a different
  dimension.
*/

use std::io::{self, BufRead};

// -----------------------------------------------------------------------
// Constants
// -----------------------------------------------------------------------

const MAX_FEATURES: usize = 52;
const MAX_TESTS: usize = 65534;
const MAX_N: usize = 32;
const MAX_WITHOUT: usize = MAX_FEATURES * MAX_N;
const MAX_DIMENSIONS: usize = 65534;
const GROUP_SIZE: usize = 5;
const MAX_NO_PROGRESS: usize = 2;
const MAX_ITERS: usize = 10;

const FEATURE_NAME: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ";

// -----------------------------------------------------------------------
// Bob Jenkins' smallprng (4-state 64-bit PRNG, fits in registers)
// -----------------------------------------------------------------------

struct RanCtx {
    a: u64,
    b: u64,
    c: u64,
    d: u64,
}

impl RanCtx {
    fn new(seed: u32) -> Self {
        let s = seed as u64;
        let mut ctx = RanCtx { a: 0xf1ea5eed, b: s, c: s, d: s };
        for _ in 0..20 { ctx.next(); }
        ctx
    }

    fn next(&mut self) -> u64 {
        let e = self.a.wrapping_sub(self.b.rotate_left(7));
        self.a = self.b ^ self.c.rotate_left(13);
        self.b = self.c.wrapping_add(self.d.rotate_left(37));
        self.c = self.d.wrapping_add(e);
        self.d = e.wrapping_add(self.a);
        self.d
    }
}

// -----------------------------------------------------------------------
// Data structures
// -----------------------------------------------------------------------

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Feature {
    d: u16,
    f: u16,
}

// TUPLE_ARRAY matches C's block size constant.
const TUPLE_ARRAY: usize = 5040;

// Block-based storage matching C's linked-list-of-blocks structure.
// Each block holds at most TUPLE_ARRAY/n tuples.  Deletion is block-local
// (swap-with-last-of-same-block), matching C's delete_tuple behaviour.
struct TupleList {
    blocks: Vec<Vec<Feature>>,
    n: usize,
    count: usize,
    block_cap: usize,
}

impl TupleList {
    fn new(n: usize) -> Self {
        let block_cap = if n > 0 { TUPLE_ARRAY / n } else { TUPLE_ARRAY };
        TupleList { blocks: Vec::new(), n, count: 0, block_cap }
    }

    fn insert(&mut self, tuple: &[Feature]) {
        let cap = self.block_cap;
        let n = self.n;
        if self.blocks.is_empty() || self.blocks.last().unwrap().len() / n >= cap {
            self.blocks.push(Vec::with_capacity(cap * n));
        }
        self.blocks.last_mut().unwrap().extend_from_slice(tuple);
        self.count += 1;
    }

    // Translate global index to (block_idx, pos_in_block).
    fn locate(&self, global_i: usize) -> (usize, usize) {
        let n = self.n;
        let mut rem = global_i;
        for (bi, block) in self.blocks.iter().enumerate() {
            let bc = block.len() / n;
            if rem < bc { return (bi, rem); }
            rem -= bc;
        }
        panic!("TupleList::locate out of bounds");
    }

    fn get(&self, global_i: usize) -> &[Feature] {
        let n = self.n;
        let (bi, pos) = self.locate(global_i);
        &self.blocks[bi][pos * n..(pos + 1) * n]
    }

    // Delete all tuples matching predicate, iterating block-locally to avoid
    // locate() overhead.  Matches C's block-local swap-with-last semantics.
    fn retain_if<F: Fn(&[Feature]) -> bool>(&mut self, keep: F) {
        let n = self.n;
        let mut bi = 0;
        while bi < self.blocks.len() {
            let mut start = 0usize;
            loop {
                let len = self.blocks[bi].len();
                if start + n > len { break; }
                if keep(&self.blocks[bi][start..start + n]) {
                    start += n;
                } else {
                    // swap with last of this block
                    let last_start = len - n;
                    if start != last_start {
                        self.blocks[bi].copy_within(last_start..len, start);
                    }
                    self.blocks[bi].truncate(last_start);
                    self.count -= 1;
                    // don't advance start; the moved element is now at start
                }
            }
            if self.blocks[bi].is_empty() {
                self.blocks.remove(bi);
            } else {
                bi += 1;
            }
        }
    }

    // Delete tuple at global index global_i using block-local swap-with-last,
    // matching C's delete_tuple.  Returns true if a tuple remains at global_i.
    fn delete(&mut self, global_i: usize) -> bool {
        let n = self.n;
        let (bi, pos) = self.locate(global_i);
        let block_count = self.blocks[bi].len() / n;
        let last_pos = block_count - 1;

        self.count -= 1;

        if pos != last_pos {
            // Replace pos with last of this block.
            let src = last_pos * n;
            let dst = pos * n;
            for k in 0..n {
                self.blocks[bi][dst + k] = self.blocks[bi][src + k];
            }
        }
        self.blocks[bi].truncate(last_pos * n);

        if self.blocks[bi].is_empty() {
            self.blocks.remove(bi);
        }

        if pos != last_pos {
            true  // moved element now at global_i; caller stays here
        } else {
            // Deleted the last element of the block.  global_i now points to
            // the first element of the next block (or past end).
            global_i < self.count
        }
    }
}

#[derive(Clone)]
struct Without {
    fe: Vec<Feature>,
}

struct Test {
    f: Vec<u16>,
}

impl Test {
    fn new(ndim: usize) -> Self {
        Test { f: vec![0; ndim] }
    }
    fn new_all_max(ndim: usize) -> Self {
        Test { f: vec![u16::MAX; ndim] }
    }
}

// -----------------------------------------------------------------------
// State
// -----------------------------------------------------------------------

struct State {
    n_final: usize,
    e_final: usize,
    ndim: usize,
    dim: Vec<usize>,
    tu: Vec<Vec<TupleList>>,     // tu[d][f] = uncovered tuples for (d,f)
    n_size: Vec<Vec<usize>>,     // n_size[d][f] = current tuple size
    tests: Vec<Test>,
    wc2: Vec<Without>,           // original withouts
    wc3: Vec<Without>,           // deduced withouts
    wc: Vec<Vec<usize>>,         // wc[d] = indices into wc2 for dimension d
    tuple_tester: Test,
    dimord: Vec<usize>,
    featord: Vec<usize>,
    rng: RanCtx,
}

impl State {
    fn new() -> Self {
        State {
            n_final: 2,
            e_final: 0,
            ndim: 0,
            dim: Vec::new(),
            tu: Vec::new(),
            n_size: Vec::new(),
            tests: Vec::new(),
            wc2: Vec::new(),
            wc3: Vec::new(),
            wc: Vec::new(),
            tuple_tester: Test::new(0),
            dimord: Vec::new(),
            featord: Vec::new(),
            rng: RanCtx::new(0),
        }
    }
}

// -----------------------------------------------------------------------
// Without checking
// -----------------------------------------------------------------------

#[inline(always)]
fn without_matches(t: &[u16], w: &Without) -> bool {
    let mut i = 0;
    while i < w.fe.len() {
        let dim_d = w.fe[i].d;
        let mut dimension_match = false;
        loop {
            if t[w.fe[i].d as usize] == w.fe[i].f { dimension_match = true; }
            i += 1;
            if i >= w.fe.len() || w.fe[i].d != dim_d { break; }
        }
        if !dimension_match { return false; }
    }
    true
}

fn count_withouts_pool(t: &[u16], wc_pool: &[Without], wc_indices: &[usize]) -> usize {
    wc_indices.iter().filter(|&&wi| without_matches(t, &wc_pool[wi])).count()
}

fn count_withouts_list(t: &[u16], withouts: &[Without]) -> usize {
    withouts.iter().filter(|w| without_matches(t, w)).count()
}

fn count_withouts_both(t: &[u16], wc2: &[Without], wc3: &[Without]) -> usize {
    count_withouts_list(t, wc2) + count_withouts_list(t, wc3)
}

// -----------------------------------------------------------------------
// Token parsing
// -----------------------------------------------------------------------

#[derive(PartialEq, Debug)]
enum Token {
    End,
    Number(usize),
    Feature(usize),
    Space,
    Error,
}

fn parse_token(inp: &[u8], curr: &mut usize) -> Token {
    if *curr >= inp.len() || inp[*curr] == 0 {
        return Token::End;
    }
    let ch = inp[*curr];
    if ch == b' ' || ch == b'\t' || ch == b'\n' || ch == b'\r' {
        while *curr < inp.len() {
            let c = inp[*curr];
            if c != b' ' && c != b'\t' && c != b'\n' && c != b'\r' {
                break;
            }
            *curr += 1;
        }
        Token::Space
    } else if ch >= b'0' && ch <= b'9' {
        let mut number: usize = 0;
        while *curr < inp.len() && inp[*curr] >= b'0' && inp[*curr] <= b'9' {
            number = number * 10 + (inp[*curr] - b'0') as usize;
            *curr += 1;
        }
        Token::Number(number)
    } else if (ch >= b'a' && ch <= b'z') || (ch >= b'A' && ch <= b'Z') {
        for i in 0..MAX_FEATURES {
            if FEATURE_NAME[i] == ch {
                *curr += 1;
                return Token::Feature(i);
            }
        }
        println!("jenny: the name '{}' is not used for any feature", ch as char);
        Token::Error
    } else {
        Token::Error
    }
}

// -----------------------------------------------------------------------
// Tuple helpers
// -----------------------------------------------------------------------

fn test_tuple(test_f: &[u16], tuple: &[Feature]) -> bool {
    for fe in tuple {
        // SAFETY: fe.d is always a valid dimension index; tuples are only
        // constructed with d < ndim and test_f has length ndim.
        if fe.f != unsafe { *test_f.get_unchecked(fe.d as usize) } {
            return false;
        }
    }
    true
}

fn subset_tuple(t1: &[Feature], t2: &[Feature]) -> bool {
    if t2.len() < t1.len() {
        return false;
    }
    let mut j = 0;
    for i in 0..t1.len() {
        while t1[i].d > t2[j].d {
            j += 1;
            if j == t2.len() {
                return false;
            }
        }
        if t1[i].d != t2[j].d || t1[i].f != t2[j].f {
            return false;
        }
    }
    true
}

fn show_tuple(fe: &[Feature]) {
    let mut out = String::new();
    for f in fe {
        out.push(' ');
        out.push_str(&(f.d as usize + 1).to_string());
        out.push(FEATURE_NAME[f.f as usize] as char);
    }
    out.push(' ');
    println!("{}", out);
}

// -----------------------------------------------------------------------
// parse_w
// -----------------------------------------------------------------------

fn parse_w(s: &mut State, myarg: &[u8]) -> bool {
    let mut fe: Vec<Feature> = Vec::new();
    let mut used = vec![false; s.ndim];
    let mut curr = 0;

    let mut value = match parse_token(myarg, &mut curr) {
        Token::Number(v) => v,
        _ => {
            println!("jenny: -w is <number><features><number><features>...");
            println!("jenny: -w must start with an integer (1 to #dimensions)");
            return false;
        }
    };

    // State machine replacing C gotos: number -> feature(s) -> number | end
    loop {
        // 'number:' label equivalent
        let dimension_number = value - 1;
        if dimension_number >= s.ndim {
            println!("jenny: -w, dimension {} does not exist, you gave only {} dimensions",
                     dimension_number + 1, s.ndim);
            return false;
        }
        if used[dimension_number] {
            println!("jenny: -w, dimension {} was given twice in a single without",
                     dimension_number + 1);
            return false;
        }
        used[dimension_number] = true;

        // must be followed by at least one feature
        let first_f = match parse_token(myarg, &mut curr) {
            Token::Feature(f) => f,
            Token::End => {
                println!("jenny: -w, withouts must follow numbers with features");
                return false;
            }
            _ => {
                println!("jenny: -w, unexpected without syntax");
                println!("jenny: proper withouts look like -w2a1bc99a");
                return false;
            }
        };
        value = first_f; // will be processed in feature loop

        // 'feature:' label equivalent
        loop {
            let f = value;
            if f >= s.dim[dimension_number] {
                println!("jenny: -w, there is no feature '{}' in dimension {}",
                         FEATURE_NAME[f] as char, dimension_number + 1);
                return false;
            }
            fe.push(Feature { d: dimension_number as u16, f: f as u16 });
            if fe.len() >= MAX_WITHOUT {
                println!("jenny: -w, at most {} features in a single without", MAX_WITHOUT);
                return false;
            }

            match parse_token(myarg, &mut curr) {
                Token::Feature(f2) => {
                    value = f2;
                    // continue feature loop
                }
                Token::Number(n) => {
                    value = n;
                    break; // exit feature loop, re-enter number loop
                }
                Token::End => {
                    // 'end:' — sort and store
                    fe.sort_by(|a, b| a.d.cmp(&b.d).then(a.f.cmp(&b.f)));
                    s.wc2.push(Without { fe });
                    return true;
                }
                _ => {
                    println!("jenny: -w, unexpected without syntax");
                    println!("jenny: proper withouts look like -w2a1bc99a");
                    return false;
                }
            }
        }
        // loop back to 'number:' with value = next dimension number
    }
}

// -----------------------------------------------------------------------
// parse_n, parse_s
// -----------------------------------------------------------------------

fn parse_n(s: &mut State, myarg: &[u8]) -> bool {
    let mut curr = 0;
    match parse_token(myarg, &mut curr) {
        Token::Number(v) => {
            if parse_token(myarg, &mut curr) != Token::End {
                println!("jenny: -n should be followed by just an integer");
                return false;
            }
            if v < 1 || v > 32 {
                println!("jenny: -n says all n-tuples should be covered.");
                return false;
            }
            if v > s.ndim {
                println!("jenny: -n, {}-tuples are impossible with only {} dimensions",
                         v, s.ndim);
                return false;
            }
            s.n_final = v;
            true
        }
        _ => {
            println!("jenny: -n should give an integer in 1..32, for example, -n2.");
            false
        }
    }
}

fn parse_s(s: &mut State, myarg: &[u8]) -> bool {
    let mut curr = 0;
    match parse_token(myarg, &mut curr) {
        Token::Number(seed) => {
            if parse_token(myarg, &mut curr) != Token::End {
                println!("jenny: -s should give just an integer, example -s123");
                return false;
            }
            s.rng = RanCtx::new(seed as u32);
            true
        }
        _ => {
            println!("jenny: -s must be followed by a positive integer");
            false
        }
    }
}

fn parse_e(s: &mut State, myarg: &[u8]) -> bool {
    let mut curr = 0;
    match parse_token(myarg, &mut curr) {
        Token::Number(v) => {
            if parse_token(myarg, &mut curr) != Token::End {
                println!("jenny: -e should give just an integer, example -e2");
                return false;
            }
            s.e_final = v;
            true
        }
        _ => {
            println!("jenny: -e must be followed by a non-negative integer");
            false
        }
    }
}

// -----------------------------------------------------------------------
// preliminary
// -----------------------------------------------------------------------

fn preliminary(s: &mut State) {
    s.tuple_tester = Test::new_all_max(s.ndim);
    s.dimord = (0..s.ndim).collect();
    s.featord = vec![0usize; MAX_FEATURES];
    s.wc = vec![Vec::new(); s.ndim];

    s.tu = (0..s.ndim).map(|d| {
        (0..s.dim[d]).map(|_| TupleList::new(1)).collect()
    }).collect();
    s.n_size = (0..s.ndim).map(|d| vec![0usize; s.dim[d]]).collect();

    // build dimension-specific without indices
    for (wi, w) in s.wc2.iter().enumerate() {
        let mut old_d: Option<u16> = None;
        for fe in &w.fe {
            if Some(fe.d) != old_d {
                s.wc[fe.d as usize].push(wi);
                old_d = Some(fe.d);
            }
        }
    }
}

// -----------------------------------------------------------------------
// load
// -----------------------------------------------------------------------

fn load(s: &mut State, testfile: &str) -> bool {
    let reader: Box<dyn BufRead> = if testfile.is_empty() {
        Box::new(io::BufReader::new(io::stdin()))
    } else {
        match std::fs::File::open(testfile) {
            Ok(f) => Box::new(io::BufReader::new(f)),
            Err(_) => {
                println!("jenny: file {} could not be opened", testfile);
                return false;
            }
        }
    };

    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        if line.starts_with('.') {
            break;
        }
        let buf = line.as_bytes();
        let mut curr = 0;
        let mut t = Test::new(s.ndim);

        for i in 0..s.ndim {
            if parse_token(buf, &mut curr) != Token::Space {
                println!("jenny: -o, non-space found where space expected");
                return false;
            }
            match parse_token(buf, &mut curr) {
                Token::Number(v) => {
                    if v - 1 != i {
                        println!("jenny: -o, number {} found out-of-place", v);
                        return false;
                    }
                }
                _ => {
                    println!("jenny: -o, non-number found where number expected");
                    return false;
                }
            }
            match parse_token(buf, &mut curr) {
                Token::Feature(f) => {
                    if f >= s.dim[i] {
                        println!("jenny: -o, feature {} does not exist in dimension {}",
                                 FEATURE_NAME[f] as char, i + 1);
                        return false;
                    }
                    t.f[i] = f as u16;
                }
                _ => {
                    println!("jenny: -o, non-feature found where feature expected");
                    return false;
                }
            }
        }
        if parse_token(buf, &mut curr) != Token::Space {
            println!("jenny: -o, non-space found where trailing space expected");
            return false;
        }
        if parse_token(buf, &mut curr) != Token::End {
            println!("jenny: -o, testcase not properly terminated");
            return false;
        }
        if count_withouts_list(&t.f, &s.wc2) > 0 {
            println!("jenny: -o, old testcase contains some without");
            return false;
        }
        add_test(s, t);
    }
    true
}

// -----------------------------------------------------------------------
// add_test
// -----------------------------------------------------------------------

fn add_test(s: &mut State, t: Test) -> bool {
    if s.tests.len() == MAX_TESTS {
        return false;
    }
    s.tests.push(t);
    true
}

// -----------------------------------------------------------------------
// report
// -----------------------------------------------------------------------

fn report(t: &Test, ndim: usize) {
    let mut out = String::new();
    for i in 0..ndim {
        out.push(' ');
        out.push_str(&(i + 1).to_string());
        out.push(FEATURE_NAME[t.f[i] as usize] as char);
    }
    out.push(' ');
    println!("{}", out);
}

// -----------------------------------------------------------------------
// build_tuples: enumerate n-tuples for (d,f) and add uncovered ones
// -----------------------------------------------------------------------

fn start_builder(offset: &mut [Feature], n: usize) {
    for i in 0..n {
        offset[i].d = i as u16;
        offset[i].f = 0;
    }
}

fn next_builder(offset: &mut [Feature], n: usize, ndim: usize, dim: &[usize]) -> bool {
    let mut i = n as i64 - 1;
    while i >= 0 {
        let idx = i as usize;
        let d = offset[idx].d as usize;
        let f = offset[idx].f as usize;
        if d == ndim - n + idx && f == dim[d] - 1 {
            i -= 1;
        } else {
            break;
        }
    }
    if i == -1 {
        return false;
    }
    let idx = i as usize;
    let d = offset[idx].d as usize;
    let f = offset[idx].f as usize;
    if f < dim[d] - 1 {
        offset[idx].f += 1;
    } else {
        offset[idx].d += 1;
        offset[idx].f = 0;
    }
    for j in idx..n - 1 {
        offset[j + 1].d = offset[j].d + 1;
        offset[j + 1].f = 0;
    }
    true
}

fn build_tuples(s: &mut State, d: usize, f: usize) {
    if s.tu[d][f].count > 0 || s.n_size[d][f] == s.n_final {
        return;
    }

    s.n_size[d][f] += 1;
    let n = s.n_size[d][f];
    s.tu[d][f] = TupleList::new(n);

    let ndim = s.ndim;
    let dim = s.dim.clone();

    if n == 1 {
        // single-feature tuple: just (d, f)
        let tuple = vec![Feature { d: d as u16, f: f as u16 }];
        for i in 0..ndim { s.tuple_tester.f[i] = u16::MAX; }
        s.tuple_tester.f[d] = f as u16;
        if count_withouts_both(&s.tuple_tester.f, &s.wc2, &s.wc3) == 0 {
            let covered = s.tests.iter().any(|t| t.f[d] == f as u16);
            if !covered {
                s.tu[d][f].insert(&tuple);
            }
        }
        s.tuple_tester.f[d] = u16::MAX;
        return;
    }

    let mut offset = vec![Feature::default(); n - 1];
    let mut tuple = vec![Feature::default(); n];
    start_builder(&mut offset, n - 1);

    loop {
        // inject (d,f) into offset to form n-tuple
        let mut i = 0;
        while i < n - 1 && (offset[i].d as usize) < d {
            tuple[i] = offset[i];
            i += 1;
        }
        // skip if offset already has dimension d
        if i < n - 1 && offset[i].d as usize == d {
            if !next_builder(&mut offset, n - 1, ndim, &dim) { break; }
            continue;
        }
        tuple[i] = Feature { d: d as u16, f: f as u16 };
        let mut oi = i;
        let mut ti = i + 1;
        while ti < n {
            tuple[ti] = offset[oi];
            oi += 1;
            ti += 1;
        }

        for k in 0..n {
            s.tuple_tester.f[tuple[k].d as usize] = tuple[k].f;
        }
        if count_withouts_both(&s.tuple_tester.f, &s.wc2, &s.wc3) == 0 {
            let covered = s.tests.iter().any(|t| {
                tuple.iter().all(|fe| t.f[fe.d as usize] == fe.f)
            });
            if !covered {
                s.tu[d][f].insert(&tuple);
            }
        }
        for k in 0..n {
            s.tuple_tester.f[tuple[k].d as usize] = u16::MAX;
        }

        if !next_builder(&mut offset, n - 1, ndim, &dim) { break; }
    }
}

// -----------------------------------------------------------------------
// obey_withouts
// -----------------------------------------------------------------------

fn obey_withouts(s: &mut State, t: &mut Test, mutable: &[bool]) -> bool {
    if count_withouts_list(&t.f, &s.wc2) == 0 {
        return true;
    }

    // Reuse s.dimord as scratch; build mutable dims with non-empty wc.
    s.dimord.clear();
    for i in 0..s.ndim {
        if mutable[i] && !s.wc[i].is_empty() { s.dimord.push(i); }
    }
    let ndim = s.dimord.len();

    let mut best_len: usize;
    let mut i = 0usize;
    while i < MAX_NO_PROGRESS {
        let mut ok = true;
        let mut j = ndim;
        while j > 0 {
            let pick = (s.rng.next() as usize) % j;
            s.dimord.swap(pick, j - 1);
            let mydim = s.dimord[j - 1];
            j -= 1;

            let mut count = count_withouts_pool(&t.f, &s.wc2, &s.wc[mydim]);
            best_len = 0;

            for k in 0..s.dim[mydim] {
                t.f[mydim] = k as u16;
                let newcount = count_withouts_pool(&t.f, &s.wc2, &s.wc[mydim]);
                if newcount <= count {
                    if newcount < count {
                        i = 0; // partial progress: reset outer loop counter
                        best_len = 0;
                        count = newcount;
                    }
                    s.featord[best_len] = k;
                    best_len += 1;
                }
            }

            if best_len == 0 {
                println!("jenny: internal error a");
                t.f[mydim] = 0;
            } else if best_len == 1 {
                t.f[mydim] = s.featord[0] as u16;
            } else {
                let pick = (s.rng.next() as usize) % best_len;
                t.f[mydim] = s.featord[pick] as u16;
            }

            if count > 0 {
                ok = false;
            }
        }

        if ok {
            return true;
        }
        i += 1;
    }
    false
}

// -----------------------------------------------------------------------
// count_tuples_for, maximize_coverage
// -----------------------------------------------------------------------

fn count_tuples_for(tu: &TupleList, test_f: &[u16]) -> usize {
    let n = tu.n;
    let mut count = 0;
    for block in &tu.blocks {
        for tuple in block.chunks_exact(n) {
            if test_tuple(test_f, tuple) {
                count += 1;
            }
        }
    }
    count
}

fn maximize_coverage(s: &mut State, t: &mut Test, mutable: &[bool], n: usize) -> usize {
    // Reuse s.dimord as scratch buffer (matches C's s->dimord usage).
    s.dimord.clear();
    for i in 0..s.ndim { if mutable[i] { s.dimord.push(i); } }
    let ndim_mutable = s.dimord.len();

    // s.featord reused as best[] scratch (matches C's stack best[MAX_FEATURES]).
    let mut total;
    loop {
        let mut progress = false;
        total = 1usize;

        let dlen = s.dimord.len();
        for i in (1..dlen).rev() {
            let j = (s.rng.next() as usize) % (i + 1);
            s.dimord.swap(i, j);
        }

        for idx in 0..ndim_mutable {
            let d = s.dimord[idx];
            s.featord[0] = usize::MAX; // sentinel: empty best list
            let mut best_len = 0usize;
            let mut best_n = s.n_size[d][t.f[d] as usize];
            let mut coverage = count_tuples_for(&s.tu[d][t.f[d] as usize], &t.f);

            let dim_d = s.dim[d];
            for f in 0..dim_d {
                t.f[d] = f as u16;
                let ok = s.wc2.is_empty()
                    || count_withouts_pool(&t.f, &s.wc2, &s.wc[d]) == 0;
                if ok {
                    let new_coverage = count_tuples_for(&s.tu[d][f], &t.f);
                    let fn_ = s.n_size[d][f];
                    if fn_ < best_n {
                        best_n = fn_;
                        progress = true;
                        coverage = new_coverage;
                        best_len = 1;
                        s.featord[0] = f;
                    } else if fn_ == best_n && new_coverage >= coverage {
                        if new_coverage > coverage {
                            progress = true;
                            coverage = new_coverage;
                            best_len = 0;
                        }
                        s.featord[best_len] = f;
                        best_len += 1;
                    }
                }
            }

            if best_len == 0 {
                println!("jenny: internal error b");
            } else if best_len == 1 {
                t.f[d] = s.featord[0] as u16;
            } else {
                let pick = (s.rng.next() as usize) % best_len;
                t.f[d] = s.featord[pick] as u16;
            }

            if s.n_size[d][t.f[d] as usize] == n {
                total += coverage;
            }
        }

        if !progress {
            break;
        }
    }
    total
}

// -----------------------------------------------------------------------
// generate_test
// -----------------------------------------------------------------------

fn generate_test(s: &mut State, t: &mut Test, tuple: &[Feature], n: usize) -> usize {
    let mut mutable = vec![true; s.ndim];
    for fe in tuple {
        mutable[fe.d as usize] = false;
    }

    let wc2_empty = s.wc2.is_empty();

    for _iter in 0..MAX_ITERS {
        for i in 0..s.ndim {
            t.f[i] = (s.rng.next() as usize % s.dim[i]) as u16;
        }
        for fe in tuple {
            t.f[fe.d as usize] = fe.f;
        }
        if wc2_empty || obey_withouts(s, t, &mutable) {
            let w = count_withouts_list(&t.f, &s.wc2);
            if w != 0 {
                println!("internal error, {} withouts", w);
            }
            return maximize_coverage(s, t, &mutable, n);
        }
    }
    0
}

// -----------------------------------------------------------------------
// cover_tuples
// -----------------------------------------------------------------------

fn cover_tuples(s: &mut State) {
    let mut curr_test = Test::new(s.ndim);

    loop {
        let mut tuple_n = MAX_N;
        let mut tuple_count = 0usize;
        let mut tuple_d = 0usize;
        let mut tuple_f_idx = 0usize;

        for d in 0..s.ndim {
            for f in 0..s.dim[d] {
                build_tuples(s, d, f);
                let cnt = s.tu[d][f].count;
                let tn = s.n_size[d][f];
                if tn < tuple_n {
                    tuple_n = tn;
                    tuple_count = cnt;
                    tuple_d = d;
                    tuple_f_idx = f;
                } else if tn == tuple_n && cnt > tuple_count {
                    tuple_count = cnt;
                    tuple_d = d;
                    tuple_f_idx = f;
                }
            }
        }

        if tuple_count == 0 {
            if tuple_n == s.n_final {
                break;
            }
            continue;
        }

        let tuple_vec: Vec<Feature> = s.tu[tuple_d][tuple_f_idx].get(0).to_vec();
        let n = tuple_n;

        let mut best_test = Test::new(s.ndim);
        let mut best_count: i64 = -1;
        let mut covered = false;

        for _ in 0..GROUP_SIZE {
            let this_count = generate_test(s, &mut curr_test, &tuple_vec, n);
            if this_count == 0 { continue; }
            covered = true;
            if this_count as i64 > best_count {
                best_count = this_count as i64;
                std::mem::swap(&mut curr_test, &mut best_test);
            }
        }

        if !covered {
            let extra = tuple_vec.clone();
            print!("Could not cover tuple ");
            show_tuple(&extra);
            s.wc3.push(Without { fe: extra.clone() });

            for d in 0..s.ndim {
                for f in 0..s.dim[d] {
                    s.tu[d][f].retain_if(|tup| !subset_tuple(&extra, tup));
                }
            }
        } else {
            for d in 0..s.ndim {
                let f = best_test.f[d] as usize;
                let best_f = &best_test.f;
                s.tu[d][f].retain_if(|tup| !test_tuple(best_f, tup));
            }
            if !add_test(s, best_test) {
                println!("jenny: exceeded maximum number of tests");
                return;
            }
        }
    }
}

// -----------------------------------------------------------------------
// confirm
// -----------------------------------------------------------------------

fn confirm(s: &mut State) -> bool {
    let n = s.n_final;
    let ndim = s.ndim;
    let dim = s.dim.clone();

    let mut offset = vec![Feature::default(); n];
    for i in 0..n {
        offset[i].d = i as u16;
        offset[i].f = 0;
    }

    loop {
        for i in 0..n {
            s.tuple_tester.f[offset[i].d as usize] = offset[i].f;
        }

        if count_withouts_both(&s.tuple_tester.f, &s.wc2, &s.wc3) == 0 {
            let covered = s.tests.iter().any(|t| {
                offset.iter().all(|fe| t.f[fe.d as usize] == fe.f)
            });
            if !covered {
                println!("problem with {}{}",
                         offset[0].d as usize + 1,
                         FEATURE_NAME[offset[0].f as usize] as char);
                for i in 0..n {
                    s.tuple_tester.f[offset[i].d as usize] = u16::MAX;
                }
                return false;
            }
        }

        for i in 0..n {
            s.tuple_tester.f[offset[i].d as usize] = u16::MAX;
        }

        if !next_builder(&mut offset, n, ndim, &dim) {
            break;
        }
    }
    true
}

// -----------------------------------------------------------------------
// parse
// -----------------------------------------------------------------------

fn print_help() {
    print!(concat!(
        "jenny:\n",
        "  Given a set of feature dimensions and withouts, produce tests\n",
        "  covering all n-tuples of features where all features come from\n",
        "  different dimensions.  For example (=, <, >, <=, >=, !=) is a\n",
        "  dimension with 6 features.  The type of the left-hand argument is\n",
        "  another dimension.  Dimensions are numbered 1..65535, in the order\n",
        "  they are listed.  Features are implicitly named a..z, A..Z.\n",
        "   3 Dimensions are given by the number of features in that dimension.\n",
        "  -h prints out these instructions.\n",
        "  -n specifies the n in n-tuple.  The default is 2 (meaning pairs).\n",
        "  -e specifies coverage for negative testcases (one per without).\n",
        "     -e0 produces no negative testcases (default).  -e1 covers each\n",
        "     feature in each without, -e2 covers all pairs, etc.  For each\n",
        "     without, dimensions in the without are restricted to its features;\n",
        "     that without and all later withouts are removed; coverage level\n",
        "     is -e.  Negative testcases follow all positive testcases.\n",
        "  -w gives withouts.  -w1b4ab says that combining the second feature\n",
        "     of the first dimension with the first or second feature of the\n",
        "     fourth dimension is disallowed.\n",
        "  -ofoo.txt reads old jenny testcases from file foo.txt and extends them.\n\n",
        "  The output is a testcase per line, one feature per dimension per\n",
        "  testcase, followed by the list of all allowed tuples that jenny could\n",
        "  not reach.\n\n",
        "  Example: jenny -n3 3 2 2 -w2b3b 5 3 -w1c3b4ace5ac 8 2 2 3 2\n",
        "  This gives ten dimensions, asks that for any three dimensions all\n",
        "  combinations of features (one feature per dimension) be covered,\n",
        "  plus it asks that certain combinations of features\n",
        "  (like (1c,3b,4c,5c)) not be covered.\n\n",
    ));
}

fn parse(args: &[String], s: &mut State) -> bool {
    if FEATURE_NAME.len() != MAX_FEATURES {
        println!("feature_name length is wrong, {}", FEATURE_NAME.len());
        return false;
    }

    // count dimensions
    let ndim: usize = args.iter()
        .filter(|a| a.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false))
        .count();
    if ndim > MAX_DIMENSIONS {
        println!("jenny: maximum number of dimensions is {}.  {} is too many.",
                 MAX_DIMENSIONS, ndim);
        return false;
    }
    s.ndim = ndim;
    s.dim = vec![0; ndim];

    // read dimension lengths
    let mut j = 0;
    let mut testfile: Option<String> = None;
    for arg in args.iter() {
        if arg.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false) {
            let bytes = arg.as_bytes();
            let mut curr = 0;
            if let Token::Number(v) = parse_token(bytes, &mut curr) {
                if parse_token(bytes, &mut curr) != Token::End {
                    println!("jenny: something was trailing a dimension number");
                    return false;
                }
                if v > MAX_FEATURES {
                    println!("jenny: dimensions must be smaller than {}.  {} is too big.",
                             MAX_FEATURES, v);
                    return false;
                }
                if v < 2 {
                    println!("jenny: a dimension must have at least 2 features, not {}", v);
                    return false;
                }
                s.dim[j] = v;
                j += 1;
            }
        } else if arg.starts_with('-') && arg.len() > 1 && arg.as_bytes()[1] == b'h' {
            print_help();
            return false;
        }
    }

    // read flags (second pass)
    for arg in args.iter() {
        if !arg.starts_with('-') { continue; }
        let bytes = arg.as_bytes();
        if bytes.len() < 2 {
            println!("jenny: '-' by itself isn't a proper argument.");
            return false;
        }
        match bytes[1] {
            b'o' => testfile = Some(String::from_utf8_lossy(&bytes[2..]).into_owned()),
            b'n' => { if !parse_n(s, &bytes[2..]) { return false; } }
            b'e' => { if !parse_e(s, &bytes[2..]) { return false; } }
            b'w' => { if !parse_w(s, &bytes[2..]) { return false; } }
            b's' => { if !parse_s(s, &bytes[2..]) { return false; } }
            b'h' => {}
            c => {
                println!("jenny: legal arguments are numbers, -n, -e, -s, -w, -h, not -{}", c as char);
                return false;
            }
        }
    }

    if s.n_final > s.ndim {
        println!("jenny: {}-tuples are impossible with only {} dimensions",
                 s.n_final, s.ndim);
        return false;
    }

    preliminary(s);

    if let Some(tf) = testfile {
        if !load(s, &tf) { return false; }
    }

    true
}

// -----------------------------------------------------------------------
// generate_negative_tests
// -----------------------------------------------------------------------

fn generate_negative_tests(s: &mut State) {
    let e = s.e_final;
    if e == 0 { return; }

    let ndim = s.ndim;
    let dim = s.dim.clone();
    let orig_wc2 = s.wc2.clone();

    for wi in 0..orig_wc2.len() {
        let w = &orig_wc2[wi];

        // Collect the without's dimension indices and allowed features per dim.
        // w.fe is sorted by d, so we group runs of the same d.
        let mut without_dims: Vec<usize> = Vec::new();
        let mut allowed: Vec<Vec<u16>> = Vec::new();
        {
            let mut i = 0;
            while i < w.fe.len() {
                let start = i;
                let dim_d = w.fe[start].d;
                while i < w.fe.len() && w.fe[i].d == dim_d {
                    i += 1;
                }
                without_dims.push(dim_d as usize);
                allowed.push(w.fe[start..i].iter().map(|fe| fe.f).collect());
            }
        }
        let k = without_dims.len();
        let e_k = e.min(k);

        // Build k-dimensional sub-problem using only the without's dims and features.
        let mut ns = State::new();
        ns.ndim = k;
        ns.dim = (0..k).map(|j| allowed[j].len()).collect();
        ns.n_final = e_k;

        // Project withouts W_0..W_{wi-1} onto the k sub-dims.
        // Only include a without if all its dims appear in without_dims.
        // Remap (d, f) -> (sub_d, sub_f); skip the without if any dim's features
        // are entirely outside the allowed set (the without can never match).
        for w_prev in &orig_wc2[..wi] {
            let mut proj: Vec<Feature> = Vec::new();
            let mut ok = true;
            let mut i = 0;
            while ok && i < w_prev.fe.len() {
                let start = i;
                let dim_d = w_prev.fe[start].d as usize;
                let sub_d = match without_dims.iter().position(|&d| d == dim_d) {
                    Some(j) => j,
                    None => { ok = false; break; }
                };
                let mut dim_proj: Vec<u16> = Vec::new();
                while i < w_prev.fe.len() && w_prev.fe[i].d == w_prev.fe[start].d {
                    let f = w_prev.fe[i].f;
                    if let Some(sub_f) = allowed[sub_d].iter().position(|&af| af == f) {
                        dim_proj.push(sub_f as u16);
                    }
                    i += 1;
                }
                if dim_proj.is_empty() {
                    // No available features match; without can never fire in sub-problem.
                    ok = false;
                } else {
                    for sub_f in dim_proj {
                        proj.push(Feature { d: sub_d as u16, f: sub_f });
                    }
                }
            }
            if ok {
                proj.sort_by(|a, b| a.d.cmp(&b.d).then(a.f.cmp(&b.f)));
                ns.wc2.push(Without { fe: proj });
            }
        }

        std::mem::swap(&mut ns.rng, &mut s.rng);
        preliminary(&mut ns);
        cover_tuples(&mut ns);
        std::mem::swap(&mut ns.rng, &mut s.rng);

        // Temporarily restrict s to withouts W_0..W_{wi-1} for obey_withouts
        // when filling the non-without dimensions of each expanded test.
        let saved_wc2 = s.wc2.clone();
        let saved_wc3 = s.wc3.clone();
        s.wc2 = orig_wc2[..wi].to_vec();
        s.wc3 = Vec::new();
        s.wc = vec![Vec::new(); ndim];
        for (wii, w2) in s.wc2.iter().enumerate() {
            let mut old_d: Option<u16> = None;
            for fe in &w2.fe {
                if Some(fe.d) != old_d {
                    s.wc[fe.d as usize].push(wii);
                    old_d = Some(fe.d);
                }
            }
        }

        // Expand each sub-test to a full-dimensional test.
        for sub_test in &ns.tests {
            // Fix the without's dims; other dims are mutable.
            let mut mutable = vec![true; ndim];
            for &wd in &without_dims {
                mutable[wd] = false;
            }

            let mut t = Test::new(ndim);
            // Set without dims from sub-test, remapping feature indices.
            for (j, &wd) in without_dims.iter().enumerate() {
                t.f[wd] = allowed[j][sub_test.f[j] as usize];
            }

            // Randomly assign other dims, then satisfy surviving withouts.
            let mut reported = false;
            for _iter in 0..MAX_ITERS {
                for d in 0..ndim {
                    if mutable[d] {
                        t.f[d] = (s.rng.next() as usize % dim[d]) as u16;
                    }
                }
                if s.wc2.is_empty() || obey_withouts(s, &mut t, &mutable) {
                    report(&t, ndim);
                    reported = true;
                    break;
                }
            }
            if !reported {
                // Best effort: report even if surviving withouts could not be satisfied.
                report(&t, ndim);
            }
        }

        // Restore withouts.
        s.wc2 = saved_wc2;
        s.wc3 = saved_wc3;
        s.wc = vec![Vec::new(); ndim];
        for (wii, w2) in s.wc2.iter().enumerate() {
            let mut old_d: Option<u16> = None;
            for fe in &w2.fe {
                if Some(fe.d) != old_d {
                    s.wc[fe.d as usize].push(wii);
                    old_d = Some(fe.d);
                }
            }
        }
    }
}

// -----------------------------------------------------------------------
// main
// -----------------------------------------------------------------------

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut s = State::new();
    if parse(&args, &mut s) {
        cover_tuples(&mut s);
        if confirm(&mut s) {
            for i in 0..s.tests.len() {
                report(&s.tests[i], s.ndim);
            }
        } else {
            println!("jenny: internal error, some tuples not covered");
        }
        generate_negative_tests(&mut s);
    }
}
