//! Shared, resolution-independent account-card mark.

pub fn rgba_icon(size: u32) -> Vec<u8> {
    rasterize(size, false)
}

pub fn rgba_tray_icon(size: u32) -> Vec<u8> {
    rasterize(size, true)
}

fn rounded_rect(x: f32, y: f32, bounds: [f32; 4], radius: f32) -> bool {
    let [left, top, width, height] = bounds;
    let dx = (x - left - width / 2.0).abs() - width / 2.0 + radius;
    let dy = (y - top - height / 2.0).abs() - height / 2.0 + radius;
    dx.max(0.0).hypot(dy.max(0.0)) + dx.max(dy).min(0.0) <= radius
}

fn mark(x: f32, y: f32) -> bool {
    let rear = rounded_rect(x, y, [12.0, 11.0, 29.0, 34.0], 6.0)
        && !rounded_rect(x, y, [14.5, 13.5, 24.0, 29.0], 3.5);
    let front = rounded_rect(x, y, [23.0, 21.0, 29.0, 32.0], 6.0);
    let head = (x - 37.5).hypot(y - 31.5) <= 4.0;
    let shoulders = rounded_rect(x, y, [30.0, 39.0, 15.0, 6.5], 3.25);
    // The foreground card occludes the rear outline; the profile is negative space.
    if front {
        !head && !shoulders
    } else {
        rear
    }
}

fn rasterize(size: u32, template: bool) -> Vec<u8> {
    let mut rgba = vec![0_u8; (size as usize) * (size as usize) * 4];
    // Supersampling preserves smooth silhouettes at small menu-bar sizes.
    const SAMPLES: u32 = 4;
    for y in 0..size {
        for x in 0..size {
            let mut total = [0.0_f32; 4];
            for sy in 0..SAMPLES {
                for sx in 0..SAMPLES {
                    let px = (x as f32 + (sx as f32 + 0.5) / SAMPLES as f32) * 64.0 / size as f32;
                    let py = (y as f32 + (sy as f32 + 0.5) / SAMPLES as f32) * 64.0 / size as f32;
                    let color = if template {
                        mark(32.0 + (px - 32.0) * 0.8, 32.0 + (py - 32.0) * 0.8)
                            .then_some([0.0, 0.0, 0.0])
                    } else if rounded_rect(px, py, [1.0, 1.0, 62.0, 62.0], 15.0) {
                        let t = (px + py) / 128.0;
                        Some(if mark(px, py) {
                            [255.0, 255.0, 255.0]
                        } else {
                            [102.0 - 40.0 * t, 108.0 - 40.0 * t, 247.0 - 34.0 * t]
                        })
                    } else {
                        None
                    };
                    if let Some(color) = color {
                        for channel in 0..3 {
                            total[channel] += color[channel];
                        }
                        total[3] += 1.0;
                    }
                }
            }
            if total[3] > 0.0 {
                let index = ((y * size + x) * 4) as usize;
                for channel in 0..3 {
                    rgba[index + channel] = (total[channel] / total[3]).round() as u8;
                }
                rgba[index + 3] = (255.0 * total[3] / (SAMPLES * SAMPLES) as f32).round() as u8;
            }
        }
    }
    rgba
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn icons_have_transparent_margins_and_antialiased_edges() {
        for size in [16, 22, 32, 64, 128] {
            for icon in [rgba_icon(size), rgba_tray_icon(size)] {
                assert_eq!(icon.len(), (size * size * 4) as usize);
                assert_eq!(icon[3], 0);
                assert!(icon.chunks_exact(4).any(|p| p[3] == 255));
                assert!(icon.chunks_exact(4).any(|p| p[3] > 0 && p[3] < 255));
            }
        }
    }

    #[test]
    fn tray_is_monochrome_and_profile_is_cut_out() {
        assert!(!mark(37.5, 31.5));
        assert!(!mark(37.5, 42.0));
        assert!(mark(26.0, 34.0));
        assert!(rgba_tray_icon(32)
            .chunks_exact(4)
            .all(|p| p[..3] == [0, 0, 0]));
    }
}
