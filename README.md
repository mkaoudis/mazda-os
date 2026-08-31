# mazda-os

An experimental, open-source infotainment UI and runtime for first-generation Mazda Connect systems, developed desktop-first and designed to preserve the vehicle's stock control systems.

> [!IMPORTANT]
> This project intentionally does **not** provide CAN/LIN transmit APIs, VIP firmware flashing, or arbitrary vehicle-control interfaces. The initial CMU integration is read-only by design.

## Goals

- Build a modern UI for the 800×480 Mazda Connect display.
- Develop and test almost everything off-car with deterministic mocks and trace replay.
- Reuse the stock CMU for display, media, Commander input, and other infotainment functions where practical.
- Treat vehicle state as read-only application input.
- Preserve the factory reverse camera, audio routing, shutdown behavior, and other stock fallbacks until each integration is understood.

## Non-goals

- Modifying powertrain, braking, steering, restraint, ADAS, or other safety-critical systems.
- Flashing or replacing the CMU's Vehicle Information Processor (VIP) firmware.
- Sending arbitrary CAN or LIN frames.
- Providing a generic bridge from application code to raw vehicle buses or device nodes.

## Repository layout

```text
apps/
  desktop/       800×480 desktop simulator
crates/
  mazda-core/    Read-only domain model and platform capability boundary
  mazda-mock/    Deterministic development backend
  mazda-ui/      Platform-neutral UI model, renderer trait, software framebuffer
docs/
  ARCHITECTURE.md
  ROADMAP.md
  SAFETY.md
```

## Development model

```text
                     UI / application
                           │
                           ▼
                    MazdaReadOnly
                           │
             ┌─────────────┴─────────────┐
             │                           │
       desktop/mock                future CMU adapter
             │                           │
      fake scenarios              allowlisted reads only
```

The application layer never receives a raw D-Bus connection, CAN socket, SPI device, `/dev/mem`, or shell-execution capability.

The UI also sits behind a deliberately small `Renderer` trait. The desktop implementation renders to a software framebuffer; the intended CMU implementation can later render the same UI through EGL/OpenGL ES without changing application behavior.

## Run the simulator

```bash
cargo run -p mazda-desktop
```

The window is the factory display resolution: **800×480**. Keyboard controls emulate the Commander input:

- Arrow keys: rotate Commander knob
- Enter / Space: select
- Backspace: back
- H: Home
- M: Music
- N: Navigation
- F: Favorites
- Escape: quit

Run verification with:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

## Status

Phase 1 desktop simulator. No code in this repository should be assumed safe for installation in a vehicle yet.

## Disclaimer

This project is not affiliated with or endorsed by Mazda Motor Corporation. Mazda and Mazda Connect are trademarks of their respective owners.
