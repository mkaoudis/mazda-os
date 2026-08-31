# Safety boundary

This project is an infotainment experiment, not a vehicle-control project.

## Development ladder

1. Desktop mocks and deterministic scenarios.
2. CPU/ABI emulation where useful.
3. Spare CMU on a bench with vehicle buses physically disconnected.
4. Real CMU read-only observation while parked.
5. Explicitly launched experimental UI while retaining the stock HMI as a fallback.

Do not skip directly to installing untested code in a daily-driver CMU.

## Physical bench boundary

A development CMU should be powered on the bench with **CAN and LIN physically disconnected**. If there is no electrical path to the vehicle network, a software bug cannot accidentally transmit onto it.

## Out of scope / prohibited architecture

The project core must not add support for:

- CAN transmission;
- LIN transmission;
- VIP firmware replacement or flashing;
- direct VIP SPI experimentation on an in-vehicle unit;
- writes to safety-critical ECUs;
- arbitrary vehicle-bus bridging;
- kernel, bootloader, or low-level flash experiments on the daily-driver CMU.

Research tools may document what exists in the hardware, but the application architecture must not depend on these capabilities.

## Real-car testing rules

Early real-car tests should be performed while stationary and should preserve factory ownership of functions such as the reverse camera, audio routing, startup/shutdown, and any safety-related UI.

The first CMU integration should observe state only. Any future mutating infotainment operation must be individually understood, reviewed, and isolated from vehicle-control buses.
