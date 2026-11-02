//! A sidebar as quill's: it slides shut and open on a spring, and while shut it peeks over the
//! page under the pointer, as Desktop's does.

use limn::{Element, Point, ScalarSpring, SpringMotion};
use std::time::{Duration, Instant};

/// Where the pointer is, for the peek: its point in the window, whether it is on the toggle,
/// the area that holds it while the sidebar slides away from under it, and whether something
/// of the sidebar's is open, as a menu, so it stays.
pub struct Pointer {
    pub at: Option<Point>,
    pub on_toggle: bool,
    pub toggle_zone: limn::Rect,
    pub held: bool,
}

pub struct Sidebar {
    /// Whether it shows; its toggle hides it.
    pub open: bool,
    pub width: f32,
    /// The width it is drawn at, on a spring to 0 or `width` as it closes and opens.
    pub shown: ScalarSpring,
    /// Shown over the page while the pointer is on it: whether it is, since when the pointer
    /// rests at the window's edge and since when it left, and the width it is drawn at.
    pub peek: bool,
    pub peek_edge: Option<Instant>,
    pub peek_left: Option<Instant>,
    pub peek_shown: ScalarSpring,
    /// Whether the peek waits for the pointer to leave the toggle, after a click on it hid
    /// the sidebar.
    pub peek_suppressed: bool,
}

impl Sidebar {
    pub fn new(width: f32) -> Self {
        let now = Instant::now();
        Self {
            open: true,
            width,
            shown: ScalarSpring::new(width, now),
            peek: false,
            peek_edge: None,
            peek_left: None,
            peek_shown: ScalarSpring::new(0.0, now),
            peek_suppressed: false,
        }
    }

    /// Its toggle clicked.
    pub fn toggle(&mut self) {
        self.open = !self.open;
        // Pinned from its peek, it stays where it is instead of sliding in again.
        if self.open && self.peek {
            let now = Instant::now();
            self.shown.snap_to(self.width, now);
            self.peek = false;
            self.peek_shown.snap_to(0.0, now);
        }
        // Hidden by a click, it does not peek again under the pointer still on its toggle
        // until the pointer has left.
        self.peek_suppressed = !self.open;
    }

    /// Move the springs on, and peek or not by where the pointer is.
    pub fn tick(&mut self, pointer: &Pointer, motion: SpringMotion, reduced: bool, now: Instant) {
        let width = self.width;
        let open = |open: bool| if open { width } else { 0.0 };
        self.shown.launch(open(self.open), motion, now);
        self.follow_pointer(pointer, now);
        self.peek_shown.launch(open(self.peek), motion, now);
        if reduced {
            self.shown.snap_to(self.shown.target(), now);
            self.peek_shown.snap_to(self.peek_shown.target(), now);
        }
    }

    /// The shut sidebar peeks at once while the pointer is on its toggle, or after it rests
    /// 100 ms within 16 px of the window's edge; it goes 180 ms after the pointer left it and
    /// the toggle, unless `held`.
    fn follow_pointer(&mut self, pointer: &Pointer, now: Instant) {
        let Pointer {
            at,
            on_toggle,
            toggle_zone,
            held,
        } = *pointer;
        let x = at.map(|p| p.x);
        if self.open {
            self.peek = false;
        } else if !self.peek {
            let at_edge = x.is_some_and(|x| x <= 16.0);
            // Held by where the pointer is, not what is under it: the sidebar slides away
            // from under it before its toggle in the bar comes back.
            let at_toggle = at.is_some_and(|p| toggle_zone.contains(p));
            self.peek_suppressed &= at_toggle || at_edge;
            self.peek_edge =
                (at_edge && !self.peek_suppressed).then(|| self.peek_edge.unwrap_or(now));
            let dwelt = self
                .peek_edge
                .is_some_and(|since| now.duration_since(since) >= Duration::from_millis(100));
            self.peek = !self.peek_suppressed && (on_toggle || dwelt);
        } else {
            let on = on_toggle || held || x.is_some_and(|x| x < self.width);
            self.peek_left = (!on).then(|| self.peek_left.unwrap_or(now));
            self.peek = self
                .peek_left
                .is_none_or(|since| now.duration_since(since) < Duration::from_millis(180));
        }
        if !self.peek {
            self.peek_left = None;
        } else {
            self.peek_edge = None;
        }
    }

    /// Still moving, or waiting on the pointer: the host keeps painting.
    pub fn animating(&self, now: Instant) -> bool {
        self.shown.is_active(now)
            || self.peek_shown.is_active(now)
            || self.peek_edge.is_some()
            || self.peek_left.is_some()
    }

    /// The width it takes beside the page now.
    pub fn docked_width(&self) -> f32 {
        self.shown.value(Instant::now()).round()
    }

    /// `content`, `width` wide, cut to what has slid in: moved left by what is still hidden,
    /// so its right edge leads and nothing in it rewraps.
    pub fn docked(&self, content: Element, height: f32) -> Option<Element> {
        let shown = self.docked_width();
        (shown > 0.0).then(|| slid(content, shown, self.width, height))
    }

    /// `base` with the shut sidebar peeking over it, the page staying where it is; its shadow
    /// is inferred.
    pub fn peeking(
        &self,
        base: Element,
        content: impl FnOnce() -> Element,
        window: limn::Size,
    ) -> Element {
        let peek = self.peek_shown.value(Instant::now()).round();
        if self.open || peek <= 0.0 {
            return base;
        }
        let sidebar = slid(content(), peek, self.width, window.height).shadow(
            limn::Color::rgba(0, 0, 0, 60),
            Point::new(0.0, 0.0),
            24.0,
            0.0,
        );
        Element::stack(vec![
            base,
            Element::row(vec![sidebar, Element::spacer().flex(1.0)]),
        ])
        .size(Some(window.width), Some(window.height))
    }
}

fn slid(content: Element, shown: f32, width: f32, height: f32) -> Element {
    content
        .translate(Point::new(shown - width, 0.0))
        .size(Some(shown), Some(height))
        .clip_children()
}
