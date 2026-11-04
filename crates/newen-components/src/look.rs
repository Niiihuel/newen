//! Newen's tokens. Neutrals measured from Claude Desktop's Code surface at 1408×844
//! git colors from its static tokens; the accent is Newen's own (spec.md §6).

use limn::{Color, Theme};
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};

static DARK: AtomicBool = AtomicBool::new(false);
/// The conversation's text size and column, as Settings chose them; read every frame.
static CHAT: AtomicU32 = AtomicU32::new(14);
static WIDTH: AtomicU32 = AtomicU32::new(770);

/// The conversation's words at Settings' "Transcript text size", as Desktop sizes them.
pub fn set_text_size(size: newen_model::TextSize) {
    use newen_model::TextSize::*;
    // Desktop's chat text sizes s, m and l (`.875rem` the
    // default), the transcript's and the composer's.
    let px = match size {
        Small => 13,
        Medium => 14,
        Large => 15,
    };
    CHAT.store(px, Ordering::Relaxed);
}

pub fn chat() -> f32 {
    CHAT.load(Ordering::Relaxed) as f32
}

/// Settings' "Transcript width", as Desktop's: its 48rem
/// column with room for the scrollbar, 960 and 1280.
pub fn set_width(width: newen_model::Width) {
    use newen_model::Width::*;
    let px = match width {
        Narrow => 770,
        Medium => 960,
        Wide => 1280,
    };
    WIDTH.store(px, Ordering::Relaxed);
}

/// The transcript and composer column, at most.
pub fn column() -> f32 {
    WIDTH.load(Ordering::Relaxed) as f32
}

/// Draw dark from now on, or light again; the tooltips follow at once.
pub fn set_dark(dark: bool) {
    DARK.store(dark, Ordering::Relaxed);
    install();
}

pub fn is_dark() -> bool {
    DARK.load(Ordering::Relaxed)
}

const fn hex(rgb: u32) -> Color {
    Color::rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
}

/// Each color as a function of the light and dark looks. The dark one is the person's own
/// Desktop in dark mode (its canvas #191919, text #EFEFEF, muted #9C9C9C, filled
/// chips #303030, raised bands #242424, hairlines #353535, git #32D74B / #FF2C56); what that
/// capture does not show is inferred, and says so.
macro_rules! tones {
    ($($(#[$doc:meta])* $name:ident = $light:expr, $dark:expr;)*) => {
        $(
            $(#[$doc])*
            pub fn $name() -> Color {
                if is_dark() { $dark } else { $light }
            }
        )*
    };
}

tones! {
    /// The sidebar's fill (dark: inferred, the capture hides it).
    sidebar = hex(0xfafafa), hex(0x1f1f1f);
    divider = hex(0xe5e5e5), hex(0x303030);
    /// The person's bubble (dark: inferred, as the filled chips).
    user_bubble = hex(0xf3f3f3), hex(0x303030);
    muted = hex(0x7d7d7d), hex(0x9c9c9c);
    /// A command's box and inline code.
    command_box = hex(0xf3f3f3), hex(0x2a2a2a);
    awaiting = hex(0xf0ac55), hex(0xf0ac55);
    /// The ring round the waiting dot and the edge of its "Needs your input" pill
    /// (dark: inferred).
    awaiting_edge = hex(0xfee9bb), hex(0x5c4520);
    running = hex(0x3b5bdb), hex(0x3b5bdb);
    /// Settings, as Desktop's draws it at 1×: the page's ground, the
    /// column of pages beside it and their edge, the hairline under the title and between
    /// rows, the page in view, the search field, a button's fill, the theme's track and a
    /// switch that is on.
    settings_page = hex(0xffffff), hex(0x1f1f1f);
    /// The card's outline: none to see in light (only its shadow), #2A2A2A in dark, quieter
    /// than the hairline under its title.
    settings_outline = hex(0xffffff), hex(0x2a2a2a);
    /// Customize's pages (Skills and Plugins, measured dark at 1×;
    /// light inferred): a card's fill, an item's icon tile, the line between items, a count's
    /// pill, an outlined button's edge, a tab's chip and its edge, a filled button.
    customize_card = hex(0xf4f4f4), hex(0x2a2a2a);
    customize_tile = hex(0xefefef), hex(0x0b0b0b);
    customize_line = hex(0xf0f0f0), hex(0x272727);
    customize_count = hex(0xe8e8e8), hex(0x333333);
    customize_edge = hex(0xd9d9d9), hex(0x4b4b4b);
    customize_tab = hex(0xe9e9e9), hex(0x3f3f3f);
    customize_tab_edge = hex(0xdadada), hex(0x4a4a4a);
    customize_button = hex(0xececec), hex(0x404040);
    settings_nav = hex(0xffffff), hex(0x191919);
    settings_nav_edge = hex(0xe5e5e5), hex(0x303030);
    settings_title_line = hex(0xe0e0e0), hex(0x3a3a3a);
    settings_row_line = hex(0xf3f3f3), hex(0x2a2a2a);
    settings_current = hex(0xdadada), hex(0x3b3b3b);
    settings_field = hex(0xffffff), hex(0x242424);
    settings_field_edge = hex(0xebebeb), hex(0x2c2c2c);
    settings_button = hex(0xf2f2f2), hex(0x404040);
    settings_track = hex(0xf3f3f3), hex(0x2a2a2a);
    /// The theme's chosen icon: a chip of the page's white on the track, and its edge
    /// (dark: inferred).
    settings_chip = hex(0xffffff), hex(0x3b3b3b);
    settings_chip_edge = hex(0xe7e7e7), hex(0x4a4a4a);
    switch_on = hex(0x257de0), hex(0x257de0);
    /// A switch that is off: its pale track under the white thumb
    /// (dark: inferred).
    switch_off = hex(0xcecece), hex(0x4d4d4d);
    /// A session named in the conversation: Desktop's blue chip
    /// (light: inferred).
    mention = hex(0x1f63b8), hex(0x8ab4f0);
    mention_fill = hex(0xe8f0fb), hex(0x1d2d42);
    /// A failed step's label (dark: inferred).
    failed = hex(0xa42528), hex(0xff6b6b);
    removed = hex(0xcd2054), hex(0xff2c56);
    /// The hairline under the top bar and pane headers, a shade darker than the divider.
    pane_border = hex(0xe0e0e0), hex(0x303030);
    /// The terminal's cursor, the blue of its ANSI palette (dark: inferred).
    cursor = hex(0x0073e6), hex(0x3b8eea);
    /// The selected sidebar row (dark: inferred).
    selected = hex(0xe0e0e0), hex(0x303030);
    /// A filled main button under the pointer (inferred: the hover was not captured).
    primary_hover = hex(0x333333), hex(0xd6d6d6);
    /// The confirmation dialog's Cancel and its soft destructive action.
    cancel = hex(0xf2f2f2), hex(0x303030);
    danger_fill = hex(0xfbe7e7), hex(0x3a1e1e);
    danger_border = hex(0xdc3c3d), hex(0xdc3c3d);
    danger_ink = hex(0xa42528), hex(0xff7b7b);
    /// The dialog's backdrop: black at about 41 %, #969696 over white.
    scrim = Color::rgba(0, 0, 0, 105), Color::rgba(0, 0, 0, 150);
    /// The permission card's hairline and the ink of the command it shows.
    card_border = hex(0xebebeb), hex(0x353535);
    command_ink = hex(0x616161), hex(0xb0b0b0);
    /// The context meter: what is past its first series, the
    /// autocompact buffer and the deferred, and the ground and the free share.
    chart_reference = hex(0x898781), hex(0x97958d);
    chart_muted = hex(0xc3c2b7), hex(0x5f5e5a);
    chart_tint = Color::rgba(0x89, 0x87, 0x81, 38), Color::rgba(0x97, 0x95, 0x8d, 38);
    /// A boundary under the pointer or being dragged.
    handle = hex(0x257de0), hex(0x257de0);
    /// The composer: its box's edge and shadow, its buttons under the pointer and
    /// with their menu open, the footer chips likewise, and the amber Manual reads in.
    composer_border = hex(0xd9d9d9), hex(0x404040);
    composer_shadow = Color::rgba(0, 0, 0, 16), Color::rgba(0, 0, 0, 60);
    composer_hover = hex(0xe5e5e5), hex(0x3a3a3a);
    composer_open = hex(0xd2d2d2), hex(0x444444);
    chip_hover = hex(0xe0e0e0), hex(0x303030);
    chip_open = hex(0xcacaca), hex(0x3a3a3a);
    manual = hex(0x794c02), hex(0xe6b35c);
    /// Hover fills: a sidebar row, a selected one, an icon button, and
    /// an active toggle (dark: inferred).
    row_hover = hex(0xededed), hex(0x272727);
    selected_hover = hex(0xd5d5d5), hex(0x383838);
    tool_hover = hex(0xe5e5e5), hex(0x303030);
    active_hover = hex(0xd8d8d8), hex(0x3a3a3a);
    /// Code's inks, Newen's own and apart from the status colours: clay keywords, sage
    /// strings, sand numbers, slate functions, plum types and teal properties over a warm ink,
    /// with stone comments and beni interpolation braces. The editor, code blocks and diffs
    /// draw with them.
    code_text = hex(0x2b2722), hex(0xe8e3da);
    code_comment = hex(0x8a8377), hex(0x8b857c);
    code_keyword = hex(0xb04a2c), hex(0xe0876a);
    code_string = hex(0x567532), hex(0xb3c48d);
    code_number = hex(0xa0611a), hex(0xe6b673);
    code_function = hex(0x2f6391), hex(0x9cbad6);
    code_type = hex(0x7a4fa0), hex(0xc7a6dd);
    code_property = hex(0x2d7468), hex(0x8cbfb3);
    code_interpolation = hex(0xb5504c), hex(0xd77d78);
    /// What a change adds, removes or changes, as crisp as the cursor's blue: green, red and
    /// that blue, for the diff's markers and bar, the gutter's marks and every +N −N.
    diff_added = hex(0x1e9e3c), hex(0x32d74b);
    diff_removed = hex(0xcd2054), hex(0xff2c56);
    diff_changed = hex(0x0073e6), hex(0x3b8eea);
    /// A diff's rows: the tint across an added or removed one, and the stronger one behind
    /// its changed words.
    diff_add_row = Color::rgba(0x1e, 0x9e, 0x3c, 22), Color::rgba(0x32, 0xd7, 0x4b, 24);
    diff_add_word = Color::rgba(0x1e, 0x9e, 0x3c, 60), Color::rgba(0x32, 0xd7, 0x4b, 64);
    diff_del_row = Color::rgba(0xcd, 0x20, 0x54, 20), Color::rgba(0xff, 0x2c, 0x56, 24);
    diff_del_word = Color::rgba(0xcd, 0x20, 0x54, 56), Color::rgba(0xff, 0x2c, 0x56, 64);
    /// The greeting's spark, Claude's orange.
    spark = hex(0xd97757), hex(0xd97757);
    /// The usage ring's track: Desktop's #E5E5E5 at rest, #CDCDCD under the pointer, met
    /// halfway so it shows on both.
    ring_track = hex(0xd6d6d6), hex(0x404040);
    /// The effort slider: its track (#454545 dark),
    /// the fill laid over it up to the thumb (Desktop's 20 % ink, 35 % light on dark:
    /// #868686 over the track), its marks at 25 %, and the thumb's edge.
    slider_track = hex(0xe6e6e6), hex(0x454545);
    slider_fill = Color::rgba(0x19, 0x19, 0x19, 51), Color::rgba(0xff, 0xff, 0xff, 89);
    slider_mark = Color::rgba(0x19, 0x19, 0x19, 64), Color::rgba(0xff, 0xff, 0xff, 64);
    slider_thumb_edge = Color::rgba(0x19, 0x19, 0x19, 30), Color::rgba(0xff, 0xff, 0xff, 0);
    /// Ultracode's violet: the reference selector's accent, lighter on dark (inferred).
    pro = hex(0x8c73c9), hex(0xa996e0);
    /// An active toggle or tab.
    active = hex(0xe5e5e5), hex(0x303030);
    /// The branch bar over the composer and its chips: the person's dark Desktop draws them
    /// #252525 and #454545; the light ones are inferred.
    branch_bar = hex(0xf5f5f5), hex(0x252525);
    branch_chip = hex(0xffffff), hex(0x454545);
    branch_chip_edge = hex(0xe3e3e3), hex(0x454545);
    branch_chip_hover = hex(0xf0f0f0), hex(0x505050);
}

// Sizes are Desktop's at 1× (Xvfb at 96 dpi, 1408×844). The earlier Xephyr captures drew
// at 0.75×, so sizes read from them are scaled.
/// Sidebar width with its 1 px border.
pub const SIDEBAR_WIDTH: f32 = 288.0;
/// The right panel: 280 px cards with 8 px around them.
pub const PANEL_WIDTH: f32 = 296.0;
pub const TOP_BAR: f32 = 36.0;
pub const PANE_HEADER: f32 = 43.0;
/// UI text; the conversation's text is larger.
pub const BODY: f32 = 14.0;
/// Section labels and captions.
pub const SMALL: f32 = 13.0;

pub fn theme() -> Theme {
    Theme {
        background: palette().surface,
        foreground: palette().text,
        accent: running(),
    }
}

/// Desktop's surfaces are its canvas under a hairline, not tinted levels; menus
/// and cards lift with a faint shadow only.
pub fn palette() -> limn::ComponentPalette {
    let dark = is_dark();
    let pick = |light: Color, dark_one: Color| if dark { dark_one } else { light };
    limn::ComponentPalette {
        surface: pick(hex(0xffffff), hex(0x191919)),
        raised: pick(hex(0xffffff), hex(0x242424)),
        hover: pick(
            Color::rgba(0x19, 0x19, 0x19, 13),
            Color::rgba(0xff, 0xff, 0xff, 13),
        ),
        selected: pick(
            Color::rgba(0x19, 0x19, 0x19, 31),
            Color::rgba(0xff, 0xff, 0xff, 31),
        ),
        border: divider(),
        text: pick(hex(0x191919), hex(0xefefef)),
        muted: muted(),
        shadow: pick(Color::rgba(0, 0, 0, 24), Color::rgba(0, 0, 0, 80)),
    }
}

/// The Ultracode track under its pixel field: the reference selector's gray-to-lilac
/// gradient, and on dark the same climb from the dark track (inferred).
pub fn ultra_track() -> Vec<Color> {
    let stops: [u32; 7] = if is_dark() {
        [
            0x454545, 0x464547, 0x4a4650, 0x4e4858, 0x524a60, 0x554b65, 0x574c68,
        ]
    } else {
        [
            0xeeebe9, 0xece9e7, 0xe2dce3, 0xd9d0df, 0xd0c1da, 0xcdbcd9, 0xcbbad8,
        ]
    };
    stops.into_iter().map(hex).collect()
}

/// The color the pixel field starts from at the track's left end: the track's own.
pub fn ultra_left() -> [f32; 3] {
    if is_dark() {
        [69.0, 69.0, 69.0]
    } else {
        [210.0, 206.0, 214.0]
    }
}

/// Menus: rows light gray under the pointer; Desktop marks the current row with
/// its check alone. Dark, a menu stands lighter than the page: #303030 with a #373737 edge
/// and #4F4F4F rows under the pointer.
pub fn menu_palette() -> limn::ComponentPalette {
    let palette = palette();
    if is_dark() {
        return limn::ComponentPalette {
            surface: hex(0x303030),
            border: hex(0x373737),
            hover: hex(0x4f4f4f),
            selected: hex(0x4f4f4f),
            ..palette
        };
    }
    limn::ComponentPalette {
        hover: hex(0xf0f0f0),
        selected: palette.raised,
        ..palette
    }
}

/// The composer's "/" and "@" lists, as Desktop's: a menu's surfaces, and the selected row's
/// description on its darker box (`#2C2C2C`; light: the
/// menu's raised, inferred).
pub fn suggestion_palette() -> limn::ComponentPalette {
    let palette = menu_palette();
    if is_dark() {
        return limn::ComponentPalette {
            raised: hex(0x2c2c2c),
            ..palette
        };
    }
    palette
}

/// The composer's editor sits on the card without a fill of its own, as Desktop's does; limn fills both the card and the editor with `selected`, so here it is the
/// card's: white, or the dark composer's #303030.
pub fn composer_palette() -> limn::ComponentPalette {
    limn::ComponentPalette {
        selected: if is_dark() {
            hex(0x303030)
        } else {
            hex(0xffffff)
        },
        ..palette()
    }
}

/// Desktop's tooltips are near-black with white words and a gray shortcut, and
/// white with dark words on a dark page, as the person asked; limn takes one style for every
/// tooltip from its tokens.
pub fn install() {
    let mut tokens = limn::tokens();
    // Shortcuts are words, not key caps, in menus and tooltips alike.
    tokens.shortcut_style = limn::ShortcutStyle::Text;
    let (fill, text, muted) = if is_dark() {
        (hex(0xffffff), hex(0x191919), hex(0x6b6b6b))
    } else {
        (hex(0x0b0b0b), hex(0xffffff), hex(0xa3a3a3))
    };
    tokens.tooltip = Some(limn::TooltipStyle {
        background: fill,
        border: fill,
        keycap: fill,
        text,
        muted,
        shadow: Color::rgba(0, 0, 0, 40),
    });
    limn::set_tokens(tokens);
}

/// The context meter's series in order, Desktop's categorical chart colors.
pub fn chart_series(index: usize) -> Color {
    let light = [0x2a78d6, 0xeb6834, 0x1baf7a, 0xeda100, 0xe87ba4, 0x008300];
    let dark = [0x3987e5, 0xd95926, 0x199e70, 0xc98500, 0xd55181, 0x008300];
    hex(if is_dark() { dark } else { light }[index % 6])
}
