# jenny

Covering-array test-case generator.  Given a set of dimensions (parameters)
each with a fixed number of features (values), jenny generates a compact set
of test cases that covers every n-tuple of features drawn from n distinct
dimensions.

Original C implementation by Bob Jenkins (March 2003, public domain).
Rust translation in `src/`.

Documentation and theory: https://burtleburtle.net/bob/math/jenny.html

## What it does

A covering array for n=2 (pairs) guarantees that every combination of any
two parameter values appears in at least one test case.  For n=3 every
triplet is covered, and so on.  The number of test cases grows much more
slowly than exhaustive testing: covering all pairs of 10 parameters with
10 values each takes roughly 25 tests instead of 10^10.

## Negative test cases (-e)

jenny is believed to be the first covering-array tool to generate negative
test cases.  The `-w` flag marks combinations that are *forbidden* (e.g.
invalid configurations).  The `-e` flag then generates additional test cases
that deliberately land in those forbidden zones, so you can verify that your
system correctly rejects them.

- `-e0` — no negative tests (default)
- `-e1` — one negative test per forbidden combination
- `-eN` — up to N negative tests per forbidden combination

## Usage

```
jenny [options] dim1 dim2 ...
```

Each `dimN` is the number of features (values) in that parameter, 2..52.

Options:
- `-n N`     cover all N-tuples (default 2)
- `-s SEED`  random seed (last `-s` wins)
- `-w SPEC`  forbidden combination (see below)
- `-e N`     generate N negative test cases per without
- `-o FILE`  read existing tests from FILE and extend them
- `-h`       help

A `-w` spec lists dimension-feature pairs: `-w1a2bc` forbids any test
where dimension 1 has feature `a` AND dimension 2 has feature `b` or `c`.
Features are named `a`..`z`, `A`..`Z` (up to 52 per dimension).

## Example

```
jenny -n3 3 3 3 3 3 -w1a2b -e2
```

Covers all triplets of a 5-dimension problem with one forbidden pair,
and generates up to 2 negative tests exercising that forbidden pair.

## Building

```
cargo build --release          # Rust
cc -O2 -o jenny jenny.c        # C
```

## Test suite

```
bash jenny_test.sh | diff - jenny_test.expected
```
