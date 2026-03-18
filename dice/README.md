# dice

Exhaustive-enumeration framework: a poor man's Prolog.

A program calls `dice.choose(n)` to make a choice in `0..n-1`.  The
framework runs the program repeatedly, feeding it every possible combination
of choices in sequence.  The program never needs to know it is being
enumerated — it just makes choices and acts on them.

Original C++ implementation by Bob Jenkins (public domain).
Rust translation in `src/`.

Documentation: https://burtleburtle.net/bob/testing/dice.html

## How it works

The framework maintains a bit-vector of state.  Each call to `choose(n)`
reads `ceil(log2(n))` bits from that vector and interprets them as the
choice.  Between runs the vector is incremented like a binary counter,
stepping through all possible bit patterns.  Choices that would exceed
`n-1` are clamped, which causes some bit patterns to produce duplicate
results; those duplicates are skipped efficiently by the carry propagation.

## Example

The included example searches for five 3s in a row on a six-sided die.
It calls `choose(6)` five times per run; the framework drives it through
all 6^5 = 7776 combinations and prints the ones where every roll is 3
(there is exactly one such sequence).

```rust
fn threes(d: &mut Dice) {
    for _ in 0..5 {
        let x = d.choose(6) + 1;
        print!("{} ", x);
        if x != 3 { println!(); return; }
    }
    println!("Yes, it is possible to roll 3 five times in a row!");
}
```

## Building

```
cargo build --release
```
