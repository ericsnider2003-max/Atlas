//! A diagonal Gaussian mixture over cepstral frames, and the "supervector"
//! of how one clip moves it (Reynolds, Quatieri & Dunn 2000; Campbell et al.
//! 2006 for the supervector).
//!
//! Why this and not just averaging a clip's cepstra (tried first, round 5):
//! a two-second clip's average depends as much on *what* was said as on
//! *who* said it. A mixture fitted to all the speech at hand gives each
//! component a rough class of sound; adapting it to one clip moves each
//! component only as far as that clip has evidence for, so two clips are
//! compared sound-class by sound-class — like with like.
#![allow(clippy::needless_range_loop, reason = "numeric kernels step through several arrays by one index; the index loop is the clear form")]

use crate::mfcc::CEPS;

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Gmm {
    pub w: Vec<f32>,
    pub mu: Vec<[f32; CEPS]>,
    pub var: Vec<[f32; CEPS]>,
}

impl Gmm {
    /// Fit `k` components to `frames` by EM, started from evenly spaced frames.
    pub fn fit(frames: &[[f32; CEPS]], k: usize, iters: usize) -> Option<Gmm> {
        if frames.len() < k * 10 || k == 0 {
            return None;
        }
        let mut g = Gmm {
            w: vec![1.0 / k as f32; k],
            mu: (0..k).map(|i| frames[i * frames.len() / k]).collect(),
            var: vec![global_var(frames); k],
        };
        for _ in 0..iters {
            let mut n = vec![0f32; k];
            let mut s1 = vec![[0f32; CEPS]; k];
            let mut s2 = vec![[0f32; CEPS]; k];
            for x in frames {
                let post = g.posteriors(x);
                for c in 0..k {
                    let p = post[c];
                    if p < 1e-6 {
                        continue;
                    }
                    n[c] += p;
                    for d in 0..CEPS {
                        s1[c][d] += p * x[d];
                        s2[c][d] += p * x[d] * x[d];
                    }
                }
            }
            let total: f32 = n.iter().sum();
            for c in 0..k {
                if n[c] < 1e-3 {
                    continue;
                }
                g.w[c] = n[c] / total;
                for d in 0..CEPS {
                    let m = s1[c][d] / n[c];
                    g.mu[c][d] = m;
                    g.var[c][d] = (s2[c][d] / n[c] - m * m).max(1e-3);
                }
            }
        }
        Some(g)
    }

    fn posteriors(&self, x: &[f32; CEPS]) -> Vec<f32> {
        let logs: Vec<f32> = (0..self.w.len())
            .map(|c| {
                let mut l = self.w[c].max(1e-12).ln();
                for d in 0..CEPS {
                    let v = self.var[c][d];
                    let diff = x[d] - self.mu[c][d];
                    l -= 0.5 * (diff * diff / v + v.ln());
                }
                l
            })
            .collect();
        let m = logs.iter().cloned().fold(f32::MIN, f32::max);
        let e: Vec<f32> = logs.iter().map(|l| (l - m).exp()).collect();
        let s: f32 = e.iter().sum();
        e.iter().map(|v| v / s).collect()
    }

    /// How `frames` move the means (maximum a posteriori, relevance `r`),
    /// scaled so plain cosine between two supervectors is meaningful.
    pub fn supervector(&self, frames: &[[f32; CEPS]], r: f32) -> Vec<f32> {
        let k = self.w.len();
        let mut n = vec![0f32; k];
        let mut s1 = vec![[0f32; CEPS]; k];
        for x in frames {
            let post = self.posteriors(x);
            for c in 0..k {
                n[c] += post[c];
                for d in 0..CEPS {
                    s1[c][d] += post[c] * x[d];
                }
            }
        }
        let mut out = Vec::with_capacity(k * CEPS);
        for c in 0..k {
            let a = n[c] / (n[c] + r);
            for d in 0..CEPS {
                let ex = if n[c] > 0.0 { s1[c][d] / n[c] } else { self.mu[c][d] };
                let adapted = a * ex + (1.0 - a) * self.mu[c][d];
                out.push((adapted - self.mu[c][d]) * self.w[c].sqrt() / self.var[c][d].sqrt());
            }
        }
        out
    }
}

fn global_var(frames: &[[f32; CEPS]]) -> [f32; CEPS] {
    let n = frames.len() as f32;
    let mut m = [0f32; CEPS];
    let mut v = [0f32; CEPS];
    for x in frames {
        for d in 0..CEPS {
            m[d] += x[d] / n;
        }
    }
    for x in frames {
        for d in 0..CEPS {
            v[d] += (x[d] - m[d]).powi(2) / n;
        }
    }
    v.map(|x| x.max(1e-3))
}
