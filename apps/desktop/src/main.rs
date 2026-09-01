use std::time::{Duration, Instant};

use mazda_core::CommanderEvent;
use mazda_mock::MockMazda;
use mazda_ui::{Framebuffer, UiModel, DISPLAY_HEIGHT, DISPLAY_WIDTH};
use minifb::{Key, KeyRepeat, Scale, Window, WindowOptions};

const SOURCE_REFRESH_INTERVAL: Duration = Duration::from_millis(100);

fn main() -> Result<(), minifb::Error> {
    let mazda = MockMazda::demo();
    let mut ui = UiModel::from_source(&mazda);
    let mut framebuffer = Framebuffer::mazda_connect();
    let mut window = Window::new(
        "mazda-os — 800x480 desktop simulator",
        DISPLAY_WIDTH,
        DISPLAY_HEIGHT,
        WindowOptions {
            resize: false,
            scale: Scale::X1,
            ..WindowOptions::default()
        },
    )?;

    window.set_target_fps(60);
    let mut last_source_refresh = Instant::now();

    while window.is_open() && !window.is_key_down(Key::Escape) {
        for key in window.get_keys_pressed(KeyRepeat::Yes) {
            if let Some(event) = commander_rotation_event(key) {
                ui.handle_commander(event);
            }
        }
        for key in window.get_keys_pressed(KeyRepeat::No) {
            if let Some(event) = commander_button_event(key) {
                ui.handle_commander(event);
            }
        }

        if last_source_refresh.elapsed() >= SOURCE_REFRESH_INTERVAL {
            ui.refresh(&mazda);
            last_source_refresh = Instant::now();
        }
        ui.render(&mut framebuffer);
        window.update_with_buffer(framebuffer.pixels(), DISPLAY_WIDTH, DISPLAY_HEIGHT)?;
    }

    Ok(())
}

const fn commander_rotation_event(key: Key) -> Option<CommanderEvent> {
    match key {
        Key::Right | Key::Down => Some(CommanderEvent::RotateClockwise),
        Key::Left | Key::Up => Some(CommanderEvent::RotateCounterClockwise),
        _ => None,
    }
}

const fn commander_button_event(key: Key) -> Option<CommanderEvent> {
    match key {
        Key::Enter | Key::Space => Some(CommanderEvent::Select),
        Key::Backspace => Some(CommanderEvent::Back),
        Key::H => Some(CommanderEvent::Home),
        Key::M => Some(CommanderEvent::Music),
        Key::N => Some(CommanderEvent::Navigation),
        Key::F => Some(CommanderEvent::Favorites),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::{commander_button_event, commander_rotation_event};
    use mazda_core::CommanderEvent;
    use minifb::Key;

    #[test]
    fn rotation_keys_map_only_to_repeatable_commander_events() {
        assert_eq!(
            commander_rotation_event(Key::Right),
            Some(CommanderEvent::RotateClockwise)
        );
        assert_eq!(
            commander_rotation_event(Key::Down),
            Some(CommanderEvent::RotateClockwise)
        );
        assert_eq!(
            commander_rotation_event(Key::Left),
            Some(CommanderEvent::RotateCounterClockwise)
        );
        assert_eq!(
            commander_rotation_event(Key::Up),
            Some(CommanderEvent::RotateCounterClockwise)
        );
        assert_eq!(commander_rotation_event(Key::Enter), None);
    }

    #[test]
    fn button_keys_map_only_to_single_press_commander_events() {
        let cases = [
            (Key::Enter, CommanderEvent::Select),
            (Key::Space, CommanderEvent::Select),
            (Key::Backspace, CommanderEvent::Back),
            (Key::H, CommanderEvent::Home),
            (Key::M, CommanderEvent::Music),
            (Key::N, CommanderEvent::Navigation),
            (Key::F, CommanderEvent::Favorites),
        ];

        for (key, event) in cases {
            assert_eq!(commander_button_event(key), Some(event));
        }
        assert_eq!(commander_button_event(Key::Right), None);
    }
}
