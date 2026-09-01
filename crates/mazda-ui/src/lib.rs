//! Platform-neutral UI model and rendering primitives for the Mazda Connect display.

use std::borrow::Cow;

use font8x8::{UnicodeFonts, BASIC_FONTS, GREEK_FONTS, HIRAGANA_FONTS, LATIN_FONTS};
use mazda_core::{CommanderEvent, Gear, MazdaReadOnly, MediaState, PlaybackState, VehicleSnapshot};

pub const DISPLAY_WIDTH: usize = 800;
pub const DISPLAY_HEIGHT: usize = 480;
const GLYPH_SIZE: usize = 8;
const CONTENT_WIDTH: usize = 528;
const MAX_METADATA_CHARACTERS: usize = 256;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Color(u32);

impl Color {
    pub const BLACK: Self = Self::rgb(0, 0, 0);
    pub const BACKGROUND: Self = Self::rgb(12, 16, 22);
    pub const PANEL: Self = Self::rgb(24, 31, 41);
    pub const PANEL_ACTIVE: Self = Self::rgb(43, 56, 72);
    pub const TEXT: Self = Self::rgb(237, 241, 245);
    pub const TEXT_MUTED: Self = Self::rgb(143, 154, 166);
    pub const ACCENT: Self = Self::rgb(103, 189, 255);
    pub const ACCENT_SOFT: Self = Self::rgb(41, 85, 119);

    #[must_use]
    pub const fn rgb(red: u8, green: u8, blue: u8) -> Self {
        Self((red as u32) << 16 | (green as u32) << 8 | blue as u32)
    }

    #[must_use]
    pub const fn raw(self) -> u32 {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: usize,
    pub y: usize,
    pub width: usize,
    pub height: usize,
}

impl Rect {
    #[must_use]
    pub const fn new(x: usize, y: usize, width: usize, height: usize) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }
}

/// Small rendering surface intentionally independent of windowing and GPU APIs.
///
/// A future CMU renderer can implement this over EGL/OpenGL ES while desktop
/// development can use the software framebuffer below.
pub trait Renderer {
    fn size(&self) -> (usize, usize);
    fn clear(&mut self, color: Color);
    fn fill_rect(&mut self, rect: Rect, color: Color);
    fn text(&mut self, x: usize, y: usize, text: &str, scale: usize, color: Color);
}

#[derive(Debug, Clone)]
pub struct Framebuffer {
    width: usize,
    height: usize,
    pixels: Vec<u32>,
}

impl Framebuffer {
    #[must_use]
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            pixels: vec![Color::BLACK.raw(); width.saturating_mul(height)],
        }
    }

    #[must_use]
    pub fn mazda_connect() -> Self {
        Self::new(DISPLAY_WIDTH, DISPLAY_HEIGHT)
    }

    #[must_use]
    pub fn pixels(&self) -> &[u32] {
        &self.pixels
    }
}

impl Renderer for Framebuffer {
    fn size(&self) -> (usize, usize) {
        (self.width, self.height)
    }

    fn clear(&mut self, color: Color) {
        self.pixels.fill(color.raw());
    }

    fn fill_rect(&mut self, rect: Rect, color: Color) {
        let x_end = rect.x.saturating_add(rect.width).min(self.width);
        let y_end = rect.y.saturating_add(rect.height).min(self.height);

        for y in rect.y.min(self.height)..y_end {
            let row = y * self.width;
            for x in rect.x.min(self.width)..x_end {
                self.pixels[row + x] = color.raw();
            }
        }
    }

    fn text(&mut self, x: usize, y: usize, text: &str, scale: usize, color: Color) {
        let scale = scale.max(1);
        if x >= self.width || y >= self.height {
            return;
        }

        let glyph_advance = GLYPH_SIZE.saturating_mul(scale);
        let mut cursor_x = x;

        for character in text.chars() {
            if cursor_x >= self.width {
                break;
            }

            for (row, bits) in glyph(character).iter().copied().enumerate() {
                let glyph_y = y.saturating_add(row.saturating_mul(scale));
                if glyph_y >= self.height {
                    break;
                }

                for column in 0..GLYPH_SIZE {
                    if bits & (1_u8 << column) != 0 {
                        let glyph_x = cursor_x.saturating_add(column.saturating_mul(scale));
                        if glyph_x < self.width {
                            self.fill_rect(Rect::new(glyph_x, glyph_y, scale, scale), color);
                        }
                    }
                }
            }
            cursor_x = cursor_x.saturating_add(glyph_advance);
        }
    }
}

fn glyph(character: char) -> [u8; GLYPH_SIZE] {
    BASIC_FONTS
        .get(character)
        .or_else(|| LATIN_FONTS.get(character))
        .or_else(|| GREEK_FONTS.get(character))
        .or_else(|| HIRAGANA_FONTS.get(character))
        .or_else(|| BASIC_FONTS.get('?'))
        .unwrap_or([0; GLYPH_SIZE])
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    NowPlaying,
    Drive,
    Phone,
    Settings,
}

impl Screen {
    const ALL: [Self; 4] = [Self::NowPlaying, Self::Drive, Self::Phone, Self::Settings];

    const fn label(self) -> &'static str {
        match self {
            Self::NowPlaying => "MUSIC",
            Self::Drive => "DRIVE",
            Self::Phone => "PHONE",
            Self::Settings => "SETTINGS",
        }
    }
}

#[derive(Debug, Clone)]
pub struct UiModel {
    screen: Screen,
    selection: Screen,
    vehicle: VehicleSnapshot,
    media: MediaState,
}

impl UiModel {
    #[must_use]
    pub fn from_source(source: &impl MazdaReadOnly) -> Self {
        Self {
            screen: Screen::NowPlaying,
            selection: Screen::NowPlaying,
            vehicle: source.vehicle_snapshot(),
            media: bounded_media_state(source.media_state()),
        }
    }

    pub fn refresh(&mut self, source: &impl MazdaReadOnly) {
        self.vehicle = source.vehicle_snapshot();
        self.media = bounded_media_state(source.media_state());
    }

    #[must_use]
    pub const fn screen(&self) -> Screen {
        self.screen
    }

    #[must_use]
    pub const fn selection(&self) -> Screen {
        self.selection
    }

    pub fn handle_commander(&mut self, event: CommanderEvent) {
        match event {
            CommanderEvent::RotateClockwise => self.move_selection(1),
            CommanderEvent::RotateCounterClockwise => self.move_selection(-1),
            CommanderEvent::Home | CommanderEvent::Music => {
                self.activate(Screen::NowPlaying);
            }
            CommanderEvent::Navigation => self.activate(Screen::Drive),
            CommanderEvent::Favorites => self.activate(Screen::Settings),
            CommanderEvent::Select => self.screen = self.selection,
            CommanderEvent::Back => self.activate(Screen::NowPlaying),
        }
    }

    pub fn render(&self, renderer: &mut impl Renderer) {
        renderer.clear(Color::BACKGROUND);
        Self::render_header(renderer);
        self.render_navigation(renderer);

        match self.screen {
            Screen::NowPlaying => self.render_now_playing(renderer),
            Screen::Drive => self.render_drive(renderer),
            Screen::Phone => {
                Self::render_placeholder(renderer, "PHONE", "Phone integration comes later.");
            }
            Screen::Settings => {
                Self::render_placeholder(
                    renderer,
                    "SETTINGS",
                    "Desktop simulator / read-only mode",
                );
            }
        }

        Self::render_footer(renderer);
    }

    fn move_selection(&mut self, delta: isize) {
        let current = Screen::ALL
            .iter()
            .position(|screen| *screen == self.selection)
            .unwrap_or_default();
        let count = Screen::ALL.len();
        let next = if delta.is_negative() {
            current.checked_sub(1).unwrap_or(count - 1)
        } else {
            (current + 1) % count
        };
        self.selection = Screen::ALL[next];
    }

    fn activate(&mut self, screen: Screen) {
        self.screen = screen;
        self.selection = screen;
    }

    fn render_header(renderer: &mut impl Renderer) {
        renderer.fill_rect(Rect::new(0, 0, DISPLAY_WIDTH, 64), Color::PANEL);
        renderer.text(24, 21, "MAZDA OS", 2, Color::TEXT);
        renderer.text(600, 24, "DESKTOP SIM", 1, Color::TEXT_MUTED);
        renderer.fill_rect(Rect::new(0, 63, DISPLAY_WIDTH, 1), Color::PANEL_ACTIVE);
    }

    fn render_navigation(&self, renderer: &mut impl Renderer) {
        renderer.fill_rect(Rect::new(0, 64, 176, 368), Color::PANEL);

        for (index, screen) in Screen::ALL.iter().copied().enumerate() {
            let y = 80 + index * 76;
            let active = screen == self.selection;
            let background = if active {
                Color::PANEL_ACTIVE
            } else {
                Color::PANEL
            };
            let text = if active {
                Color::TEXT
            } else {
                Color::TEXT_MUTED
            };
            renderer.fill_rect(Rect::new(12, y, 152, 60), background);
            if active {
                renderer.fill_rect(Rect::new(12, y, 4, 60), Color::ACCENT);
            }
            renderer.text(32, y + 22, screen.label(), 1, text);
        }
    }

    fn render_now_playing(&self, renderer: &mut impl Renderer) {
        let title = non_empty(self.media.title.as_deref()).unwrap_or("Nothing playing");
        let artist = non_empty(self.media.artist.as_deref()).unwrap_or("Unknown artist");
        let album = non_empty(self.media.album.as_deref()).unwrap_or("Unknown album");

        renderer.text(208, 92, "NOW PLAYING", 1, Color::ACCENT);
        render_fitted_text(renderer, 208, 126, title, CONTENT_WIDTH, 3, Color::TEXT);
        render_fitted_text(
            renderer,
            208,
            166,
            artist,
            CONTENT_WIDTH,
            2,
            Color::TEXT_MUTED,
        );
        render_fitted_text(
            renderer,
            208,
            198,
            album,
            CONTENT_WIDTH,
            1,
            Color::TEXT_MUTED,
        );

        let playback = match self.media.playback {
            PlaybackState::Playing => "PLAYING",
            PlaybackState::Paused => "PAUSED",
            PlaybackState::Stopped => "STOPPED",
        };
        renderer.text(208, 250, playback, 1, Color::TEXT_MUTED);

        self.render_vehicle_card(renderer, 208, 326);
    }

    fn render_drive(&self, renderer: &mut impl Renderer) {
        renderer.text(208, 92, "DRIVE", 1, Color::ACCENT);
        renderer.text(208, 132, "VEHICLE STATUS", 2, Color::TEXT);
        self.render_vehicle_card(renderer, 208, 208);
        renderer.text(208, 336, "READ-ONLY DATA PATH", 1, Color::TEXT_MUTED);
        renderer.text(
            208,
            360,
            "No vehicle-control capability is exposed.",
            1,
            Color::TEXT_MUTED,
        );
    }

    fn render_vehicle_card(&self, renderer: &mut impl Renderer, x: usize, y: usize) {
        renderer.fill_rect(Rect::new(x, y, 528, 78), Color::PANEL);

        let speed = format!("{:.0} KM/H", self.vehicle.speed.get());
        let temperature = format!("{:.0} C", self.vehicle.outside_temperature.get());
        renderer.text(x + 20, y + 20, &speed, 2, Color::TEXT);
        renderer.text(
            x + 210,
            y + 23,
            gear_label(self.vehicle.gear),
            1,
            Color::TEXT_MUTED,
        );
        renderer.text(x + 350, y + 23, &temperature, 1, Color::TEXT_MUTED);
    }

    fn render_placeholder(renderer: &mut impl Renderer, title: &str, subtitle: &str) {
        renderer.text(208, 92, title, 1, Color::ACCENT);
        renderer.text(208, 144, subtitle, 1, Color::TEXT_MUTED);
    }

    fn render_footer(renderer: &mut impl Renderer) {
        renderer.fill_rect(Rect::new(0, 432, DISPLAY_WIDTH, 48), Color::PANEL);
        renderer.text(
            24,
            451,
            "ARROWS: MOVE  ENTER: OPEN  BACKSPACE: BACK  H/M/N/F: SHORTCUTS  ESC: QUIT",
            1,
            Color::TEXT_MUTED,
        );
    }
}

fn bounded_media_state(mut media: MediaState) -> MediaState {
    truncate_metadata(&mut media.title);
    truncate_metadata(&mut media.artist);
    truncate_metadata(&mut media.album);
    media
}

fn truncate_metadata(value: &mut Option<String>) {
    let Some(value) = value else {
        return;
    };
    if let Some((index, _)) = value.char_indices().nth(MAX_METADATA_CHARACTERS) {
        value.truncate(index);
    }
}

fn non_empty(value: Option<&str>) -> Option<&str> {
    value.filter(|value| !value.is_empty())
}

fn render_fitted_text(
    renderer: &mut impl Renderer,
    x: usize,
    y: usize,
    text: &str,
    max_width: usize,
    preferred_scale: usize,
    color: Color,
) {
    let (text, scale) = fit_text(text, max_width, preferred_scale);
    renderer.text(x, y, &text, scale, color);
}

fn fit_text(text: &str, max_width: usize, preferred_scale: usize) -> (Cow<'_, str>, usize) {
    let preferred_scale = preferred_scale.max(1);
    let character_count = text.chars().count();
    let unscaled_width = character_count.saturating_mul(GLYPH_SIZE);
    let scale = max_width
        .checked_div(unscaled_width)
        .unwrap_or(preferred_scale)
        .clamp(1, preferred_scale);
    let glyph_advance = GLYPH_SIZE.saturating_mul(scale);
    let visible_characters = max_width / glyph_advance;

    if character_count <= visible_characters {
        return (Cow::Borrowed(text), scale);
    }
    if visible_characters == 0 {
        return (Cow::Borrowed(""), scale);
    }
    if visible_characters <= 3 {
        return (Cow::Owned(".".repeat(visible_characters)), scale);
    }

    let mut fitted = text
        .chars()
        .take(visible_characters - 3)
        .collect::<String>();
    fitted.push_str("...");
    (Cow::Owned(fitted), scale)
}

const fn gear_label(gear: Gear) -> &'static str {
    match gear {
        Gear::Park => "PARK",
        Gear::Reverse => "REVERSE",
        Gear::Neutral => "NEUTRAL",
        Gear::Drive => "DRIVE",
        Gear::Manual => "MANUAL",
        Gear::Unknown => "UNKNOWN",
    }
}

#[cfg(test)]
mod tests {
    use super::{
        bounded_media_state, fit_text, Color, Framebuffer, Rect, Renderer, Screen, UiModel,
        DISPLAY_HEIGHT, DISPLAY_WIDTH, MAX_METADATA_CHARACTERS,
    };
    use mazda_core::{
        CommanderEvent, Gear, MediaState, PlaybackState, SpeedKph, TemperatureC, VehicleSnapshot,
    };

    #[test]
    fn mazda_framebuffer_matches_factory_resolution() {
        let framebuffer = Framebuffer::mazda_connect();
        assert_eq!(framebuffer.size(), (DISPLAY_WIDTH, DISPLAY_HEIGHT));
        assert_eq!(framebuffer.pixels().len(), DISPLAY_WIDTH * DISPLAY_HEIGHT);
    }

    #[test]
    fn fill_rect_is_clipped_to_surface() {
        let mut framebuffer = Framebuffer::new(4, 4);
        framebuffer.clear(Color::BLACK);
        framebuffer.fill_rect(Rect::new(3, 3, 10, 10), Color::ACCENT);

        let changed = framebuffer
            .pixels()
            .iter()
            .filter(|pixel| **pixel == Color::ACCENT.raw())
            .count();
        assert_eq!(changed, 1);
    }

    #[test]
    fn text_clips_extreme_coordinates_without_wrapping() {
        let mut framebuffer = Framebuffer::new(8, 8);
        framebuffer.text(usize::MAX, 0, "A", 1, Color::ACCENT);
        framebuffer.text(0, usize::MAX, "A", 1, Color::ACCENT);
        framebuffer.text(0, 0, "A", usize::MAX, Color::ACCENT);

        assert!(framebuffer
            .pixels()
            .iter()
            .all(|pixel| *pixel == Color::BLACK.raw()));
    }

    #[test]
    fn text_stops_after_the_visible_glyph_capacity() {
        let mut expected = Framebuffer::new(8, 8);
        expected.text(0, 0, "A", 1, Color::ACCENT);

        let mut actual = Framebuffer::new(8, 8);
        actual.text(0, 0, &"A".repeat(1_000_000), 1, Color::ACCENT);

        assert_eq!(actual.pixels(), expected.pixels());
    }

    #[test]
    fn text_renders_supported_unicode_and_a_visible_fallback() {
        let mut latin = Framebuffer::new(8, 8);
        latin.text(0, 0, "é", 1, Color::ACCENT);
        assert!(latin
            .pixels()
            .iter()
            .any(|pixel| *pixel == Color::ACCENT.raw()));

        let mut unsupported = Framebuffer::new(8, 8);
        unsupported.text(0, 0, "🚗", 1, Color::ACCENT);
        let mut fallback = Framebuffer::new(8, 8);
        fallback.text(0, 0, "?", 1, Color::ACCENT);
        assert_eq!(unsupported.pixels(), fallback.pixels());
    }

    #[test]
    fn default_title_scales_down_without_clipping() {
        let title = "Everything in Its Right Place";
        let (fitted, scale) = fit_text(title, 528, 3);

        assert_eq!(fitted, title);
        assert_eq!(scale, 2);
    }

    #[test]
    fn long_text_is_ellipsized_to_the_available_width() {
        let input = "A".repeat(100);
        let (fitted, scale) = fit_text(&input, 64, 3);

        assert_eq!(fitted, "AAAAA...");
        assert_eq!(scale, 1);
    }

    #[test]
    fn media_metadata_is_bounded_before_rendering() {
        let media = bounded_media_state(MediaState {
            title: Some("A".repeat(MAX_METADATA_CHARACTERS + 100)),
            artist: None,
            album: None,
            playback: PlaybackState::Playing,
        });

        assert_eq!(
            media.title.expect("title").chars().count(),
            MAX_METADATA_CHARACTERS
        );
    }

    #[test]
    fn commander_navigation_wraps_and_shortcuts_remain_deterministic() {
        let mut ui = UiModel {
            screen: Screen::NowPlaying,
            selection: Screen::NowPlaying,
            vehicle: VehicleSnapshot {
                speed: SpeedKph::new(0.0),
                gear: Gear::Park,
                outside_temperature: TemperatureC::new(20.0),
            },
            media: MediaState {
                title: None,
                artist: None,
                album: None,
                playback: PlaybackState::Stopped,
            },
        };

        ui.handle_commander(CommanderEvent::RotateCounterClockwise);
        assert_eq!(ui.screen(), Screen::NowPlaying);
        assert_eq!(ui.selection(), Screen::Settings);
        ui.handle_commander(CommanderEvent::Select);
        assert_eq!(ui.screen(), Screen::Settings);
        ui.handle_commander(CommanderEvent::RotateClockwise);
        assert_eq!(ui.screen(), Screen::Settings);
        assert_eq!(ui.selection(), Screen::NowPlaying);
        ui.handle_commander(CommanderEvent::Navigation);
        assert_eq!(ui.screen(), Screen::Drive);
        ui.handle_commander(CommanderEvent::Music);
        assert_eq!(ui.screen(), Screen::NowPlaying);
        ui.handle_commander(CommanderEvent::Favorites);
        assert_eq!(ui.screen(), Screen::Settings);
        ui.handle_commander(CommanderEvent::Back);
        assert_eq!(ui.screen(), Screen::NowPlaying);
        assert_eq!(ui.selection(), Screen::NowPlaying);
    }
}
