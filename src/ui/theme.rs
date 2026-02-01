use gpui::{App, Hsla, Pixels, SharedString, hsla, px};
use gpui_component::Theme;
use gpui_component::theme::ActiveTheme;

pub const FONT_FAMILY_MONO: &str = "SF Mono";
pub const FONT_FAMILY_MONO_FALLBACK: &str = "Menlo";
pub const FONT_FAMILY_MONO_FALLBACK_2: &str = "Monaco";
pub const FONT_FAMILY_SYSTEM: &str = ".AppleSystemUIFont";

pub const FONT_SIZE_XS: Pixels = px(11.);
pub const FONT_SIZE_SM: Pixels = px(12.);
pub const FONT_SIZE_BASE: Pixels = px(13.);
pub const FONT_SIZE_MD: Pixels = px(14.);
pub const FONT_SIZE_LG: Pixels = px(16.);
pub const FONT_SIZE_XL: Pixels = px(18.);

pub const SPACING_XS: Pixels = px(4.);
pub const SPACING_SM: Pixels = px(8.);
pub const SPACING_MD: Pixels = px(12.);
pub const SPACING_BASE: Pixels = px(16.);
pub const SPACING_LG: Pixels = px(20.);
pub const SPACING_XL: Pixels = px(24.);
pub const SPACING_2XL: Pixels = px(32.);

pub const WINDOW_MIN_WIDTH: Pixels = px(900.);
pub const WINDOW_MIN_HEIGHT: Pixels = px(600.);
pub const WINDOW_DEFAULT_WIDTH: Pixels = px(1000.);
pub const WINDOW_DEFAULT_HEIGHT: Pixels = px(700.);

// Sidebar width values (f32 is the single source of truth)
pub const SIDEBAR_MIN_WIDTH_F32: f32 = 180.0;
pub const SIDEBAR_MAX_WIDTH_F32: f32 = 400.0;
pub const SIDEBAR_DEFAULT_WIDTH_F32: f32 = 300.0;

// Pixels versions derived from f32 values
pub const SIDEBAR_MIN_WIDTH: Pixels = px(SIDEBAR_MIN_WIDTH_F32);
pub const SIDEBAR_MAX_WIDTH: Pixels = px(SIDEBAR_MAX_WIDTH_F32);
pub const SIDEBAR_DEFAULT_WIDTH: Pixels = px(SIDEBAR_DEFAULT_WIDTH_F32);
pub const SIDEBAR_SECTION_HEIGHT: Pixels = px(28.);
pub const SIDEBAR_ITEM_HEIGHT: Pixels = px(32.);
pub const SIDEBAR_TOGGLE_SIZE: Pixels = px(28.);
pub const SIDEBAR_TOGGLE_ICON_SIZE: Pixels = px(18.);
pub const SIDEBAR_RAIL_WIDTH: Pixels = px(48.);
pub const SIDEBAR_ICON_SIZE: Pixels = px(16.);
pub const SIDEBAR_ICON_CONTAINER_WIDTH: Pixels = px(24.);

pub const HEADER_HEIGHT: Pixels = px(44.);
pub const HEADER_ICON_SIZE: Pixels = px(20.);
pub const HEADER_CLEARANCE_HEIGHT: Pixels = px(32.);
pub const COPY_BUTTON_SIZE: Pixels = px(32.);
pub const COPY_BUTTON_OFFSET: Pixels = px(32.);

pub const CONTENT_PADDING: Pixels = px(20.);
pub const CONTENT_LINE_HEIGHT: f32 = 1.5;

pub const BORDER_RADIUS_SM: Pixels = px(4.);
pub const BORDER_RADIUS_MD: Pixels = px(6.);
pub const BORDER_RADIUS_LG: Pixels = px(8.);
pub const BORDER_WIDTH: Pixels = px(1.);
pub const DIVIDER_HEIGHT: Pixels = px(1.);

pub const ANIMATION_FAST_MS: u64 = 100;
pub const ANIMATION_NORMAL_MS: u64 = 200;
pub const ANIMATION_SLOW_MS: u64 = 300;

#[derive(Clone, Copy, Debug)]
pub struct KopiColors {
    pub background: Hsla,
    pub background_secondary: Hsla,
    pub sidebar_background: Hsla,
    pub foreground: Hsla,
    pub foreground_muted: Hsla,
    pub border: Hsla,
    pub divider: Hsla,
    pub selection: Hsla,
    pub hover: Hsla,
    pub accent: Hsla,
}

impl KopiColors {
    pub fn light() -> Self {
        Self {
            background: hsla(0., 0., 1., 1.),
            background_secondary: hsla(0., 0., 0.97, 1.),
            sidebar_background: hsla(0., 0., 0.96, 1.),
            foreground: hsla(0., 0., 0.1, 1.),
            foreground_muted: hsla(0., 0., 0.35, 1.),
            border: hsla(0., 0., 0.82, 1.),
            divider: hsla(0., 0., 0.88, 1.),
            selection: hsla(0., 0., 0.88, 1.),
            hover: hsla(0., 0., 0.92, 1.),
            accent: hsla(0., 0., 0.2, 1.),
        }
    }

    pub fn dark() -> Self {
        Self {
            background: hsla(0., 0., 0.11, 1.),
            background_secondary: hsla(0., 0., 0.14, 1.),
            sidebar_background: hsla(0., 0., 0.09, 1.),
            foreground: hsla(0., 0., 0.93, 1.),
            foreground_muted: hsla(0., 0., 0.55, 1.),
            border: hsla(0., 0., 0.22, 1.),
            divider: hsla(0., 0., 0.18, 1.),
            selection: hsla(0., 0., 0.22, 1.),
            hover: hsla(0., 0., 0.16, 1.),
            accent: hsla(0., 0., 0.75, 1.),
        }
    }
}

pub fn configure_kopi_theme(cx: &mut App) {
    Theme::global_mut(cx).font_size = FONT_SIZE_BASE;

    let available_fonts = cx.text_system().all_font_names();
    let font_family: SharedString = if available_fonts.contains(&FONT_FAMILY_MONO.to_string()) {
        FONT_FAMILY_MONO.into()
    } else if available_fonts.contains(&FONT_FAMILY_MONO_FALLBACK.to_string()) {
        FONT_FAMILY_MONO_FALLBACK.into()
    } else if available_fonts.contains(&FONT_FAMILY_MONO_FALLBACK_2.to_string()) {
        FONT_FAMILY_MONO_FALLBACK_2.into()
    } else {
        FONT_FAMILY_SYSTEM.into()
    };

    Theme::global_mut(cx).font_family = font_family;
}

pub fn get_kopi_colors(cx: &App) -> KopiColors {
    let theme = cx.theme();
    if theme.is_dark() {
        KopiColors::dark()
    } else {
        KopiColors::light()
    }
}

pub trait KopiStyleExt {
    fn sidebar_container(self, colors: &KopiColors) -> Self;
    fn content_container(self, colors: &KopiColors) -> Self;
    fn header_bar(self, colors: &KopiColors) -> Self;
    fn sidebar_section_header(self, colors: &KopiColors) -> Self;
    fn sidebar_item(self, colors: &KopiColors, is_selected: bool) -> Self;
}

impl<E: gpui::Styled> KopiStyleExt for E {
    fn sidebar_container(self, colors: &KopiColors) -> Self {
        self.bg(colors.sidebar_background)
            .border_r(BORDER_WIDTH)
            .border_color(colors.border)
            .min_w(SIDEBAR_MIN_WIDTH)
            .max_w(SIDEBAR_MAX_WIDTH)
    }

    fn content_container(self, colors: &KopiColors) -> Self {
        self.bg(colors.background)
            .pt(CONTENT_PADDING)
            .pb(CONTENT_PADDING)
            .pl(CONTENT_PADDING)
            .flex_1()
    }

    fn header_bar(self, colors: &KopiColors) -> Self {
        self.bg(colors.background)
            .h(HEADER_HEIGHT)
            .border_b(BORDER_WIDTH)
            .border_color(colors.border)
            .px(SPACING_BASE)
    }

    fn sidebar_section_header(self, colors: &KopiColors) -> Self {
        self.h(SIDEBAR_SECTION_HEIGHT)
            .px(SPACING_MD)
            .text_size(FONT_SIZE_XS)
            .text_color(colors.foreground_muted)
    }

    fn sidebar_item(self, colors: &KopiColors, is_selected: bool) -> Self {
        let bg = if is_selected {
            colors.selection
        } else {
            gpui::transparent_black()
        };

        self.h(SIDEBAR_ITEM_HEIGHT)
            .px(SPACING_MD)
            .bg(bg)
            .text_size(FONT_SIZE_SM)
            .text_color(colors.foreground)
            .rounded(BORDER_RADIUS_SM)
            .mx(SPACING_XS)
    }
}

#[derive(Clone, Copy, Debug)]
pub enum IconSize {
    Small,
    Medium,
    Large,
    XLarge,
}

impl IconSize {
    pub fn pixels(self) -> Pixels {
        match self {
            IconSize::Small => px(12.),
            IconSize::Medium => px(16.),
            IconSize::Large => px(20.),
            IconSize::XLarge => px(24.),
        }
    }
}
