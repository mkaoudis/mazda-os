//! Platform-neutral UI model and rendering primitives for the Mazda Connect display.

use font8x8::{UnicodeFonts, BASIC_FONTS};
use mazda_core::{CommanderEvent, Gear, MazdaReadOnly, MediaState, PlaybackState, VehicleSnapshot};

pub const DISPLAY_WIDTH: usize = 800;
pub const DISPLAY_HEIGHT: usize = 480;

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
        let mut cursor_x = x;

        for character in text.chars() {
            if let Some(glyph) = BASIC_FONTS.get(character) {
                for (row, bits) in glyph.iter().copied().enumerate() {
                    for column in 0..8 {
                        if bits & (1_u8 << column) != 0 {
                            self.fill_rect(
                                Rect::new(cursor_x + column * scale, y + row * scale, scale, scale),
                                color,
                            );
                        }
                    }
                }
            }
            cursor_x = cursor_x.saturating_add(8 * scale);
        }
    }
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
    vehicle: VehicleSnapshot,
    media: MediaState,
}

impl UiModel {
    #[must_use]
    pub fn from_source(source: &impl MazdaReadOnly) -> Self {
        Self {
            screen: Screen::NowPlaying,
            vehicle: source.vehicle_snapshot(),
            media: source.media_state(),
        }
    }

    pub fn refresh(&mut self, source: &impl MazdaReadOnly) {
        self.vehicle = source.vehicle_snapshot();
        self.media = source.media_state();
    }

    #[must_use]
    pub const fn screen(&self) -> Screen {
        self.screen
    }

    pub fn handle_commander(&mut self, event: CommanderEvent) {
        match event {
            CommanderEvent::RotateClockwise => self.move_selection(1),
            CommanderEvent::RotateCounterClockwise => self.move_selection(-1),
            CommanderEvent::Home | CommanderEvent::Music => self.screen = Screen::NowPlaying,
            CommanderEvent::Navigation => self.screen = Screen::Drive,
            CommanderEvent::Favorites => self.screen = Screen::Settings,
            CommanderEvent::Select | CommanderEvent::Back => {}
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
                Self::render_placeholder(renderer, "SETTINGS", "Desktop simulator / read-only mode");
            }
        }

        Self::render_footer(renderer);
    }

    fn move_selection(&mut self, delta: isize) {
        let current = Screen::ALL
            .iter()
            .position(|screen| *screen == self.screen)
            .unwrap_or_default();
        let count = Screen::ALL.len();
        let next = if delta.is_negative() {
            current.checked_sub(1).unwrap_or(count - 1)
        } else {
            (current + 1) % count
        };
        self.screen = Screen::ALL[next];
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
            let active = screen == self.screen;
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
        let title = self.media.title.as_deref().unwrap_or("Nothing playing");
        let artist = self.media.artist.as_deref().unwrap_or("Unknown artist");
        let album = self.media.album.as_deref().unwrap_or("Unknown album");

        renderer.text(208, 92, "NOW PLAYING", 1, Color::ACCENT);
        renderer.text(208, 126, title, 3, Color::TEXT);
        renderer.text(208, 166, artist, 2, Color::TEXT_MUTED);
        renderer.text(208, 198, album, 1, Color::TEXT_MUTED);

        renderer.fill_rect(Rect::new(208, 250, 528, 6), Color::PANEL_ACTIVE);
        renderer.fill_rect(Rect::new(208, 250, 326, 6), Color::ACCENT);

        let playback = match self.media.playback {
            PlaybackState::Playing => "PLAYING",
            PlaybackState::Paused => "PAUSED",
            PlaybackState::Stopped => "STOPPED",
        };
        renderer.text(208, 280, playback, 1, Color::TEXT_MUTED);

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
            "ARROWS: COMMANDER  ENTER: SELECT  H/M/N/F: SHORTCUTS  ESC: QUIT",
            1,
            Color::TEXT_MUTED,
        );
    }
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
    use super::{Color, Framebuffer, Rect, Renderer, DISPLAY_HEIGHT, DISPLAY_WIDTH};

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
}
