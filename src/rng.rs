//! A tiny deterministic random number generator (SplitMix64).
//!
//! Kept in-crate so a seed produces the same building on every platform and
//! every version of our dependencies.

#[derive(Clone, Debug)]
pub struct Rng {
    state: u64,
}

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self {
            state: seed ^ 0x5DEE_CE66_D1CE_4E5B,
        }
    }

    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// Uniform in `[0, 1)`.
    pub fn next_f64(&mut self) -> f64 {
        (self.next_u64() >> 11) as f64 / (1u64 << 53) as f64
    }

    /// Uniform in `[lo, hi)`.
    pub fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.next_f64()
    }

    /// Uniform index in `0..n`. `n` must be positive.
    pub fn below(&mut self, n: usize) -> usize {
        assert!(n > 0, "Rng::below needs a positive bound");
        (self.next_u64() % n as u64) as usize
    }

    pub fn chance(&mut self, probability: f64) -> bool {
        self.next_f64() < probability
    }

    pub fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len())]
    }
}

#[cfg(test)]
mod tests {
    use super::Rng;

    #[test]
    fn same_seed_same_sequence() {
        let mut a = Rng::new(7);
        let mut b = Rng::new(7);
        for _ in 0..100 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
    }

    #[test]
    fn floats_stay_in_range() {
        let mut rng = Rng::new(1);
        for _ in 0..10_000 {
            let v = rng.range(2.0, 3.0);
            assert!((2.0..3.0).contains(&v));
        }
    }
}
