# mazda-os

A modern replacement frontend/runtime for first-generation Mazda Connect (Gen 6.5), targeting the stock CMU. The long-term goal is to own the infotainment UX while preserving Mazda's integrated display, Commander input, audio, camera, and other stock services where practical.

The project is desktop-first and deliberately **not** a vehicle-control project.

## Approach

- Build and exercise the UI off-car at the factory 800×480 resolution.
- Keep application behavior independent of the graphics backend; desktop uses a software framebuffer, while the CMU target is Wayland/EGL/OpenGL ES.
- Treat vehicle state as typed, read-only application input through `MazdaReadOnly`.
- Reuse stock CMU services only after their interfaces are understood.
- Test on a spare bench CMU before the daily-driver unit, retaining the stock HMI as a fallback during early in-car work.

```text
                  UI / application
                        │
             ┌──────────┴──────────┐
             │                     │
         Renderer             MazdaReadOnly
             │                     │
       desktop / EGL         mock / CMU adapter
```

Application code does not receive raw CAN/LIN access, D-Bus connections, VIP flashing, low-level device access, arbitrary shell execution, or arbitrary filesystem writes. Powertrain, braking, steering, restraints, ADAS, and other safety-critical systems are out of scope.

The first CMU integration is observational. Any future mutating infotainment operation must be individually understood and isolated from vehicle-control buses.

## Repository layout

```text
apps/
  cmu-inspect/    allowlisted, stdout-only CMU metadata collector
  desktop/       800×480 desktop simulator
crates/
  mazda-core/    domain types and read-only capability boundary
  mazda-mock/    deterministic development backend
  mazda-ui/      UI model, renderer trait, software framebuffer
docs/
  BENCH_SETUP.md
  ROADMAP.md
```

## Run the simulator

```bash
cargo run -p mazda-desktop
```

Controls:

- Arrow keys: rotate Commander knob
- Enter / Space: select
- Backspace: back
- H / M: Home or Music → Now Playing
- N: Navigation → Drive
- F: Favorites → Settings
- Escape: quit

Verification:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo test --workspace --locked
```

## Hardware development

Use a spare CMU on a bench before touching the daily-driver unit. See [`docs/BENCH_SETUP.md`](docs/BENCH_SETUP.md).

The first inspection utility prepares a firmware-gated, report-only USB payload for an isolated
Gen-6.5 bench CMU. A passive Mac-to-CMU cable is not supported because both ports are USB hosts.
Read [`docs/CMU_USB_INSPECTION.md`](docs/CMU_USB_INSPECTION.md) before building or connecting
anything.

No code in this repository should yet be assumed safe for installation in a vehicle.

## Disclaimer

This project is not affiliated with or endorsed by Mazda Motor Corporation. Mazda and Mazda Connect are trademarks of their respective owners.
