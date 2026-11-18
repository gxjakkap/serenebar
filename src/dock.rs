//! Where the dock window and bar go, and how much each icon magnifies.

/// Window and bar rects (left, top, right, bottom) in screen pixels. The window is the full
/// monitor width and always includes the magnification room, so it only moves when the dock
/// does. Moving it per frame showed the previous frame at the new position for one refresh.
pub fn dock_geometry(
    monitor: [i32; 4],
    band: [i32; 4],
    top: bool,
    bar_w: i32,
    bar_h: i32,
    gap: i32,
    head: i32,
) -> ([i32; 4], [i32; 4]) {
    let y = if top {
        band[1] + gap
    } else {
        band[3] - gap - bar_h - head
    };
    let win = [monitor[0], y, monitor[2], y + bar_h + head];
    let x = monitor[0] + (monitor[2] - monitor[0] - bar_w) / 2;
    let by = if top { y } else { y + head };
    (win, [x, by, x + bar_w, by + bar_h])
}

/// Largest icon scale under the cursor.
pub const MAG: f32 = 1.85;

/// Scale of a dock icon `d` DIPs from the cursor. A cosine bump about five icons wide.
pub fn mag_scale(d: f32, step: f32) -> f32 {
    let range = step * 2.4;
    if d >= range {
        1.0
    } else {
        1.0 + (MAG - 1.0) * 0.5 * (1.0 + (std::f32::consts::PI * d / range).cos())
    }
}

/// Starts of the left, centre and right groups on a full-width bar `w` wide, from each
/// group's span. The centre group sits in the middle unless that runs into a side group; then
/// it slides away from it, like the Windows taskbar does when the tray grows. When there is no
/// room on either side it keeps clear of the left group.
pub fn full_width_starts(w: f32, pad: f32, gap: f32, spans: [f32; 3]) -> [f32; 3] {
    let gap_after = |s: f32| if s > 0.0 { gap } else { 0.0 };
    let right = w - pad - spans[2];
    let centre = ((w - spans[1]) / 2.0)
        .min(right - gap_after(spans[2]) - spans[1])
        .max(pad + spans[0] + gap_after(spans[0]));
    [pad, centre, right]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn magnification_peaks_under_cursor() {
        let step = 42.0;
        assert_eq!(mag_scale(0.0, step), MAG);
        let one_over = mag_scale(step, step);
        assert!(one_over > 1.3 && one_over < MAG);
        assert_eq!(mag_scale(step * 3.0, step), 1.0); // three icons over, back to normal size
        assert!(mag_scale(10.0, step) > mag_scale(20.0, step));
    }

    #[test]
    fn window_never_moves_with_bar_width() {
        let (mon, band) = ([0, 0, 1920, 1080], [0, 1012, 1920, 1080]);
        let (w1, b1) = dock_geometry(mon, band, false, 600, 56, 6, 40);
        let (w2, b2) = dock_geometry(mon, band, false, 900, 56, 6, 40);
        assert_eq!(w1, w2); // magnifying widens the bar, never the window
        assert_eq!(w1, [0, 1080 - 6 - 56 - 40, 1920, 1080 - 6]);
        assert_eq!((b1[1], b1[3]), (1080 - 6 - 56, 1080 - 6)); // bar sits at the bottom of the window
        assert_eq!((b1[0] + b1[2]) / 2, 960);
        assert_eq!(b2[2] - b2[0], 900);
        let (wt, bt) = dock_geometry(mon, [0, 0, 1920, 68], true, 600, 56, 6, 40);
        assert_eq!((wt[1], bt[1], wt[3]), (6, 6, 6 + 56 + 40)); // docked at the top, room below
    }

    #[test]
    fn centre_group_slides_clear_of_a_wide_right_group() {
        // room to spare: dead centre
        assert_eq!(
            full_width_starts(1000.0, 10.0, 20.0, [50.0, 100.0, 100.0]),
            [10.0, 450.0, 890.0]
        );
        // right group reaches past the middle: the centre ends one gap before it
        let s = full_width_starts(1000.0, 10.0, 20.0, [50.0, 300.0, 500.0]);
        assert_eq!(s, [10.0, 170.0, 490.0]);
        assert_eq!(s[1] + 300.0 + 20.0, s[2]);
        // no room at all: it stays clear of the left group
        assert_eq!(
            full_width_starts(1000.0, 10.0, 20.0, [50.0, 300.0, 900.0])[1],
            80.0
        );
    }
}
