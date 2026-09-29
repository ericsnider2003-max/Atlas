//! Choosing between options while still learning which is best: Thompson
//! sampling over Beta posteriors.
//!
//! **Sources:** Thompson (1933); Chapelle & Li (2011), *An Empirical
//! Evaluation of Thompson Sampling*; Russo et al. (2018), *A Tutorial on
//! Thompson Sampling* §3 (Beta–Bernoulli). Each option keeps a Beta(1 + yes,
//! 1 + no) belief about how often it is welcome; to choose, draw one sample
//! from each belief and take the highest. Beta draws come from two Gamma
//! draws (Marsaglia & Tsang, 2000, *A Simple Method for Generating Gamma
//! Variables*). Clean-room.
//!
//! **Why Atlas wants it.** When two proactive offers clear every bar at once,
//! `proactive::consider` took the more confident one — every time. An offer
//! kind you would welcome but that is never the most confident never got
//! raised, so its record never grew, so it never got raised: learning stops
//! exactly where it is needed. Sampling from the belief still favours what
//! you have welcomed, and still tries the others often enough to find out.

/// A small, seedable generator (SplitMix64) so a choice can be reproduced.
#[derive(Debug, Clone)]
pub struct Rng(pub u64);

impl Rng {
    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    fn unit(&mut self) -> f64 {
        ((self.next_u64() >> 11) as f64 + 0.5) / (1u64 << 53) as f64
    }
    fn normal(&mut self) -> f64 {
        let (u, v) = (self.unit(), self.unit());
        (-2.0 * u.ln()).sqrt() * (2.0 * std::f64::consts::PI * v).cos()
    }
    fn gamma(&mut self, shape: f64) -> f64 {
        if shape < 1.0 {
            // Boost: Gamma(a) = Gamma(a+1) · U^(1/a).
            return self.gamma(shape + 1.0) * self.unit().powf(1.0 / shape);
        }
        let d = shape - 1.0 / 3.0;
        let c = 1.0 / (9.0 * d).sqrt();
        loop {
            let x = self.normal();
            let v = (1.0 + c * x).powi(3);
            if v <= 0.0 {
                continue;
            }
            let u = self.unit();
            if u < 1.0 - 0.0331 * x.powi(4) || u.ln() < 0.5 * x * x + d * (1.0 - v + v.ln()) {
                return d * v;
            }
        }
    }
    /// One draw from Beta(a, b).
    fn beta(&mut self, a: f64, b: f64) -> f64 {
        let x = self.gamma(a);
        let y = self.gamma(b);
        x / (x + y)
    }
}

/// Pick one of `options`, each given as (yes, no) counts. Returns the index.
pub fn pick(options: &[(u32, u32)], rng: &mut Rng) -> Option<usize> {
    options
        .iter()
        .enumerate()
        .map(|(i, (y, n))| (i, rng.beta(1.0 + *y as f64, 1.0 + *n as f64)))
        .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
        .map(|(i, _)| i)
}
