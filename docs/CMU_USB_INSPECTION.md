# Firmware-gated CMU USB inspection

`mazda-cmu-inspect` prepares a report-only USB payload for a first-generation Mazda Connect CMU.
It is restricted to firmware `74.00.324A` and to the Linux application processor. It is not a
vehicle interface.

The collector does not invoke VIP utilities, open CAN or LIN devices, call vehicle-data APIs,
flash firmware, remount filesystems, install persistence, configure networking, load modules,
change services, or reboot. It reads a fixed list of files under `/jci`, `/proc`, `/sys`, and the
running kernel's USB-network module directory. Its only intentional writes are a new bounded report
directory on the same removable USB filesystem.

This boundary is structural, not merely procedural. There is no arbitrary command, path, or
collector option exposed by the Mac utility. Future VIP, CAN, and LIN functionality is out of
scope.

## What mechanism this uses

The armrest USB ports and a normal MacBook USB port are both USB hosts. Do not connect them with a
passive host-to-host cable.

The phase-one report instead uses the FAT32 update-filename command injection documented by Zero
Day Initiative for `74.00.324A`. A mass-storage drive contains exactly:

- `jci-autoupdate`, which asks the stock scanner to inspect update files;
- one otherwise empty `.up` file whose filename invokes only `cmu-inspect.sh` from one selected
  mount;
- `cmu-inspect.sh`, the fixed report collector.

This is root command execution in the CMU's Linux application processor. The payload is report-only,
but the overall mechanism is neither passive nor zero-write: the stock scanner may log activity,
reads may update access times, and the final `sync` calls may flush unrelated stock writes already
pending. No Mazda update image is included, parsed, or installed.

Primary references:

- [ZDI analysis of the v74.00.324A USB command injection](https://www.zerodayinitiative.com/blog/2024/11/7/multiple-vulnerabilities-in-the-mazda-in-vehicle-infotainment-ivi-system)
- [Mazda Connect USB-media documentation](https://www.mazdausa.com/static/manuals/mazdaconnect-6gb/contents/48020100.html)
- [Published CMU kernel configuration](https://github.com/silverchris/mazda-cmu-documentation/blob/gh-pages/kernel-config.md)

## Provenance and licensing

The launcher in this MIT-licensed repository was independently written from ZDI's published
filename-injection mechanics. It derives `/` with standard shell built-ins and names one explicitly
selected mount. It does not use the `${HOME%root}` expression, mount-search loop, launcher source,
or payload source from TouchTune or another community installer. Those projects remain ecosystem
corroboration in [`REFERENCES.md`](REFERENCES.md), not implementation sources.

## Phase 1: prepare a report drive on macOS

Check **Settings → System → About** on the CMU first. The screen may show `74.00.324`, but the
on-device gate must normalize the internal `JCI_SW_VER` and `JCI_SW_VER_PATCH` fields to exactly
`74.00.324A`. A missing patch on a `74.00.324` base, a `B` patch, or any other version exits before
the report directory is created.

Use Disk Utility to erase a dedicated drive as FAT32 with an MBR partition map. The preparer uses
`diskutil` to require all three facts: FAT32 rather than FAT16, removable media, and
`FDisk_partition_scheme`. It also requires the mounted volume root under `/Volumes`, refuses
existing non-metadata content, creates new files without overwriting, and enumerates the root again
after writing. If an AppleDouble sidecar, unrelated `.up`, or any other unexpected entry appears,
the three files it created are rolled back.

ZDI's documented single-drive target was mounted at `sda1`, so the corresponding preparation is:

```bash
cargo run --locked -p mazda-cmu-inspect -- \
  prepare-usb --firmware 74.00.324A --cmu-mount sda1 /Volumes/MAZDA_CMU
```

Use `sdb1` only when that exact mount has already been established on the spare bench unit. Do not
cycle through guessed mounts or put multiple launcher files on one drive. A wrong selection safely
produces no report; treat that as a stop condition, not a reason to broaden the launcher.

The command creates exactly three payload files. Inspect the root, eject the drive normally, and do
not rename or edit the unusual `.up` filename. Do not use a drive containing `._*`, `.DS_Store`, a
firmware package, an installer, a tweak, or another `.up` file.

## Phase 1: isolated bench run

Before inserting the drive:

1. Use a spare Gen-6.5 CMU, not the daily-driver unit.
2. Physically disconnect vehicle CAN and LIN.
3. Use current-limited, fused bench power as described in [`BENCH_SETUP.md`](BENCH_SETUP.md).
4. Confirm `74.00.324` on the CMU screen again.
5. Disconnect every other USB device and confirm the prepared drive contains only the three payload
   files plus allowed macOS metadata directories.

Insert the drive only after the stock HMI has fully booted. The collector is intentionally silent
and does not kill or open HMI dialogs. Allow scanner activity to finish before turning the bench
supply off normally and removing the drive. Never remove it during activity.

Each fixed-file read and checksum is capped at 256 KiB and five seconds. Production uses the
BusyBox 1.19-compatible `timeout -t SECS -s KILL` form. If the CMU lacks the required `timeout`,
`dd`, or `cksum` applet, the payload fails closed. It does not substitute an unbounded command.
Open-ended commands such as `ps`, `df`, `ifconfig`, and `busybox --list` are not run.

A completed report has:

- `manifest.tsv` using schema 2 and build ID `mazda-cmu-inspect-report-v2`;
- exactly one row for every expected source, in fixed order;
- the byte length, POSIX `cksum`, and exact output filename for each successful capture;
- a final `result<TAB>complete` record;
- `sync-complete`, created between two successful stock `/bin/sync` calls, with no subsequent
  report writes.

The separate completion marker distinguishes a fully flushed run from a manifest that was merely
written. POSIX `cksum` detects accidental truncation or corruption; it is not a cryptographic
authenticity claim.

The report covers CMU and kernel versions; CPU and memory information; mounts and partitions;
loaded modules and kernel configuration when exposed by `/proc`; input, framebuffer, DRM, USB, and
unclassified network-interface inventories; and only the explicitly relevant `usbnet`, `asix`,
`cdc_ether`, and `cdc_ncm` filenames from the exact running-kernel release directory. It does not
enumerate processes, scan a module directory, or execute network tools.

Back on the Mac, validate before opening individual files or making a transport decision:

```bash
cargo run --locked -p mazda-cmu-inspect -- \
  analyze-report /Volumes/MAZDA_CMU/mazda-cmu-report
```

The analyzer is read-only. It rejects a missing or incorrect completion marker, missing or duplicate
rows, unknown statuses, any `timeout` or `io_error`, wrong sizes or checksums, unexpected files,
symlinks, captures over 1 MiB, the wrong build ID, module paths from any kernel other than the
reported running release, and any firmware identity other than `74.00.324A`. Interface names are
reported as unclassified; names alone are never treated as proof that an interface is unrelated to
the vehicle.

Allowed failure statuses are `not_found`, `not_regular_file`, `permission_denied`, `io_error`,
`timeout`, and `dependency_failed`. Missing evidence is never a reason to elevate privileges or
broaden the payload.

Report content is sensitive. Review and redact hostnames, mount paths, and network details before
publishing it.

## Phase 2 remains disabled

No direct Mac transport, USB-Ethernet probe, or remote shell is implemented. A report may show an
ASIX, CDC Ethernet, or CDC NCM module for the running kernel, but that is compatibility evidence
only. Inserting an adapter is not passive: CMU hotplug can load a module, configure an interface, or
write stock logs.

Do not insert networking hardware, load a module, change an interface, start a server, install an
authorized key, or connect the CMU to another network based on this report. Any later transport
must be separately designed and reviewed after a genuine report from the exact bench CMU. A later
shell would require a separate explicit feature with key-only authentication, a dedicated isolated
interface, no persistence, and deterministic teardown.

## Stop conditions

Stop without working around the condition if:

- the firmware cannot be normalized to exactly `74.00.324A`;
- the unit is connected to vehicle CAN or LIN;
- the selected mount was guessed or the drive is not scanned automatically;
- any firmware update or installation screen appears;
- either `result<TAB>complete` or `sync-complete` is missing;
- any capture reports `timeout` or `io_error`;
- the display, camera, audio, power behavior, or stock HMI becomes abnormal;
- continuing would require a firmware package, persistence, remount, reboot, watchdog change, VIP
  command, vehicle-bus access, networking change, or another exploit.
