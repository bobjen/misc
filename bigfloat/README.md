# bigfloat

Arbitrary-precision floating-point library.

Written by Bob Jenkins because MPFR was too difficult to get working at the
time (public domain).  Rust translation in `src/`.

Documentation for the original C++: https://burtleburtle.net/bob/math/bigfloat.html

## Precision

By default 20 base-2^32 digits (~193 decimal digits).  Recompile with
`C_DIGITS` and `C_LOG` adjusted for more or less precision.

## Operations

Arithmetic: `add`, `sub`, `mul`, `div`, `sqrt`, `power`, `power_int`

Transcendental: `exp`, `ln`, `log`, `sin`, `cos`, `tan`, `csc`, `sec`,
`a_sin`, `a_cos`, `a_tan`

Constants: `pi()`, `e_const()`

Random: `rand` (uniform), `rand_norm` (normal distribution)

Linear algebra: `gaussian_elimination`

Conversions: `to_double`, `to_integer`, `to_fraction`,
`print_decimal`, `print_hex`, `print_continued_fraction`

Special values: `±0`, `±inf`, `NaN`

## Example

```rust
use bigfloat::BigFloat;

// Solve 2×2 system via Gaussian elimination
let mut m = vec![
    vec![BigFloat::from_int(1), BigFloat::from_int(1), BigFloat::from_int(2)],
    vec![BigFloat::from_int(1), BigFloat::from_int(2), BigFloat::from_int(3)],
];
BigFloat::gaussian_elimination(&mut m, 2, 2);

println!("{}", BigFloat::pi().to_double());   // π to ~193 decimal digits
```

## Building

```
cargo build --release
cargo test --features bigfloat-test
```
