# Firmware-gated CMU USB inspection

`mazda-cmu-inspect` prepares a report-only USB payload for a first-generation Mazda Connect CMU.
The current implementation is deliberately limited to the `74.00.324` / `74.00.324A` firmware
family. It is an application-processor characterization tool, not a vehicle interface.

The payload never calls VIP tools, opens CAN or LIN interfaces, changes vehicle services, flashes
firmware, remounts filesystems, installs persistence, configures networking, loads modules, or
reboots the CMU. It reads a fixed allowlist and writes a bounded report to the same removable USB
filesystem from which it launched.

## What mechanism this uses

The armrest USB ports are host-side ports. A normal MacBook USB port is also a host, so a passive
host-to-host cable cannot expose the CMU and must not be used.

For firmware `74.00.324A`, the initial report instead uses the update-scanner command-injection path
documented by Zero Day Initiative. A FAT32 mass-storage device contains:

- `jci-autoupdate`, which asks the stock scanner to inspect update files;
- an otherwise empty `.up` file whose filename invokes only `cmu-inspect.sh`;
- `cmu-inspect.sh`, the fixed report collector.

This is root command execution in the CMU's Linux application processor, even though the invoked
payload is report-only. The distinction matters: the collector makes no explicit persistent CMU
write, but the stock update scanner is outside this project's control. Reads can also update access
times on writable filesystems, and stock logging may record scanner activity. Do not describe the
whole mechanism as zero-write or risk-free.

The launcher is derived from the vulnerability mechanics described by ZDI, not from a firmware
update package. No signed or unsigned Mazda update image is included, parsed, or installed.

Primary references:

- [ZDI analysis of the v74.00.324A USB command injection](https://www.zerodayinitiative.com/blog/2024/11/7/multiple-vulnerabilities-in-the-mazda-in-vehicle-infotainment-ivi-system)
- [Mazda Connect USB-media documentation](https://www.mazdausa.com/static/manuals/mazdaconnect-6gb/contents/48020100.html)
- [Published CMU kernel configuration](https://github.com/silverchris/mazda-cmu-documentation/blob/gh-pages/kernel-config.md)
- [TouchTune's independently validated v74 USB workflow](https://github.com/Miatafy/TouchTune)

## Phase 1: prepare a report drive on macOS

First check the CMU version using Mazda Connect's **Settings → System → About** screen. Stop unless
it shows the `74.00.324` build family. The About screen can omit the trailing package `A`; the
internal collector accepts only the exact `74.00.324` or `74.00.324A` base after launch.

Use Disk Utility to prepare a dedicated, blank drive with an MBR partition map and FAT32 filesystem.
Confirm its volume path carefully. The preparer refuses a filesystem root, symlink, missing path,
non-directory, unconfirmed firmware, or destination containing anything other than standard macOS
volume metadata. It creates only new files and never overwrites existing content.

From the repository:

```bash
cargo run --locked -p mazda-cmu-inspect -- \
  prepare-usb --firmware 74.00.324A /Volumes/MAZDA_CMU
```

The command creates exactly three payload files. Inspect the drive in Finder, eject it normally,
and do not rename or edit the unusual `.up` filename. macOS `._*` AppleDouble sidecars or unrelated
`.up` files must not be present.

## Phase 1: isolated bench run

Before inserting the drive:

1. Use a spare Gen-6.5 CMU, not the daily-driver unit.
2. Physically disconnect vehicle CAN and LIN.
3. Use current-limited, fused bench power as described in [`BENCH_SETUP.md`](BENCH_SETUP.md).
4. Confirm `74.00.324` on the CMU screen again.
5. Confirm the USB drive contains no firmware package, installer, tweak, or unrelated `.up` file.

Insert the drive only after the stock HMI has fully booted. The collector is intentionally silent: it
does not kill or open HMI dialogs. Allow the scanner to finish and the drive activity to stop before
turning the bench supply off normally and removing the drive. Never remove it during activity.

A successful run creates `mazda-cmu-report/manifest.tsv` on the drive. The manifest is written first
and ends with `result<TAB>complete` only after every observation was attempted. Treat a missing final
record as an incomplete run; do not retry repeatedly or try a different exploit. After writing the
final record, the production payload calls the stock `/bin/sync` once to flush the USB report. That
also flushes any writes already pending elsewhere in the stock system, another reason not to claim
the overall mechanism is zero-write. Return to source review and bench diagnosis.

The report contains:

- exact CMU and kernel version data;
- CPU, memory, partition, mount, and filesystem summaries;
- loaded modules and the published kernel configuration when exposed by `/proc`;
- input, framebuffer, DRM, USB, and network-interface inventories;
- process names and existing interface configuration.

Every file or command capture is capped at 256 KiB. The manifest records `ok`, `truncated`,
`not_found`, `not_regular_file`, `permission_denied`, `io_error`, or `command_error` for each source.
Missing sources are evidence about that firmware and are never a reason to elevate privileges.

Report content is sensitive. Review and redact hostnames, mount paths, network details, and process
names before publishing it.

## Phase 2: direct Mac report transport

No direct Mac transport or remote shell is enabled yet. The published CMU kernel configuration lists
ASIX AX88xxx, CDC Ethernet, and CDC NCM USB-network drivers as modules. That makes a USB-Ethernet
peripheral a plausible host-safe link from a CMU armrest port to a Mac, without asking either host to
pretend to be a USB device.

The phase-1 report must first prove that the exact CMU has the corresponding modules and reveal its
existing interface names and network tools. Only then should the project add an ephemeral, isolated
link-local address and a one-shot report server. A later shell must be a separate explicit feature,
key-only, bound only to that USB interface, non-persistent, and torn down when the payload or link
exits.

Until those facts are captured, do not load a module, change an interface, start SSH, install an
authorized key, or connect the CMU to another network.

## Stop conditions

Stop without working around the condition if:

- the firmware is not exactly `74.00.324` / `74.00.324A`;
- the unit is connected to vehicle CAN or LIN;
- the drive is not mounted and scanned automatically;
- any firmware update or installation screen appears;
- the report manifest is missing its final `result<TAB>complete` record;
- the display, camera, audio, power behavior, or stock HMI becomes abnormal;
- continuing would require a firmware package, persistence, remount, reboot, watchdog change, VIP
  command, vehicle-bus access, or a second unreviewed exploit.
