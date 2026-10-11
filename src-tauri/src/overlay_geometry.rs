//! All desktop coordinates are physical pixels. Scaling applies only to bar size.

#[derive(Clone, Copy, Debug)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

impl Rect {
    fn contains(self, (x, y): (f64, f64)) -> bool {
        x >= self.x as f64
            && y >= self.y as f64
            && x < self.x as f64 + self.width as f64
            && y < self.y as f64 + self.height as f64
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Display {
    pub bounds: Rect,
    pub work: Rect,
    pub scale: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Placement {
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
    pub scale: f64,
}

/// Recover the old logical settings using the destination display's scale,
/// never the (possibly off-screen) window's current scale.
pub fn legacy_position(displays: &[Display], x: f64, y: f64) -> Option<(f64, f64)> {
    displays.iter().find_map(|display| {
        let point = (x * display.scale, y * display.scale);
        display.bounds.contains(point).then_some(point)
    })
}

pub fn place(
    displays: &[Display],
    primary: usize,
    position: Option<(f64, f64)>,
    providers: usize,
    zoom: f64,
) -> Option<Placement> {
    let point = position.filter(|(x, y)| x.is_finite() && y.is_finite());
    let selected = point
        .and_then(|point| displays.iter().position(|d| d.bounds.contains(point)))
        .unwrap_or(primary);
    let display = displays.get(selected).or_else(|| displays.first())?;
    let work = display.work;
    if work.width == 0 || work.height == 0 || !display.scale.is_finite() || display.scale <= 0.0 {
        return None;
    }
    let scale = display.scale;
    let zoom = if zoom.is_finite() {
        zoom.clamp(0.75, 1.5)
    } else {
        1.0
    };
    // Reduce margins as well as size on exceptionally small work areas.
    let margin_x = (4.0 * scale)
        .round()
        .min((work.width.saturating_sub(1) / 2) as f64);
    let margin_y = (4.0 * scale)
        .round()
        .min((work.height.saturating_sub(1) / 2) as f64);
    let available_w = work.width as f64 - 2.0 * margin_x;
    let available_h = work.height as f64 - 2.0 * margin_y;
    let preferred = 230.0 * providers.max(1) as f64 * zoom * scale;
    let min_w = (414.0 * zoom * scale).min(available_w);
    let width = preferred.clamp(min_w, available_w).round() as u32;
    let height = (40.0 * zoom * scale).min(available_h).round().max(1.0) as u32;
    let min_x = work.x as f64 + margin_x;
    let min_y = work.y as f64 + margin_y;
    let max_x = work.x as f64 + work.width as f64 - width as f64 - margin_x;
    let max_y = work.y as f64 + work.height as f64 - height as f64 - margin_y;
    let (x, y) = point.filter(|p| display.bounds.contains(*p)).unwrap_or((
        work.x as f64 + (work.width as f64 - width as f64) / 2.0,
        work.y as f64 + 8.0 * scale,
    ));
    Some(Placement {
        x: x.clamp(min_x, max_x).round() as i32,
        y: y.clamp(min_y, max_y).round() as i32,
        width,
        height,
        scale,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn displays() -> [Display; 2] {
        [
            Display {
                bounds: Rect {
                    x: 0,
                    y: 0,
                    width: 3200,
                    height: 2000,
                },
                work: Rect {
                    x: 0,
                    y: 0,
                    width: 3200,
                    height: 1940,
                },
                scale: 1.25,
            },
            Display {
                bounds: Rect {
                    x: 3200,
                    y: 0,
                    width: 2560,
                    height: 1440,
                },
                work: Rect {
                    x: 3200,
                    y: 0,
                    width: 2560,
                    height: 1392,
                },
                scale: 1.0,
            },
        ]
    }

    #[test]
    fn actual_offscreen_position_recovers_to_primary() {
        let p = place(&displays(), 0, Some((5928.0, 86.0)), 4, 0.8).unwrap();
        assert_eq!((p.x, p.y, p.width, p.height), (1140, 10, 920, 40));
    }

    #[test]
    fn legacy_mixed_dpi_position_is_clamped_on_destination() {
        let ds = displays();
        let point = legacy_position(&ds, 4742.4, 68.8).unwrap();
        let p = place(&ds, 0, Some(point), 4, 0.8).unwrap();
        assert_eq!((p.x, p.y, p.width, p.height), (4742, 69, 736, 32));
        assert!(p.x + (p.width as i32) < 5760);
    }

    #[test]
    fn dragging_to_edges_keeps_entire_bar_above_taskbar() {
        let p = place(&displays(), 0, Some((5759.0, 1439.0)), 4, 1.0).unwrap();
        assert_eq!((p.x, p.y), (4836, 1348));
    }

    #[test]
    fn disconnected_monitor_and_invalid_positions_recover() {
        let ds = [displays()[0]];
        for point in [(4742.0, 69.0), (f64::NAN, 0.0), (f64::INFINITY, 0.0)] {
            assert_eq!(
                place(&ds, 0, Some(point), 4, 1.0),
                place(&ds, 0, None, 4, 1.0)
            );
        }
        assert_eq!(place(&[], 0, None, 4, 1.0), None);
    }

    #[test]
    fn negative_monitor_origin_is_preserved() {
        let ds = [Display {
            bounds: Rect {
                x: -1920,
                y: -1080,
                width: 1920,
                height: 1080,
            },
            work: Rect {
                x: -1880,
                y: -1080,
                width: 1880,
                height: 1080,
            },
            scale: 1.5,
        }];
        let p = place(&ds, 0, Some((-1919.0, -1079.0)), 2, 1.0).unwrap();
        assert_eq!((p.x, p.y), (-1874, -1074));
    }

    #[test]
    fn resize_fits_smaller_secondary_monitor() {
        let mut ds = displays();
        ds[1].work.width = 500;
        ds[1].work.height = 50;
        let p = place(&ds, 0, Some((3600.0, 40.0)), 6, 1.5).unwrap();
        assert_eq!((p.x, p.y, p.width, p.height), (3204, 4, 492, 42));
    }
}
