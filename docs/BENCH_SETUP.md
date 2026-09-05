# Bench setup

Use a spare Gen 6.5 CMU before touching the daily-driver unit.

## Minimum hardware

- compatible CX-5 CMU and 7-inch display assembly;
- vehicle-side CMU power connector/pigtail;
- current-limited 12 V bench supply capable of at least 5 A;
- inline 5 A fuse;
- 3.3 V USB-to-UART adapter;
- hookup wire and terminals.

Initially connect only CMU power, ACC, ground, display, and UART. Leave every connection to the **vehicle** buses physically absent.

Verify the connector pinout for the exact CMU before applying power. Connect the bench supply's positive output through the fuse to B+ and ACC, and connect its negative output directly to CMU ground.

```text
laptop ── USB/UART ── CMU ── display

bench PSU + ── 5 A fuse ──┬── CMU B+
                          └── CMU ACC
bench PSU - ───────────────── CMU ground

vehicle CAN/LIN ── disconnected
```

A Commander switch may later be connected directly to the CMU on an isolated bench LIN segment, provided that segment has no electrical connection to the vehicle.

## First hardware pass

Before installing project code, record:

- CMU part number and firmware version;
- CPU, memory, and kernel command line;
- mounted filesystems and free space;
- running processes and loaded modules;
- Wayland/EGL/GLES and Vivante versions;
- relevant input devices.

Prefer observation and filesystem copies over modification. Do not flash the VIP, bootloader, kernel, or daily-driver CMU during characterization.

The firmware-gated collector in `apps/cmu-inspect` covers part of this first pass. It uses a
published v74 update-scanner command injection to run a fixed shell payload and therefore is not a
passive USB read. Follow [`CMU_USB_INSPECTION.md`](CMU_USB_INSPECTION.md) and preserve its stricter
firmware, removable-media, and application-processor-only boundaries.

The update scanner begins root execution before the collector's firmware gate can run. Treat the
gate as a collection-scope check, not as containment for the launcher or protection for an
off-target CMU. Use the collector only on a spare, physically isolated bench unit and never while
the CMU is connected to a vehicle.
