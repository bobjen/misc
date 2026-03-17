// dice: enumerate all possible dice rolls looking for ten 3s in a row.
// Translated from dice.cpp by Bob Jenkins.
//
// Dice allows a user program to call dice.choose(n) to make choices, but
// the user program will be called for all legal combinations of choices.
// It's a poor man's Prolog.  Avoid calling choose(0).

const BITS_PER_U64: u64 = 64;
const STATE_SIZE: usize = 1024;
const TOTAL_BITS: u64 = BITS_PER_U64 * STATE_SIZE as u64;

/// Enumerates all possible combinations of choices made via `choose`.
/// Call `exec` with a callback; the callback will be invoked once per
/// distinct combination of choice values.
struct Dice {
    state: [u64; STATE_SIZE],
    offset: u64,
}

impl Dice {
    fn new() -> Self {
        Dice {
            state: [0u64; STATE_SIZE],
            offset: 0,
        }
    }

    // Set logn bits starting at offset to 1 (so the next Inc() carries past them).
    fn mask(&mut self, logn: u64) {
        let index = (self.offset / BITS_PER_U64) as usize;
        let shift = (self.offset % BITS_PER_U64) as u32;
        let mask = (1u64 << logn) - 1;
        self.state[index] |= mask.wrapping_shl(shift);
        if BITS_PER_U64 - (shift as u64) < logn {
            self.state[index + 1] |= mask >> (BITS_PER_U64 - (shift as u64));
        }
    }

    // Read logn bits starting at offset.
    fn read_bits(&self, logn: u64) -> u64 {
        let index = (self.offset / BITS_PER_U64) as usize;
        let shift = self.offset % BITS_PER_U64;
        let mask = (1u64 << logn) - 1;
        let mut result = (self.state[index] >> shift) & mask;
        if BITS_PER_U64 - shift < logn {
            result |= (self.state[index + 1] & (mask >> (BITS_PER_U64 - shift)))
                << (BITS_PER_U64 - shift);
        }
        result
    }

    // Increment the state at offset. Returns true if we wrap around (all combinations done).
    fn inc(&mut self) -> bool {
        let mut index = (self.offset / BITS_PER_U64) as usize;
        let shift = (self.offset % BITS_PER_U64) as u32;
        self.state[index] = self.state[index].wrapping_add(1u64 << shift);
        while self.state[index] == 0 {
            index += 1;
            if index >= STATE_SIZE {
                return true;
            }
            self.state[index] = self.state[index].wrapping_add(1);
        }
        false
    }

    /// Choose a value in 0..n-1.  Avoid calling choose(0).
    fn choose(&mut self, n: u64) -> u64 {
        if n > 0x8000_0000_0000_0000u64 {
            eprintln!("Error: Can't do choose({}), {} is too big", n, n);
            std::process::exit(1);
        }

        // Round n up to the next power of two; use that many bits.
        let mut logn = 0u64;
        while (1u64 << logn) < n {
            logn += 1;
        }

        if self.offset < logn {
            eprintln!("Error: Too many dice rolls");
            std::process::exit(1);
        }

        self.offset -= logn;
        let mut result = self.read_bits(logn);

        if result >= n - 1 {
            // If we've reached n-1, mask the bits to all 1s so Inc() carries
            // correctly past them.  Values above n-1 are clamped to n-1.
            if result == n - 1 {
                self.mask(logn);
            } else {
                result = n - 1;
            }
        }

        result
    }

    /// Call `callback` for every possible set of choices.
    fn exec<F: FnMut(&mut Dice)>(&mut self, mut callback: F) {
        loop {
            self.offset = TOTAL_BITS;
            callback(self);
            if self.inc() {
                break;
            }
        }
    }
}

fn threes(d: &mut Dice) {
    for _ in 0..5 {
        let x = d.choose(6) + 1;
        print!("{} ", x);
        if x != 3 {
            println!();
            return;
        }
    }
    println!();
    println!("Yes, it is possible to roll 3 five times in a row!");
}

fn main() {
    let mut d = Dice::new();
    d.exec(threes);
}
