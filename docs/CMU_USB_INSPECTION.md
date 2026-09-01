# Read-only CMU inspection over USB media

`mazda-cmu-inspect` collects an allowlisted snapshot of Linux metadata from a first-generation
Mazda Connect CMU. It is intended for a spare, bus-disconnected bench CMU. The utility reads only;
it sends its report to standard output and has no code for mounting filesystems, running commands,
changing privileges, opening vehicle buses, or writing files.

## What the armrest port can and cannot do

The Mazda Connect USB port is the host in its documented USB-media and Android Auto uses. A normal
laptop USB port is also a host. Connecting those two host ports does not expose the CMU as a USB
device and must not be attempted with a passive host-to-host cable.

The utility therefore uses the port only as removable-media transport:

```text
build machine ──copies binary──▶ FAT32 USB storage
                                      │
                                      ▼
isolated CMU ──reads binary from── armrest USB port
       │
       └── report leaves on the existing console's stdout
           or through shell redirection to the same USB storage
```

The stock CMU does not acquire a console or start this binary merely because the drive is inserted.
An already-established, explicitly authorized console is required. On the first bench pass, use the
UART setup in [`BENCH_SETUP.md`](BENCH_SETUP.md). This project does not provide or depend on an
autorun exploit, firmware updater, Android Auto impersonation, or host-to-host USB bridge.

Mazda documents the console USB port as accepting phones and FAT32 USB media. Android's USB
documentation separately describes Android Auto's relevant topology: the accessory/head unit is
the USB host and the Android handset is the device. These are transport facts, not evidence of a
general-purpose CMU management protocol:

- [Mazda Connect USB documentation](https://www.mazdausa.com/static/manuals/mazdaconnect-6gb/contents/48020100.html)
- [Android USB host and accessory overview](https://developer.android.com/develop/connectivity/usb)

## Report contents

The JSON report has schema version `1`. Each observation has a fixed source name, a status, and
either captured content or `null`. The collector reads at most 256 KiB from any individual source.
Missing and permission-protected sources are reported rather than treated as fatal errors.

The fixed file allowlist covers:

- kernel version and boot arguments;
- CPU and memory information;
- mounted filesystems and loaded kernel modules;
- Linux input-device inventory;
- OS release and issue text;
- framebuffer and DRM device metadata.

It also enumerates numeric `/proc` entries and reads only each process's `comm` name. It does not
collect process arguments or environments. No arbitrary path option is exposed by the executable.

## Build

First confirm the processor ABI, dynamic loader, and C library on the exact spare CMU. Do not infer
them from a model year or part number. Then install an appropriate Rust target on the build machine
and build with that confirmed target:

```bash
rustup target add <confirmed-target>
cargo build --release --locked --target <confirmed-target> -p mazda-cmu-inspect
```

The resulting binary is
`target/<confirmed-target>/release/mazda-cmu-inspect`. Copy it to a clean FAT32 USB device using the
build machine. If the CMU's removable-media mount is `noexec`, or the binary reports an incompatible
loader or ABI, stop. Do not remount the filesystem, copy the binary onto CMU storage, or change the
CMU configuration during this characterization pass.

## Bench run

Before inserting the USB device:

1. Use a spare CMU, not the daily-driver unit.
2. Physically disconnect vehicle CAN and LIN.
3. Use current-limited, fused bench power as described in [`BENCH_SETUP.md`](BENCH_SETUP.md).
4. Establish the read-only UART console and identify the existing USB mount path.
5. Confirm the USB filesystem is mounted without changing its mount state or options.

Invoke the binary directly from its existing USB mount. Capturing stdout in the UART terminal is the
strictest read-only workflow:

```bash
<existing-usb-mount>/mazda-cmu-inspect
```

If a file is needed and the shell permits writing the removable device, redirect stdout back to that
same USB filesystem:

```bash
<existing-usb-mount>/mazda-cmu-inspect > <existing-usb-mount>/cmu-report.json
```

The redirection is performed by the shell and writes the removable USB device, not CMU storage. The
collector itself still opens no output files. After the command exits, flush and remove the device
using the stock system's normal procedure.

Treat report content as sensitive: boot arguments, mount paths, hostnames, and process names can
identify the unit or its configuration. Review and redact a report before publishing it.

## Stop conditions

Stop without trying to work around the condition if any of these occurs:

- the target ABI or dynamic loader has not been confirmed;
- the drive is not mounted automatically by the stock CMU;
- execution from removable media is denied;
- running the collector would require root escalation, remounting, an exploit, or a CMU filesystem
  write;
- the CMU is connected to vehicle buses;
- the display, camera, audio, power behavior, or stock HMI becomes abnormal.

A `not_found`, `permission_denied`, `not_regular_file`, or `io_error` observation is expected on some
firmware and is not a reason to raise privileges. Record the result and refine the allowlist later
using an isolated bench fixture.
