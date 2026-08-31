# Architecture

## Principle: capabilities, not conventions

"Do not write to the car" should be enforced by the interfaces available to application code, not by comments or developer discipline.

The core crate therefore exposes a narrow read-only trait. UI code can observe state and consume input events, but it cannot request arbitrary transport operations.

## Layers

```text
┌──────────────────────────────────────────┐
│ UI / renderer / application state       │
└─────────────────────┬────────────────────┘
                      │ typed read-only API
┌─────────────────────▼────────────────────┐
│ mazda-core                               │
│ domain types + MazdaReadOnly trait       │
└─────────────────────┬────────────────────┘
                      │
          ┌───────────┴───────────┐
          │                       │
┌─────────▼────────┐    ┌─────────▼────────────┐
│ mazda-mock       │    │ future CMU backend   │
│ scenarios/replay │    │ allowlisted reads    │
└──────────────────┘    └──────────────────────┘
```

## What must never leak upward

The application layer must not receive:

- a raw CAN socket or CAN transmit primitive;
- a LIN transmit primitive;
- a raw D-Bus connection;
- `/dev/spidev*`, `/dev/mem`, or equivalent low-level device access;
- a VIP firmware update primitive;
- arbitrary shell/process execution on the CMU;
- arbitrary filesystem write access.

If the eventual CMU backend needs an unsafe or privileged mechanism internally to *read* data, that mechanism remains private to the backend and is wrapped in the smallest possible allowlisted operation.

## Rendering

Rendering is deliberately absent from the initial scaffold. The target constraints should be established from real hardware before selecting the production renderer:

- target resolution: 800×480 initially;
- expected target: Wayland/EGL/OpenGL ES on the stock CMU;
- desktop renderer should exercise the same application state and layout logic;
- renderer choice must not couple UI code to the CMU transport layer.

## Future CMU adapter

The first real-hardware adapter should support only operations we can prove are observational, such as retrieving current media metadata or consuming Commander/input events from known read-only sources.

Prefer an explicit allowlist of concrete operations over a generic IPC abstraction.
