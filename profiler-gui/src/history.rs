//! Short sensor history for the System Monitor charts.
//!
//! Samples arrive once a second. The longest window is kept in a ring
//! buffer and charts are drawn as SVG-style path strings on a fixed
//! 300 x 100 viewbox, downsampled so a chart never has more than
//! [`MAX_POINTS`] points.

use std::collections::VecDeque;
use std::fmt::Write;

/// Longest selectable window, in samples (15 min at 1 Hz).
pub const CAPACITY: usize = 15 * 60;
/// Points per chart; about one per 2 px at typical widths.
pub const MAX_POINTS: usize = 300;
const VIEW_W: f32 = 300.0;
const VIEW_H: f32 = 100.0;

/// The charted series, in display order.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Sample {
    pub cpu_temp_c: f32,
    pub cpu_usage: f32,
    pub package_w: f32,
    pub fan_rpm: f32,
}

#[derive(Debug, Default)]
pub struct History {
    samples: VecDeque<Sample>,
}

impl History {
    pub fn push(&mut self, sample: Sample) {
        if self.samples.len() == CAPACITY {
            self.samples.pop_front();
        }
        self.samples.push_back(sample);
    }

    /// The last `window` values of one series, oldest first.
    pub fn series(&self, window: usize, pick: impl Fn(&Sample) -> f32) -> Vec<f32> {
        let skip = self.samples.len().saturating_sub(window);
        self.samples.iter().skip(skip).map(pick).collect()
    }
}

/// Average consecutive values so at most `max` remain.
fn downsample(values: &[f32], max: usize) -> Vec<f32> {
    if values.len() <= max || max == 0 {
        return values.to_vec();
    }
    let per = values.len().div_ceil(max);
    values
        .chunks(per)
        .map(|c| {
            #[allow(clippy::cast_precision_loss)] // chunk sizes are tiny
            let n = c.len() as f32;
            c.iter().sum::<f32>() / n
        })
        .collect()
}

/// Line and filled-area path commands for `values` drawn between `lo`
/// and `hi`. The newest value sits at the right edge; a window that is
/// not yet full leaves the left side empty. Returns empty strings when
/// there is nothing to draw.
pub fn chart_paths(values: &[f32], window: usize, lo: f32, hi: f32) -> (String, String) {
    let slots = window.clamp(2, MAX_POINTS);
    let shown = downsample(values, MAX_POINTS.min(window.max(1)));
    // Map the window onto the viewbox; downsampled data uses the same
    // proportion of the width as the raw samples it came from.
    #[allow(clippy::cast_precision_loss)] // small counts
    let (used, step) = (
        values.len().min(window) as f32 / window.max(1) as f32,
        VIEW_W / (slots - 1) as f32,
    );
    if shown.len() < 2 {
        return (String::new(), String::new());
    }
    #[allow(clippy::cast_precision_loss)]
    let span = (shown.len() - 1) as f32;
    let width = (used * VIEW_W).min(VIEW_W).max(step);
    let range = (hi - lo).max(f32::EPSILON);

    let mut line = String::with_capacity(shown.len() * 14);
    let mut first_x = VIEW_W;
    for (i, v) in shown.iter().enumerate() {
        #[allow(clippy::cast_precision_loss)]
        let x = VIEW_W - width + width * (i as f32 / span);
        let y = VIEW_H - ((v - lo) / range).clamp(0.0, 1.0) * VIEW_H;
        if i == 0 {
            first_x = x;
        }
        let _ = write!(line, "{}{x:.1} {y:.1}", if i == 0 { "M" } else { " L" });
    }
    let area = format!("{line} L{VIEW_W} {VIEW_H} L{first_x:.1} {VIEW_H} Z");
    (line, area)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ring_buffer_keeps_newest() {
        let mut h = History::default();
        #[allow(clippy::cast_precision_loss)]
        for i in 0..(CAPACITY + 10) {
            h.push(Sample {
                cpu_usage: i as f32,
                ..Sample::default()
            });
        }
        let s = h.series(3, |s| s.cpu_usage);
        #[allow(clippy::cast_precision_loss)]
        let last = (CAPACITY + 9) as f32;
        assert_eq!(
            s,
            [
                last - 2.0,
                last - 1.0,
                last
            ]
        );
        assert_eq!(h.series(usize::MAX, |s| s.cpu_usage).len(), CAPACITY);
    }

    #[test]
    fn downsampling_averages() {
        assert_eq!(
            downsample(
                &[
                    1.0, 3.0, 5.0, 7.0
                ],
                2
            ),
            [2.0, 6.0]
        );
        assert_eq!(downsample(&[1.0, 2.0], 5), [1.0, 2.0]);
    }

    #[test]
    fn full_window_spans_the_width() {
        let values: Vec<f32> = vec![
            0.0, 50.0, 100.0,
        ];
        let (line, area) = chart_paths(&values, 3, 0.0, 100.0);
        assert_eq!(line, "M0.0 100.0 L150.0 50.0 L300.0 0.0");
        assert_eq!(area, format!("{line} L300 100 L0.0 100 Z"));
    }

    #[test]
    fn partial_window_hugs_the_right_edge() {
        let (line, _) = chart_paths(
            &[
                10.0, 10.0,
            ],
            60,
            0.0,
            100.0,
        );
        assert!(line.ends_with("L300.0 90.0"), "{line}");
        assert!(!line.starts_with("M0.0"), "{line}");
    }

    #[test]
    fn out_of_range_values_are_clamped() {
        let (line, _) = chart_paths(
            &[
                -5.0, 500.0,
            ],
            2,
            0.0,
            100.0,
        );
        assert_eq!(line, "M0.0 100.0 L300.0 0.0");
    }

    #[test]
    fn too_few_points_draw_nothing() {
        assert_eq!(
            chart_paths(&[1.0], 60, 0.0, 1.0),
            (String::new(), String::new())
        );
    }

    #[test]
    fn long_windows_are_downsampled() {
        let values = vec![1.0; CAPACITY];
        let (line, _) = chart_paths(&values, CAPACITY, 0.0, 2.0);
        assert!(line.matches('L').count() < MAX_POINTS, "too many points");
    }
}
