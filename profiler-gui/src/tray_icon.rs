//! Draws the tray icon (the app logo: a chart line on a dark rounded
//! square) in the accent colour, as ARGB32 pixels for `StatusNotifierItem`.
//! Drawing it avoids shipping image files and an image decoder.

/// Logo geometry on a 64 x 64 canvas, matching `ui/app-icon.svg`.
const CANVAS: f32 = 64.0;
const CORNER: f32 = 10.0;
const STROKE: f32 = 5.0;
const LINES: [&[(f32, f32)]; 2] = [
    &[
        (10.0, 42.0),
        (22.0, 28.0),
        (31.0, 37.0),
        (42.0, 21.0),
        (54.0, 34.0),
    ],
    &[
        (10.0, 51.0),
        (54.0, 51.0),
    ],
];
const BACKGROUND: (u8, u8, u8) = (0x14, 0x14, 0x14);

/// Distance from `p` to the segment `a`-`b`.
fn segment_distance(p: (f32, f32), a: (f32, f32), b: (f32, f32)) -> f32 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len2 = dx.mul_add(dx, dy * dy);
    let t = if len2 == 0.0 {
        0.0
    } else {
        ((p.0 - a.0).mul_add(dx, (p.1 - a.1) * dy) / len2).clamp(0.0, 1.0)
    };
    (p.0 - t.mul_add(dx, a.0)).hypot(p.1 - t.mul_add(dy, a.1))
}

/// Signed distance to the rounded square (negative inside).
fn square_distance(p: (f32, f32)) -> f32 {
    let half = CANVAS / 2.0;
    let qx = (p.0 - half).abs() - (half - CORNER);
    let qy = (p.1 - half).abs() - (half - CORNER);
    qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - CORNER
}

/// Coverage 0..1 from a signed distance, with `aa` canvas units of blur.
fn coverage(distance: f32, aa: f32) -> f32 {
    (0.5 - distance / aa).clamp(0.0, 1.0)
}

fn blend(under: f32, over: f32, alpha: f32) -> f32 {
    under + (over - under) * alpha
}

/// Pixel data of a `size` x `size` icon: ARGB, one byte each, row by row
/// (the byte order `StatusNotifierItem` expects).
pub fn render(size: u32, accent: (u8, u8, u8)) -> Vec<u8> {
    #[allow(clippy::cast_precision_loss)] // icon sizes are tiny
    let scale = CANVAS / size as f32;
    let mut out = Vec::with_capacity((size * size * 4) as usize);
    let channel = |v: u8| f32::from(v);
    for y in 0..size {
        for x in 0..size {
            #[allow(clippy::cast_precision_loss)]
            let p = ((x as f32 + 0.5) * scale, (y as f32 + 0.5) * scale);
            let shape = coverage(square_distance(p), scale);
            let line_distance = LINES
                .iter()
                .flat_map(|line| line.windows(2))
                .map(|w| segment_distance(p, w[0], w[1]))
                .fold(f32::MAX, f32::min);
            let line = coverage(line_distance - STROKE / 2.0, scale);
            let mix = |bg: u8, fg: u8| blend(channel(bg), channel(fg), line);
            let rgb = [
                mix(BACKGROUND.0, accent.0),
                mix(BACKGROUND.1, accent.1),
                mix(BACKGROUND.2, accent.2),
            ];
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)] // 0..=255
            let byte = |v: f32| v.round().clamp(0.0, 255.0) as u8;
            out.extend([
                byte(shape * 255.0),
                byte(rgb[0]),
                byte(rgb[1]),
                byte(rgb[2]),
            ]);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const AMBER: (u8, u8, u8) = (0xff, 0xa5, 0x1a);

    fn pixel(data: &[u8], size: u32, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * size + x) * 4) as usize;
        [
            data[i],
            data[i + 1],
            data[i + 2],
            data[i + 3],
        ]
    }

    #[test]
    fn size_and_layout() {
        let data = render(32, AMBER);
        assert_eq!(data.len(), 32 * 32 * 4);
        // Rounded corner is transparent, the middle of the square opaque.
        assert_eq!(pixel(&data, 32, 0, 0)[0], 0);
        assert_eq!(
            pixel(&data, 32, 16, 8),
            [
                255, 0x14, 0x14, 0x14
            ]
        );
    }

    #[test]
    fn baseline_is_accent() {
        // The baseline runs along y = 51 of 64, i.e. row 25 of 32.
        let data = render(32, AMBER);
        assert_eq!(
            pixel(&data, 32, 16, 25),
            [
                255, 0xff, 0xa5, 0x1a
            ]
        );
    }

    #[test]
    fn geometry() {
        assert!((segment_distance((5.0, 5.0), (0.0, 0.0), (10.0, 0.0)) - 5.0).abs() < 1e-4);
        assert!((segment_distance((15.0, 0.0), (0.0, 0.0), (10.0, 0.0)) - 5.0).abs() < 1e-4);
        assert!(square_distance((32.0, 32.0)) < 0.0);
        assert!(
            square_distance((0.5, 0.5)) > 0.0,
            "outside the rounded corner"
        );
    }
}
