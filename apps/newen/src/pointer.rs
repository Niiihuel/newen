//! The pointer over the browser's page, heard from the compositor on a thread of its own
//! (docs/browser.md §8.2). The window reads its input once a frame, and a frame waits for the
//! screen: a move, a press or a wheel step could sit there a whole refresh before the page
//! knew of it. Here each one reaches Chromium as it arrives (`Native::pointer_on`), on a
//! second `wl_pointer` of the window's own connection.
//!
//! The window still hears the same events, a frame later. It gives the page none of its moves
//! or wheel while this thread runs, and asks of each press and release whether the page was
//! given it here (`Apart::gave`), so none is given twice and none is lost. What is the page's
//! is decided as the window decides it: the page's rectangle, less what Newen has above it,
//! as the window laid them out last (`Pointer::lay`).

use crate::page::{self, Apart, Clicks};
use eframe::egui;
use newen_model::{BrowserInput, Modifiers};
use std::collections::VecDeque;
use std::os::fd::AsRawFd;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};
use wayland_client::protocol::{
    wl_pointer::{self, WlPointer},
    wl_registry::{self, WlRegistry},
    wl_seat::{self, WlSeat},
    wl_surface::WlSurface,
};
use wayland_client::{Connection, Dispatch, EventQueue, Proxy, QueueHandle, WEnum};

/// How long the window waits to hear whether a press was the page's, when this thread has not
/// said yet: it is woken by the same event and says within microseconds. Past this the
/// window gives the press itself.
const WAIT: Duration = Duration::from_millis(5);
/// A press the window never asked about (a button it does not know) is forgotten after this.
const STALE: Duration = Duration::from_secs(2);
/// How often the thread looks whether it is to stop.
const LOOK: i32 = 250;
/// A wheel's notch in the page, as `page.rs` moves it: 100 px, Chromium's own on Linux.
const NOTCH: f32 = 100.0;

/// Where the pointer's input goes: the page of the surface named, if it is on screen
/// (`Native::pointer_on`).
pub type Give = Arc<dyn Fn(&str, &BrowserInput) -> bool + Send + Sync>;

/// The thread, for the window: it is told the page's place each frame and asked about each
/// press. Dropped, the thread ends before the window's connection does.
pub struct Pointer {
    shared: Arc<Shared>,
    thread: Option<std::thread::JoinHandle<()>>,
}

#[derive(Default)]
struct Shared {
    laid: Mutex<Laid>,
    /// Each press and release the compositor told, oldest first, with whether the page was
    /// given it here.
    buttons: Mutex<VecDeque<Told>>,
    told: Condvar,
    stop: AtomicBool,
    ended: AtomicBool,
    /// The window found the page's place as told here to be what limn makes of it under the
    /// pointer: off, the page's moves and wheel are the window's to give for now.
    agreed: AtomicBool,
}

/// The window as it was last laid out: the page's surface and rectangle, what of Newen's is
/// above it, and the keys held.
#[derive(Clone, Default)]
struct Laid {
    page: Option<(String, egui::Rect)>,
    above: Vec<egui::Rect>,
    keys: Modifiers,
}

impl Laid {
    /// The page's rectangle if the page is what a press at `pos` would land on.
    fn over(&self, pos: egui::Pos2) -> Option<egui::Rect> {
        let (_, rect) = self.page.as_ref()?;
        (rect.contains(pos) && !self.above.iter().any(|above| above.contains(pos))).then_some(*rect)
    }
}

struct Told {
    button: egui::PointerButton,
    pressed: bool,
    given: bool,
    at: Instant,
}

impl Pointer {
    /// Hear the pointer of the window whose Wayland `display` and `surface` these are, and
    /// give the page's share to `give`. `None` where there is no seat to hear.
    pub fn start(
        display: *mut std::ffi::c_void,
        surface: *mut std::ffi::c_void,
        give: Give,
    ) -> Result<Self, String> {
        let backend =
            unsafe { wayland_backend::client::Backend::from_foreign_display(display.cast()) };
        let connection = Connection::from_backend(backend);
        let window = unsafe {
            let id =
                wayland_backend::client::ObjectId::from_ptr(WlSurface::interface(), surface.cast())
                    .map_err(|e| e.to_string())?;
            WlSurface::from_id(&connection, id).map_err(|e| e.to_string())?
        };
        Self::on(connection, window, give)
    }

    fn on(connection: Connection, window: WlSurface, give: Give) -> Result<Self, String> {
        let mut events = connection.new_event_queue();
        let qh = events.handle();
        let _registry = connection.display().get_registry(&qh, ());
        let shared = Arc::new(Shared::default());
        let mut state = State {
            shared: shared.clone(),
            give,
            window,
            seat: None,
            pointer: None,
            on_window: false,
            route: Route::default(),
            travel: [0.0; 2],
            notches: [0; 2],
        };
        // The seat is known before the thread starts: without one there is nothing to hear.
        events.roundtrip(&mut state).map_err(|e| e.to_string())?;
        if state.seat.is_none() {
            return Err("the compositor has no wl_seat".into());
        }
        let thread = std::thread::Builder::new()
            .name("newen-pointer".into())
            .spawn(move || run(events, state))
            .map_err(|e| e.to_string())?;
        Ok(Self {
            shared,
            thread: Some(thread),
        })
    }

    /// Whether the thread still hears the pointer: once it ended (the connection broke), the
    /// window gives the page its input itself again.
    pub fn hears(&self) -> bool {
        !self.shared.ended.load(Ordering::Relaxed)
    }

    /// The window as this frame laid it out: the page's surface and rectangle (`None` when
    /// no page is on screen), the rectangles of Newen's own that are above it, and the keys
    /// held. `pointer` is where the pointer is now and whether limn finds the page under it:
    /// if these rectangles say otherwise they are not trusted, and until they are the page is
    /// given nothing here and the window gives it its moves and wheel itself.
    pub fn lay(
        &self,
        page: Option<(String, egui::Rect)>,
        above: Vec<egui::Rect>,
        keys: Modifiers,
        pointer: Option<(egui::Pos2, bool)>,
    ) {
        let mut laid = Laid { page, above, keys };
        let agreed = pointer.is_none_or(|(pos, limn)| laid.over(pos).is_some() == limn);
        if !agreed {
            laid.page = None;
        }
        // In this order: the thread stops giving before the window starts to.
        *self.shared.laid.lock().unwrap() = laid;
        self.shared.agreed.store(agreed, Ordering::Relaxed);
    }
}

impl Apart for Pointer {
    fn gave(&self, button: egui::PointerButton, pressed: bool) -> bool {
        let deadline = Instant::now() + WAIT;
        let mut buttons = self.shared.buttons.lock().unwrap();
        loop {
            buttons.retain(|told| told.at.elapsed() < STALE);
            let found = buttons
                .iter()
                .position(|told| (told.button, told.pressed) == (button, pressed));
            if let Some(found) = found {
                // Those before it were never asked about: the window does not know them.
                return buttons.drain(..=found).next_back().is_some_and(|t| t.given);
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() || !self.hears() {
                return false;
            }
            buttons = self.shared.told.wait_timeout(buttons, left).unwrap().0;
        }
    }

    fn moves(&self) -> bool {
        self.shared.agreed.load(Ordering::Relaxed)
    }
}

impl Drop for Pointer {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// The pointer's events as the page's input: what `page.rs` does with the window's, for
/// events that carry no window.
#[derive(Default)]
struct Route {
    at: Option<egui::Pos2>,
    /// The button held since a press on the page: its moves and release are the page's even
    /// off it.
    held: Option<egui::PointerButton>,
    clicks: Clicks,
    /// The pointer was last over the page: moved off it, the page is told it left.
    hovering: bool,
}

impl Route {
    /// The pointer is at `pos` in the window.
    fn moved(&mut self, laid: &Laid, pos: egui::Pos2) -> Option<BrowserInput> {
        self.at = Some(pos);
        let page = laid.page.as_ref().map(|(_, rect)| *rect);
        let rect = laid.over(pos).or(page.filter(|_| self.held.is_some()));
        let was = std::mem::replace(&mut self.hovering, rect.is_some());
        match rect {
            Some(rect) => Some(BrowserInput::Move {
                x: pos.x - rect.min.x,
                y: pos.y - rect.min.y,
            }),
            None => was.then_some(BrowserInput::Leave),
        }
    }

    /// The pointer left the window.
    fn left(&mut self) -> Option<BrowserInput> {
        self.at = None;
        let held = self.held.take().is_some();
        (std::mem::take(&mut self.hovering) || held).then_some(BrowserInput::Leave)
    }

    /// A button went down or up where the pointer is: the page's input, if it is the page's.
    fn button(
        &mut self,
        laid: &Laid,
        button: egui::PointerButton,
        pressed: bool,
    ) -> Option<BrowserInput> {
        let pos = self.at?;
        let known = page::pointer_button(button)?;
        let place = |rect: egui::Rect| (pos.x - rect.min.x, pos.y - rect.min.y);
        if pressed {
            let (x, y) = place(laid.over(pos)?);
            self.held = Some(button);
            return Some(BrowserInput::Down {
                x,
                y,
                button: known,
                clicks: self.clicks.count(pos, button),
                keys: laid.keys,
            });
        }
        if self.held != Some(button) {
            return None;
        }
        self.held = None;
        let page = laid.page.as_ref().map(|(_, rect)| *rect);
        let (x, y) = place(laid.over(pos).or(page)?);
        Some(BrowserInput::Up {
            x,
            y,
            button: known,
            keys: laid.keys,
        })
    }

    /// A press the page did not take after all (another tab is on screen): not held.
    fn refused(&mut self) {
        self.held = None;
    }

    /// The wheel moved: `travel` in the window's pixels, right and down, and `notches` when a
    /// wheel's steps made it (a touchpad's travel has none).
    fn wheel(&self, laid: &Laid, travel: [f32; 2], notches: [f32; 2]) -> Option<BrowserInput> {
        let pos = self.at?;
        let rect = laid.over(pos)?;
        let precise = notches == [0.0; 2];
        let [x, y] = if precise {
            travel
        } else {
            notches.map(|n| n * NOTCH)
        };
        // Shift turns a vertical wheel sideways, as browsers do.
        let (dx, dy) = if laid.keys.shift && x == 0.0 {
            (y, 0.0)
        } else {
            (x, y)
        };
        Some(BrowserInput::Wheel {
            x: pos.x - rect.min.x,
            y: pos.y - rect.min.y,
            dx,
            dy,
            precise,
        })
    }
}

/// The compositor's button (Linux's input codes) as the window names it; `None` for one the
/// window does not tell of either.
fn button_of(code: u32) -> Option<egui::PointerButton> {
    Some(match code {
        0x110 => egui::PointerButton::Primary,
        0x111 => egui::PointerButton::Secondary,
        0x112 => egui::PointerButton::Middle,
        0x113 | 0x116 => egui::PointerButton::Extra1,
        0x114 | 0x115 => egui::PointerButton::Extra2,
        _ => return None,
    })
}

struct State {
    shared: Arc<Shared>,
    give: Give,
    window: WlSurface,
    seat: Option<WlSeat>,
    pointer: Option<WlPointer>,
    /// The pointer is on the window's own surface, not another of the program's.
    on_window: bool,
    route: Route,
    /// A wheel's events until their frame: its travel, and its notches in 120ths.
    travel: [f64; 2],
    notches: [i32; 2],
}

impl State {
    fn laid(&self) -> Laid {
        self.shared.laid.lock().unwrap().clone()
    }

    /// Give the page `input`; whether it took it.
    fn give(&self, laid: &Laid, input: Option<BrowserInput>) -> bool {
        match (input, &laid.page) {
            (Some(input), Some((surface, _))) => (self.give)(surface, &input),
            _ => false,
        }
    }

    fn button(&mut self, code: u32, pressed: bool) {
        // Only the window's own surface: a press on another of the program's (a title bar
        // it draws itself) never reaches the window's input, which would then take what is
        // told of it for its next press.
        let (Some(button), true) = (button_of(code), self.on_window) else {
            return;
        };
        let laid = self.laid();
        let input = self.route.button(&laid, button, pressed);
        let asked = input.is_some();
        let given = self.give(&laid, input);
        if asked && pressed && !given {
            self.route.refused();
        }
        let told = Told {
            button,
            pressed,
            given,
            at: Instant::now(),
        };
        self.shared.buttons.lock().unwrap().push_back(told);
        self.shared.told.notify_all();
    }

    /// The wheel's events of one frame, as one step.
    fn wheel(&mut self) {
        let travel = std::mem::take(&mut self.travel).map(|v| v as f32);
        let notches = std::mem::take(&mut self.notches).map(|v| v as f32 / 120.0);
        if !self.on_window || (travel == [0.0; 2] && notches == [0.0; 2]) {
            return;
        }
        let laid = self.laid();
        self.give(&laid, self.route.wheel(&laid, travel, notches));
    }
}

impl Dispatch<WlRegistry, ()> for State {
    fn event(
        state: &mut Self,
        registry: &WlRegistry,
        event: wl_registry::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_registry::Event::Global {
            name,
            interface,
            version,
        } = event
        {
            // The first seat, as the window's own takes; version 8 tells a wheel's notches
            // in 120ths.
            if interface == "wl_seat" && state.seat.is_none() {
                state.seat = Some(registry.bind(name, version.min(8), qh, ()));
            }
        }
    }
}

impl Dispatch<WlSeat, ()> for State {
    fn event(
        state: &mut Self,
        seat: &WlSeat,
        event: wl_seat::Event,
        _: &(),
        _: &Connection,
        qh: &QueueHandle<Self>,
    ) {
        if let wl_seat::Event::Capabilities {
            capabilities: WEnum::Value(has),
        } = event
        {
            let pointer = has.contains(wl_seat::Capability::Pointer);
            if pointer && state.pointer.is_none() {
                state.pointer = Some(seat.get_pointer(qh, ()));
            } else if !pointer {
                if let Some(gone) = state.pointer.take() {
                    if gone.version() >= 3 {
                        gone.release();
                    }
                }
            }
        }
    }
}

impl Dispatch<WlPointer, ()> for State {
    fn event(
        state: &mut Self,
        pointer: &WlPointer,
        event: wl_pointer::Event,
        _: &(),
        _: &Connection,
        _: &QueueHandle<Self>,
    ) {
        let at = |x: f64, y: f64| egui::pos2(x as f32, y as f32);
        match event {
            wl_pointer::Event::Enter {
                surface,
                surface_x,
                surface_y,
                ..
            } => {
                state.on_window = surface.id() == state.window.id();
                if state.on_window {
                    let laid = state.laid();
                    let input = state.route.moved(&laid, at(surface_x, surface_y));
                    state.give(&laid, input);
                }
            }
            wl_pointer::Event::Leave { .. } => {
                if std::mem::take(&mut state.on_window) {
                    let laid = state.laid();
                    let input = state.route.left();
                    state.give(&laid, input);
                }
            }
            wl_pointer::Event::Motion {
                surface_x,
                surface_y,
                ..
            } if state.on_window => {
                let laid = state.laid();
                let input = state.route.moved(&laid, at(surface_x, surface_y));
                state.give(&laid, input);
            }
            wl_pointer::Event::Button {
                button,
                state: WEnum::Value(pressed),
                ..
            } => state.button(button, pressed == wl_pointer::ButtonState::Pressed),
            wl_pointer::Event::Axis {
                axis: WEnum::Value(axis),
                value,
                ..
            } => {
                state.travel[usize::from(axis == wl_pointer::Axis::VerticalScroll)] += value;
                // Before version 5 an event is its own frame.
                if pointer.version() < 5 {
                    state.wheel();
                }
            }
            wl_pointer::Event::AxisDiscrete {
                axis: WEnum::Value(axis),
                discrete,
            } => {
                state.notches[usize::from(axis == wl_pointer::Axis::VerticalScroll)] +=
                    discrete * 120;
            }
            wl_pointer::Event::AxisValue120 {
                axis: WEnum::Value(axis),
                value120,
            } => {
                state.notches[usize::from(axis == wl_pointer::Axis::VerticalScroll)] += value120;
            }
            wl_pointer::Event::Frame => state.wheel(),
            _ => {}
        }
    }
}

/// Hear the connection until told to stop or it breaks. Events are read as the window's own
/// thread reads them (prepare, wait, read), so neither takes the other's.
fn run(mut events: EventQueue<State>, mut state: State) {
    let shared = state.shared.clone();
    while !shared.stop.load(Ordering::Relaxed) {
        if events.dispatch_pending(&mut state).is_err() {
            break;
        }
        // What the handlers asked for (the seat's pointer) goes out; a full socket is the
        // window's to drain.
        let _ = events.flush();
        let Some(reading) = events.prepare_read() else {
            continue;
        };
        let mut wait = libc::pollfd {
            fd: reading.connection_fd().as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        // SAFETY: one valid pollfd, which outlives the call.
        let ready = unsafe { libc::poll(&mut wait, 1, LOOK) };
        if ready > 0 && wait.revents & libc::POLLIN != 0 {
            // Another thread may have read them first: then there is nothing to read.
            let _ = reading.read();
        } else if ready > 0 {
            break;
        }
    }
    shared.ended.store(true, Ordering::Relaxed);
    shared.told.notify_all();
}

#[cfg(test)]
mod tests {
    use super::*;
    use newen_model::PointerButton;

    fn laid() -> Laid {
        let rect = |x, y, w, h| egui::Rect::from_min_size(egui::pos2(x, y), egui::vec2(w, h));
        Laid {
            page: Some(("browser:1:3".into(), rect(100.0, 50.0, 400.0, 300.0))),
            // A menu of Newen's over the page's corner.
            above: vec![rect(400.0, 50.0, 100.0, 80.0)],
            keys: Modifiers::default(),
        }
    }

    #[test]
    fn the_pointer_is_the_pages_where_nothing_of_newens_is_above_it() {
        let (laid, mut route) = (laid(), Route::default());
        let moved = |route: &mut Route, x, y| route.moved(&laid, egui::pos2(x, y));
        assert_eq!(
            moved(&mut route, 150.0, 70.0),
            Some(BrowserInput::Move { x: 50.0, y: 20.0 })
        );
        // Over Newen's menu the page is told the pointer left, once.
        assert_eq!(moved(&mut route, 450.0, 70.0), Some(BrowserInput::Leave));
        assert_eq!(moved(&mut route, 460.0, 70.0), None);
        assert_eq!(
            route.button(&laid, egui::PointerButton::Primary, true),
            None
        );
        // A press on the page holds it: its moves and release are the page's off it too.
        moved(&mut route, 150.0, 70.0);
        let down = route.button(&laid, egui::PointerButton::Primary, true);
        assert!(
            matches!(down, Some(BrowserInput::Down { x, y, button: PointerButton::Left, clicks: 1, .. })
                if (x, y) == (50.0, 20.0))
        );
        assert_eq!(
            moved(&mut route, 700.0, 70.0),
            Some(BrowserInput::Move { x: 600.0, y: 20.0 })
        );
        let up = route.button(&laid, egui::PointerButton::Primary, false);
        assert!(matches!(up, Some(BrowserInput::Up { x, .. }) if x == 600.0));
        // Let go, the pointer off the page is no longer its own.
        assert_eq!(moved(&mut route, 701.0, 70.0), Some(BrowserInput::Leave));
        // A button the page does not know, and a release with no press, are not its own.
        moved(&mut route, 150.0, 70.0);
        assert_eq!(route.button(&laid, egui::PointerButton::Extra1, true), None);
        assert_eq!(
            route.button(&laid, egui::PointerButton::Primary, false),
            None
        );
        assert_eq!(route.left(), Some(BrowserInput::Leave));
        assert_eq!(route.left(), None);
    }

    #[test]
    fn a_touchpads_travel_is_exact_and_a_wheels_notch_is_a_hundred_pixels() {
        let (mut laid, mut route) = (laid(), Route::default());
        assert_eq!(
            route.wheel(&laid, [0.0, 12.5], [0.0; 2]),
            None,
            "no pointer yet"
        );
        route.moved(&laid, egui::pos2(150.0, 70.0));
        let wheel = |dx, dy, precise| {
            Some(BrowserInput::Wheel {
                x: 50.0,
                y: 20.0,
                dx,
                dy,
                precise,
            })
        };
        assert_eq!(
            route.wheel(&laid, [0.0, 12.5], [0.0; 2]),
            wheel(0.0, 12.5, true)
        );
        // A wheel's notch counts, not the travel the compositor gives it.
        assert_eq!(
            route.wheel(&laid, [0.0, 15.0], [0.0, 1.0]),
            wheel(0.0, 100.0, false)
        );
        assert_eq!(
            route.wheel(&laid, [0.0, -7.5], [0.0, -0.5]),
            wheel(0.0, -50.0, false)
        );
        laid.keys.shift = true;
        assert_eq!(
            route.wheel(&laid, [0.0, 15.0], [0.0, 1.0]),
            wheel(100.0, 0.0, false)
        );
        // Over what Newen has above the page, the wheel is Newen's.
        route.moved(&laid, egui::pos2(450.0, 70.0));
        assert_eq!(route.wheel(&laid, [0.0, 12.5], [0.0; 2]), None);
    }

    fn told(button: egui::PointerButton, pressed: bool, given: bool) -> Told {
        Told {
            button,
            pressed,
            given,
            at: Instant::now(),
        }
    }

    #[test]
    fn each_press_is_asked_about_once_and_one_never_told_is_the_windows() {
        use egui::PointerButton::{Extra1, Primary, Secondary};
        let pointer = Pointer {
            shared: Arc::default(),
            thread: None,
        };
        let tell = |told: Told| pointer.shared.buttons.lock().unwrap().push_back(told);
        tell(told(Primary, true, true));
        // One the window never hears of, before the release it does.
        tell(told(Extra1, true, false));
        tell(told(Primary, false, true));
        assert!(pointer.gave(Primary, true));
        assert!(pointer.gave(Primary, false));
        assert!(pointer.shared.buttons.lock().unwrap().is_empty());
        // Told while the window waits: the thread hears the same event a moment later.
        let shared = pointer.shared.clone();
        let late = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(1));
            (shared.buttons.lock().unwrap()).push_back(told(Secondary, true, true));
            shared.told.notify_all();
        });
        assert!(pointer.gave(Secondary, true));
        late.join().unwrap();
        // Never told (a touch): the window gives it itself, after a short wait.
        let asked = Instant::now();
        assert!(!pointer.gave(Primary, true));
        assert!(asked.elapsed() >= WAIT);
    }

    #[test]
    fn a_place_limn_does_not_agree_with_is_not_trusted() {
        let pointer = Pointer {
            shared: Arc::default(),
            thread: None,
        };
        let Laid { page, above, keys } = laid();
        let lay = |at: (f32, f32), limn: bool| {
            let pointer_at = Some((egui::pos2(at.0, at.1), limn));
            pointer.lay(page.clone(), above.clone(), keys, pointer_at);
            let given = pointer.shared.laid.lock().unwrap().page.is_some();
            (pointer.moves(), given)
        };
        // On the page, and limn says so; over Newen's menu, and limn says it is not the page.
        assert_eq!(lay((150.0, 70.0), true), (true, true));
        assert_eq!(lay((450.0, 70.0), false), (true, true));
        // limn finds something over the page that these rectangles do not have: the window
        // gives the page its moves itself, and the thread gives it nothing.
        assert_eq!(lay((150.0, 70.0), false), (false, false));
        assert_eq!(lay((150.0, 70.0), true), (true, true));
    }

    /// On a real compositor: the seat is bound, the thread runs and it ends when dropped.
    /// Nothing is shown and no input is made; skipped where there is no Wayland to talk to.
    #[test]
    fn the_thread_hears_a_real_compositor_and_ends_when_dropped() {
        if std::env::var_os("WAYLAND_DISPLAY").is_none() {
            return;
        }
        let Ok(connection) = Connection::connect_to_env() else {
            return;
        };
        // A surface of this test's own stands for the window's: never mapped.
        struct Find(Option<wayland_client::protocol::wl_compositor::WlCompositor>);
        impl Dispatch<WlRegistry, ()> for Find {
            fn event(
                found: &mut Self,
                registry: &WlRegistry,
                event: wl_registry::Event,
                _: &(),
                _: &Connection,
                qh: &QueueHandle<Self>,
            ) {
                if let wl_registry::Event::Global {
                    name, interface, ..
                } = event
                {
                    if interface == "wl_compositor" {
                        found.0 = Some(registry.bind(name, 1, qh, ()));
                    }
                }
            }
        }
        wayland_client::delegate_noop!(Find: ignore wayland_client::protocol::wl_compositor::WlCompositor);
        wayland_client::delegate_noop!(Find: ignore WlSurface);
        let mut events = connection.new_event_queue();
        let qh = events.handle();
        let _registry = connection.display().get_registry(&qh, ());
        let mut find = Find(None);
        events.roundtrip(&mut find).unwrap();
        let compositor = find.0.as_ref().expect("a compositor");
        let window = compositor.create_surface(&qh, ());
        let given = Arc::new(Mutex::new(0));
        let count = given.clone();
        let give: Give = Arc::new(move |_, _| {
            *count.lock().unwrap() += 1;
            true
        });
        let pointer = Pointer::on(connection, window, give).expect("a seat");
        std::thread::sleep(Duration::from_millis(300));
        assert!(pointer.hears());
        // The window's own thread reads the same connection: neither waits on the other. A
        // reader that kept the other out until its own wait ended would take a quarter of a
        // second for each of these.
        let asked = Instant::now();
        for _ in 0..200 {
            events.roundtrip(&mut find).unwrap();
        }
        assert!(
            asked.elapsed() < Duration::from_secs(2),
            "{:?}",
            asked.elapsed()
        );
        assert!(pointer.hears());
        let asked = Instant::now();
        drop(pointer);
        assert!(asked.elapsed() < Duration::from_millis(2 * LOOK as u64));
        // The pointer never was on a surface that is not shown.
        assert_eq!(*given.lock().unwrap(), 0);
    }
}
