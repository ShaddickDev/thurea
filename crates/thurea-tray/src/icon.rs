//! Thurea's logo as tray pixmaps (ARGB32), drawn here so the tray needs no icon files:
//! the blue gradient thurea with its check mark, or an orange one when protection needs a look.

/// The logo's outline as a polygon (its own coordinates: x 31..77, y 27..82), the same shape
/// as extension/icons/thurea.svg and the Omarchy plugin's ThureaGeometry.js.
fn outline() -> Vec<(f32, f32)> {
    fn cubic(p: [(f32, f32); 4], out: &mut Vec<(f32, f32)>) {
        for i in 1..=24 {
            let t = i as f32 / 24.0;
            let u = 1.0 - t;
            let b = [u * u * u, 3.0 * u * u * t, 3.0 * u * t * t, t * t * t];
            out.push((
                b[0] * p[0].0 + b[1] * p[1].0 + b[2] * p[2].0 + b[3] * p[3].0,
                b[0] * p[0].1 + b[1] * p[1].1 + b[2] * p[2].1 + b[3] * p[3].1,
            ));
        }
    }
    let mut p = vec![(54.0, 27.0), (77.0, 35.5), (77.0, 52.0)];
    cubic([(77.0, 52.0), (77.0, 66.5), (67.5, 76.5), (54.0, 82.0)], &mut p);
    cubic([(54.0, 82.0), (40.5, 76.5), (31.0, 66.5), (31.0, 52.0)], &mut p);
    p.push((31.0, 35.5));
    p
}

fn inside(poly: &[(f32, f32)], x: f32, y: f32) -> bool {
    let mut c = false;
    let mut j = poly.len() - 1;
    for i in 0..poly.len() {
        let (xi, yi) = poly[i];
        let (xj, yj) = poly[j];
        if (yi > y) != (yj > y) && x < (xj - xi) * (y - yi) / (yj - yi) + xi {
            c = !c;
        }
        j = i;
    }
    c
}

fn seg_dist(p: (f32, f32), a: (f32, f32), b: (f32, f32)) -> f32 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let t = (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / (dx * dx + dy * dy)).clamp(0.0, 1.0);
    ((p.0 - a.0 - t * dx).powi(2) + (p.1 - a.1 - t * dy).powi(2)).sqrt()
}

/// An ARGB32 (network byte order) picture of the logo, `size` pixels square, 4× supersampled.
pub fn pixmap(size: usize, warning: bool) -> Vec<u8> {
    let poly = outline();
    let check = [(43.5, 54.5), (51.0, 62.0), (65.0, 47.0)];
    let scale = 58.0 / size as f32; // logo units per pixel (56-unit logo with a little margin)
    let (ox, oy) = (25.0, 25.5);
    let mut out = Vec::with_capacity(size * size * 4);
    for py in 0..size {
        for px in 0..size {
            let (mut a, mut r, mut g, mut b) = (0f32, 0f32, 0f32, 0f32);
            for sy in 0..4 {
                for sx in 0..4 {
                    let x = ox + (px as f32 + (sx as f32 + 0.5) / 4.0) * scale;
                    let y = oy + (py as f32 + (sy as f32 + 0.5) / 4.0) * scale;
                    if !inside(&poly, x, y) {
                        continue;
                    }
                    let on_check = seg_dist((x, y), check[0], check[1]).min(seg_dist((x, y), check[1], check[2])) < 2.5;
                    let (cr, cg, cb) = if on_check {
                        (0x11, 0x13, 0x18)
                    } else if warning {
                        (0xF2, 0x9B, 0x38)
                    } else {
                        // #A9D8FF → #4F86E0 along the diagonal
                        let t = (((x - 31.0) * 46.0 + (y - 27.0) * 55.0) / (46.0 * 46.0 + 55.0 * 55.0)).clamp(0.0, 1.0);
                        let mix = |s: f32, e: f32| s + (e - s) * t;
                        (mix(169.0, 79.0) as i32, mix(216.0, 134.0) as i32, mix(255.0, 224.0) as i32)
                    };
                    a += 1.0;
                    r += cr as f32;
                    g += cg as f32;
                    b += cb as f32;
                }
            }
            if a == 0.0 {
                out.extend_from_slice(&[0, 0, 0, 0]);
            } else {
                out.extend_from_slice(&[(a / 16.0 * 255.0) as u8, (r / a) as u8, (g / a) as u8, (b / a) as u8]);
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn logo_is_drawn() {
        let p = super::pixmap(32, false);
        assert_eq!(p.len(), 32 * 32 * 4);
        let opaque = p.chunks(4).filter(|px| px[0] == 255).count();
        assert!(opaque > 300 && opaque < 1000, "{opaque} opaque pixels");
        assert_eq!(&p[0..4], &[0, 0, 0, 0]); // corner stays transparent
    }
}
