# Roadmap

## Phase 0 — scaffold

- [x] Read-only platform capability boundary.
- [x] Deterministic mock backend.
- [x] Desktop smoke-test app.
- [x] Safety and architecture documentation.
- [ ] CI green on GitHub.

## Phase 1 — desktop simulator

- Add an 800×480 desktop window.
- Model Commander knob/buttons and touchscreen input.
- Build representative media, home, and vehicle-info screens.
- Add deterministic scenarios and event-trace replay.
- Establish frame-time and memory budgets.

## Phase 2 — hardware characterization

Using a spare bench CMU first:

- Confirm exact SoC/core topology and memory.
- Inventory Wayland/EGL/GLES versions and Vivante libraries.
- Inventory input devices and stock HMI IPC boundaries.
- Record display timing and compositor behavior.
- Produce a read-only filesystem/process/hardware report.

## Phase 3 — native graphics proof

- Cross-compile a minimal ARM application.
- Open an EGL/GLES surface on the bench CMU.
- Render a stable 800×480 scene.
- Consume Commander input without vehicle-bus access.
- Exit cleanly back to the stock HMI.

## Phase 4 — read-only CMU backend

- Implement only explicitly allowlisted observational operations.
- Map CMU data into `mazda-core` types.
- Add recorded trace fixtures captured from the bench unit.
- Verify that application code has no raw transport capability.

## Phase 5 — in-car experimental launcher

- Launch manually while stationary.
- Preserve stock HMI and reverse-camera fallback.
- Add crash/restart recovery.
- Measure CPU, GPU, memory, thermals, and boot impact.

## Later

- Media integration and richer visualizations.
- Native Wayland/GLES production renderer.
- Evaluate OEM 8-inch display and, separately, higher-resolution panel experiments.
- Bluetooth/audio research without expanding vehicle-control capability.
