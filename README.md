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
  desktop/       Desktop smoke-test application
crates/
  mazda-core/    Read-only domain model and platform capability boundary
  mazda-mock/    Deterministic development backend
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

## Run the scaffold

```bash
cargo run -p mazda-desktop
cargo test --workspace
```

The first milestone is intentionally not graphical: establish the capability boundary and deterministic simulator first, then add the rendering stack behind that boundary.

## Status

Very early research/scaffolding. No code in this repository should be assumed safe for installation in a vehicle yet.

## Disclaimer

This project is not affiliated with or endorsed by Mazda Motor Corporation. Mazda and Mazda Connect are trademarks of their respective owners.
