use mazda_core::CommanderEvent;
use mazda_mock::MockMazda;
use mazda_ui::{Framebuffer, UiModel, DISPLAY_HEIGHT, DISPLAY_WIDTH};
use minifb::{Key, KeyRepeat, Scale, Window, WindowOptions};

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

    while window.is_open() && !window.is_key_down(Key::Escape) {
        for key in window.get_keys_pressed(KeyRepeat::Yes) {
            if let Some(event) = commander_event(key) {
                ui.handle_commander(event);
            }
        }

        ui.refresh(&mazda);
        ui.render(&mut framebuffer);
        window.update_with_buffer(framebuffer.pixels(), DISPLAY_WIDTH, DISPLAY_HEIGHT)?;
    }

    Ok(())
}

const fn commander_event(key: Key) -> Option<CommanderEvent> {
    match key {
        Key::Right | Key::Down => Some(CommanderEvent::RotateClockwise),
        Key::Left | Key::Up => Some(CommanderEvent::RotateCounterClockwise),
        Key::Enter | Key::Space => Some(CommanderEvent::Select),
        Key::Backspace => Some(CommanderEvent::Back),
        Key::H => Some(CommanderEvent::Home),
        Key::M => Some(CommanderEvent::Music),
        Key::N => Some(CommanderEvent::Navigation),
        Key::F => Some(CommanderEvent::Favorites),
        _ => None,
    }
}
