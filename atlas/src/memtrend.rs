//! Pure trend calculations for Atlas's bounded memory samples, used by the
//! health view to distinguish ordinary variation from sustained growth.

use crate::memwatch::Sample;

const HOUR: u64 = 60 * 60;
const WINDOW: u64 = 6 * HOUR;
const MIN_SPAN: u64 = 3 * HOUR;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Verdict {
    InsufficientData,
    Steady,
    Watch,
    Growing,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Trend {
    pub verdict: Verdict,
    pub samples: usize,
    pub span_seconds: u64,
    pub private_mb_per_hour: Option<f64>,
    pub r_squared: Option<f64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Presentation {
    pub summary: String,
    pub sparkline: Option<String>,
}

/// Fit a least-squares line to private bytes from the most recent six hours.
/// `Growing` requires at least three hours of observations, >20 MB/h, and
/// R² > 0.6; a short-lived startup peak cannot trigger it.
pub fn private_memory(samples: &[Sample], now_unix: u64) -> Trend {
    let start = now_unix.saturating_sub(WINDOW);
    let mut points: Vec<(f64, f64)> = samples
        .iter()
        .filter_map(|sample| {
            let bytes = sample.private_bytes?;
            (sample.at_unix >= start && sample.at_unix <= now_unix)
                .then_some((sample.at_unix as f64, bytes as f64 / 1_048_576.0))
        })
        .collect();
    points.sort_by(|a, b| a.0.total_cmp(&b.0));
    let Some(first) = points.first() else {
        return empty(points.len(), 0);
    };
    let Some(last) = points.last() else {
        return empty(0, 0);
    };
    let span_seconds = (last.0 - first.0).max(0.0) as u64;
    if points.len() < 2 || span_seconds < MIN_SPAN {
        return empty(points.len(), span_seconds);
    }

    // Center x around the first sample to avoid precision loss with epoch time.
    let mean_x = points.iter().map(|p| p.0 - first.0).sum::<f64>() / points.len() as f64;
    let mean_y = points.iter().map(|p| p.1).sum::<f64>() / points.len() as f64;
    let (cov, var_x, var_y) = points.iter().fold((0.0, 0.0, 0.0), |(cov, vx, vy), p| {
        let dx = p.0 - first.0 - mean_x;
        let dy = p.1 - mean_y;
        (cov + dx * dy, vx + dx * dx, vy + dy * dy)
    });
    if var_x == 0.0 {
        return empty(points.len(), span_seconds);
    }
    let slope_mb_per_hour = cov / var_x * HOUR as f64;
    let r_squared = if var_y == 0.0 {
        1.0
    } else {
        (cov * cov / (var_x * var_y)).clamp(0.0, 1.0)
    };
    let verdict = if slope_mb_per_hour < 10.0 {
        Verdict::Steady
    } else if slope_mb_per_hour > 20.0 && r_squared > 0.6 {
        Verdict::Growing
    } else {
        Verdict::Watch
    };
    Trend {
        verdict,
        samples: points.len(),
        span_seconds,
        private_mb_per_hour: Some(slope_mb_per_hour),
        r_squared: Some(r_squared),
    }
}

/// A compact, truthful Status-page summary plus a decorative sparkline. The
/// text carries the verdict and units; assistive technology can skip the bars.
pub fn presentation(samples: &[Sample], now_unix: u64) -> Presentation {
    let trend = private_memory(samples, now_unix);
    let latest = samples
        .iter()
        .filter(|sample| sample.private_bytes.is_some() && sample.at_unix <= now_unix)
        .max_by_key(|sample| sample.at_unix);
    let sparkline = private_sparkline(samples, now_unix, 24);
    let Some(latest) = latest else {
        return Presentation {
            summary:
                "Collecting — no readable Atlas memory samples yet; 3 hours are needed for a trend."
                    .into(),
            sparkline,
        };
    };
    let age = now_unix.saturating_sub(latest.at_unix);
    let freshness = if age > 120 {
        format!("Stale — last sample {} ago", duration(age))
    } else {
        format!("last sample {} ago", duration(age))
    };
    let state = match trend.verdict {
        Verdict::InsufficientData => format!(
            "Collecting — {:.1} of 3 hours, {} samples",
            trend.span_seconds as f64 / 3600.0,
            trend.samples
        ),
        Verdict::Steady => format!(
            "Steady — {:+.1} MB/h over {:.1} hours",
            trend.private_mb_per_hour.unwrap_or(0.0),
            trend.span_seconds as f64 / 3600.0
        ),
        Verdict::Watch => format!(
            "Watch — {:+.1} MB/h over {:.1} hours",
            trend.private_mb_per_hour.unwrap_or(0.0),
            trend.span_seconds as f64 / 3600.0
        ),
        Verdict::Growing => format!(
            "Growing — {:+.1} MB/h over {:.1} hours",
            trend.private_mb_per_hour.unwrap_or(0.0),
            trend.span_seconds as f64 / 3600.0
        ),
    };
    Presentation {
        summary: format!("Private memory · {state} · {freshness}"),
        sparkline,
    }
}

/// Scale the recent six-hour private-memory samples into `width` block bars.
/// Missing values are omitted; a flat series stays visibly flat.
pub fn private_sparkline(samples: &[Sample], now_unix: u64, width: usize) -> Option<String> {
    if width == 0 {
        return None;
    }
    let start = now_unix.saturating_sub(WINDOW);
    let mut points: Vec<(u64, f64)> = samples
        .iter()
        .filter_map(|sample| {
            let bytes = sample.private_bytes?;
            (sample.at_unix >= start && sample.at_unix <= now_unix)
                .then_some((sample.at_unix, bytes as f64 / 1_048_576.0))
        })
        .collect();
    points.sort_by_key(|point| point.0);
    if points.is_empty() {
        return None;
    }
    let count = points.len().min(width);
    let values: Vec<f64> = (0..count)
        .map(|bucket| {
            let from = bucket * points.len() / count;
            let to = ((bucket + 1) * points.len() / count).max(from + 1);
            points[from..to].iter().map(|point| point.1).sum::<f64>() / (to - from) as f64
        })
        .collect();
    let min = values.iter().copied().fold(f64::INFINITY, f64::min);
    let max = values.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    const BARS: [char; 8] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'];
    Some(
        values
            .iter()
            .map(|value| {
                let level = if max == min {
                    3
                } else {
                    (((value - min) / (max - min) * 7.0).round() as usize).min(7)
                };
                BARS[level]
            })
            .collect(),
    )
}

fn duration(seconds: u64) -> String {
    if seconds < 60 {
        format!("{seconds}s")
    } else if seconds < 3600 {
        format!("{}m", seconds / 60)
    } else {
        format!("{}h {}m", seconds / 3600, (seconds % 3600) / 60)
    }
}

fn empty(samples: usize, span_seconds: u64) -> Trend {
    Trend {
        verdict: Verdict::InsufficientData,
        samples,
        span_seconds,
        private_mb_per_hour: None,
        r_squared: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn series(mb_per_hour: f64, count: usize) -> Vec<Sample> {
        (0..count)
            .map(|i| Sample {
                record_type: "sample".into(),
                at_unix: 1_000_000 + i as u64 * 1800,
                process_id: 1,
                private_bytes: Some(((500.0 + mb_per_hour * i as f64 / 2.0) * 1_048_576.0) as u64),
                working_set_bytes: None,
                peak_working_set_bytes: None,
                pagefile_bytes: None,
                handles: None,
                threads: None,
                gdi_objects: None,
                user_objects: None,
                system_commit_bytes: None,
                system_available_bytes: None,
                related_processes: Vec::new(),
            })
            .collect()
    }

    #[test]
    fn requires_three_hours_and_classifies_slopes() {
        assert_eq!(
            private_memory(&series(30.0, 5), 1_020_000).verdict,
            Verdict::InsufficientData
        );
        assert_eq!(
            private_memory(&series(0.0, 7), 1_020_000).verdict,
            Verdict::Steady
        );
        assert_eq!(
            private_memory(&series(15.0, 7), 1_020_000).verdict,
            Verdict::Watch
        );
        assert_eq!(
            private_memory(&series(30.0, 7), 1_020_000).verdict,
            Verdict::Growing
        );
    }

    #[test]
    fn flat_series_has_defined_fit_quality() {
        let trend = private_memory(&series(0.0, 7), 1_020_000);
        assert_eq!(trend.r_squared, Some(1.0));
        assert_eq!(trend.private_mb_per_hour, Some(0.0));
    }

    #[test]
    fn accepts_samples_out_of_order() {
        let mut samples = series(30.0, 7);
        samples.reverse();
        assert_eq!(
            private_memory(&samples, 1_020_000).verdict,
            Verdict::Growing
        );
    }

    #[test]
    fn status_presentation_is_honest_about_collection_and_staleness() {
        let fresh = series(30.0, 3);
        let starting = presentation(&fresh, 1_003_600);
        assert!(starting.summary.contains("Collecting"));
        assert!(starting.summary.contains("Private memory"));
        assert!(starting.sparkline.is_some());

        let stale = presentation(&fresh, 1_004_000);
        assert!(stale.summary.contains("Stale — last sample"));

        let empty = presentation(&[], 1_020_000);
        assert!(empty.summary.contains("no readable Atlas memory samples"));
        assert!(empty.sparkline.is_none());
    }

    #[test]
    fn sparkline_is_bounded_and_flat_data_stays_flat() {
        let flat = series(0.0, 720);
        let chart = private_sparkline(&flat, 1_020_000, 24).unwrap();
        assert_eq!(chart.chars().count(), 12);
        assert!(chart.chars().all(|ch| ch == '▄'));
        assert_eq!(private_sparkline(&flat, 1_020_000, 0), None);
    }
}
