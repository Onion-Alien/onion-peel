//! Rectangle helpers. Right and bottom are exclusive, like Windows.

use windows::Win32::Foundation::RECT;

/// The box spanned by two corner pixels.
pub fn span(a: (i32, i32), b: (i32, i32)) -> RECT {
    RECT { left: a.0.min(b.0), top: a.1.min(b.1), right: a.0.max(b.0) + 1, bottom: a.1.max(b.1) + 1 }
}

/// `r` moved by (dx, dy).
pub fn offset(r: RECT, dx: i32, dy: i32) -> RECT {
    RECT { left: r.left + dx, top: r.top + dy, right: r.right + dx, bottom: r.bottom + dy }
}

pub fn inflate(r: RECT, n: i32) -> RECT {
    RECT { left: r.left - n, top: r.top - n, right: r.right + n, bottom: r.bottom + n }
}

pub fn intersect(a: RECT, b: RECT) -> RECT {
    RECT { left: a.left.max(b.left), top: a.top.max(b.top), right: a.right.min(b.right), bottom: a.bottom.min(b.bottom) }
}

/// Smallest box around both; an empty box counts as nothing.
pub fn union(a: RECT, b: RECT) -> RECT {
    let empty = |r: RECT| r.right <= r.left || r.bottom <= r.top;
    match (empty(a), empty(b)) {
        (true, _) => b,
        (_, true) => a,
        _ => RECT { left: a.left.min(b.left), top: a.top.min(b.top), right: a.right.max(b.right), bottom: a.bottom.max(b.bottom) },
    }
}

pub fn contains(r: &RECT, p: (i32, i32)) -> bool {
    p.0 >= r.left && p.0 < r.right && p.1 >= r.top && p.1 < r.bottom
}

/// The same spot of `r` in `b` as it has in `a`, scaled.
pub fn map_rect(r: RECT, a: RECT, b: RECT) -> RECT {
    let f = |v: i32, a0: i32, a1: i32, b0: i32, b1: i32| b0 + ((v - a0) as i64 * (b1 - b0) as i64 / (a1 - a0).max(1) as i64) as i32;
    RECT {
        left: f(r.left, a.left, a.right, b.left, b.right),
        top: f(r.top, a.top, a.bottom, b.top, b.bottom),
        right: f(r.right, a.left, a.right, b.left, b.right),
        bottom: f(r.bottom, a.top, a.bottom, b.top, b.bottom),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(left: i32, top: i32, right: i32, bottom: i32) -> RECT {
        RECT { left, top, right, bottom }
    }

    #[test]
    fn span_holds_both_corners_whatever_the_drag_direction() {
        assert_eq!(span((5, 9), (2, 3)), rect(2, 3, 6, 10));
        assert_eq!(span((2, 3), (5, 9)), rect(2, 3, 6, 10));
        assert_eq!(span((4, 4), (4, 4)), rect(4, 4, 5, 5)); // one pixel
    }

    #[test]
    fn offset_and_inflate() {
        assert_eq!(offset(rect(1, 2, 3, 4), -10, 5), rect(-9, 7, -7, 9));
        assert_eq!(inflate(rect(10, 10, 20, 20), 2), rect(8, 8, 22, 22));
        assert_eq!(inflate(rect(10, 10, 20, 20), -2), rect(12, 12, 18, 18));
    }

    #[test]
    fn intersect_clips_to_the_shared_part() {
        assert_eq!(intersect(rect(0, 0, 10, 10), rect(5, -5, 20, 6)), rect(5, 0, 10, 6));
    }

    #[test]
    fn union_ignores_empty_boxes() {
        let (a, b) = (rect(0, 0, 4, 4), rect(10, 2, 12, 9));
        assert_eq!(union(a, b), rect(0, 0, 12, 9));
        assert_eq!(union(RECT::default(), b), b);
        assert_eq!(union(a, rect(7, 7, 7, 9)), a);
    }

    #[test]
    fn contains_leaves_out_right_and_bottom() {
        let r = rect(-5, 0, 5, 10);
        assert!(contains(&r, (-5, 0)) && contains(&r, (4, 9)));
        assert!(!contains(&r, (5, 5)) && !contains(&r, (0, 10)) && !contains(&r, (-6, 5)));
    }

    #[test]
    fn map_rect_scales_between_monitor_sizes() {
        // A 1280x720 game on a 2560x1440 monitor that sits at x = 1920.
        let (game, monitor) = (rect(0, 0, 1280, 720), rect(1920, 0, 4480, 1440));
        assert_eq!(map_rect(rect(100, 50, 300, 250), game, monitor), rect(2120, 100, 2520, 500));
        assert_eq!(map_rect(game, game, monitor), monitor);
        assert_eq!(map_rect(rect(2120, 100, 2520, 500), monitor, game), rect(100, 50, 300, 250));
        // An empty source box must not divide by zero.
        assert_eq!(map_rect(rect(1, 1, 2, 2), RECT::default(), monitor).left, monitor.left + 2560);
    }
}
