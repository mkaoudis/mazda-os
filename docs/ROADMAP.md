# Roadmap

## Done

- Read-only platform capability boundary and deterministic mock backend.
- 800×480 desktop simulator with Commander input.
- Platform-neutral UI and renderer abstraction.
- Bench-only, firmware-gated, report-only CMU USB payload preparer with off-target tests.
- CI: formatting, strict Clippy, and workspace tests.

## Next — bench CMU characterization

- Boot a spare CMU with vehicle buses physically disconnected.
- Validate the privileged USB entry mechanism only on that isolated spare unit; the internal
  firmware gate runs after entry and is not a containment boundary.
- Confirm SoC/core topology, memory, storage, firmware, and kernel configuration.
- Inventory Wayland/EGL/GLES, Vivante libraries, input devices, and stock HMI IPC boundaries.
- Produce a reproducible read-only hardware/software report.

## Native CMU proof

- Cross-compile a minimal ARM application.
- Open an EGL/GLES surface and render a stable 800×480 scene.
- Consume Commander input without vehicle-bus access.
- Exit cleanly to the stock HMI.

## Integration

- Implement an allowlisted read-only CMU backend and capture trace fixtures.
- Launch the experimental UI manually in-car while stationary, preserving stock camera/audio/startup/shutdown behavior and crash recovery.
- Measure CPU, GPU, memory, thermals, and boot impact.

## Later

- Media integration and richer visualizations.
- Production Wayland/GLES renderer.
- Bluetooth/audio research.
- Evaluate the OEM 8-inch display and, separately, higher-resolution panel experiments.
