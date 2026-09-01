# Car-specific CMU USB inspection

`mazda-cmu-inspect` prepares a report-only USB payload for one target:

- 2019.5 Mazda CX-5 GT;
- About-screen version `70.00.100 NA N`;
- internal firmware identity `70.00.100A-NA`;
- raw version fields `JCI_SW_VER="MAZ_CMU-150_70.00.100"`,
  `JCI_SW_VER_PATCH="A"`, and `JCI_SW_FLAVOR="NA"`;
- software part number `SWI10-24818-807R02`.

This is not a general Mazda utility. The model and trim are a required owner confirmation; the
collector cannot verify them without querying vehicle-side services, which are intentionally out of
scope. Once launched, it independently requires the complete published raw version, patch, flavor,
and software part fields before creating a report. Prefixes are not discarded or normalized for
the gate.

The final `N` on the screen is the navigation protocol marker. It is not the firmware patch. The
patch is the `A` in Mazda's package identity `cmu150_NA_70.00.100A`. Mazda's North American service
CDN still identifies the matching failsafe package by that name, while the published NA firmware
component table associates `70.00.100A` with software part `SWI10-24818-807R02`.

The collector is restricted to the Linux application processor. It does not invoke VIP utilities,
open CAN or LIN devices, call vehicle-data APIs, flash firmware, remount filesystems, install
persistence, configure networking, load modules, change services, or reboot. It reads a fixed list
of files under `/jci`, `/proc`, `/sys`, and the running kernel's USB-network module directory. Its
only intentional writes are a new bounded report directory on the removable USB drive.

This boundary is structural: the Mac utility exposes no arbitrary command, collector path, mount,
or shell option. Future VIP, CAN, and LIN functionality is out of scope.

## Evidence boundary

The armrest hub and a normal MacBook USB port are both USB hosts. Do not join them with a passive
host-to-host cable. Phase one uses a mass-storage drive prepared by the Mac and read back by the Mac
afterward.

The drive uses the update-filename command injection documented by Zero Day Initiative on
`74.00.324A`. ZDI states that earlier versions down to at least the v70 family may also be affected,
but its published test target was not `70.00.100A`. The stock v70 update pipeline and exact package
identity are established; this particular filename trigger on this particular v70 build remains a
hardware-validation assumption. The first insertion is therefore both the collection attempt and a
bounded compatibility test. Do not describe the v70 entry path as confirmed until the exact car
returns a valid report.

The USB root contains exactly:

- `jci-autoupdate`, which asks the stock scanner to inspect update files;
- one otherwise empty `.up` file whose filename invokes only `cmu-inspect.sh` from the fixed
  first-drive mount `/tmp/mnt/sda1`;
- `cmu-inspect.sh`, the fixed report collector.

This is root command execution in the CMU's application processor. The payload is report-only, but
the overall mechanism is not passive or literally zero-write: the stock scanner may log activity,
reads may update access times, and final `sync` calls may flush unrelated stock writes already
pending. No Mazda firmware image is included, parsed, modified, or installed.

References:

- [ZDI analysis of the v74.00.324A USB command injection](https://www.zerodayinitiative.com/blog/2024/11/7/multiple-vulnerabilities-in-the-mazda-in-vehicle-infotainment-ivi-system)
- [Mazda-hosted NA 70.00.100A failsafe package](https://s3.amazonaws.com/tsd.mazdausa.com/MAZDA_CONNECT/cmu150_NA_70.00.100A_failsafe.up)
- [Published CMU firmware component table](https://github.com/silverchris/mazda-cmu-documentation/blob/gh-pages/versions.md)
- [Published CMU kernel configuration](https://github.com/silverchris/mazda-cmu-documentation/blob/gh-pages/kernel-config.md)

## Provenance and licensing

The launcher in this MIT-licensed repository was independently written from ZDI's published
filename-injection mechanics. It derives `/` with standard shell built-ins and names only `sda1`.
It does not use the `${HOME%root}` expression, mount-search loop, launcher source, or payload source
from TouchTune or another community installer. Those projects remain ecosystem corroboration in
[`REFERENCES.md`](REFERENCES.md), not implementation sources.

## Prepare the report drive on macOS

Confirm the screen still reads exactly `70.00.100 NA N`. A different version, region, or terminal
letter is a stop condition.

`prepare-usb` is deliberately available only on macOS because its safety checks depend on
`diskutil`. On Linux and Windows it returns an explicit unsupported-platform error without writing
anything. `analyze-report` remains cross-platform.

Use Disk Utility to erase a dedicated drive as FAT32 with an MBR partition map. The preparer uses
`diskutil` to require FAT32 rather than FAT16, removable media, and
`FDisk_partition_scheme`. It also requires the mounted volume root under `/Volumes`, refuses
existing non-metadata content, creates new files without overwriting, and reads every payload file
back byte-for-byte. If an AppleDouble sidecar, unrelated `.up`, or any unexpected entry appears,
the three files it created are rolled back.

Run:

```bash
cargo run --locked -p mazda-cmu-inspect -- \
  prepare-usb --target cx5-2019.5-gt-70.00.100-na-n /Volumes/MAZDA_CMU
```

The deliberately long target confirmation binds preparation to the car and the version shown in
the supplied photo. The generated launcher has no selectable mount. ZDI's single-drive example used
`sda1`, so this build uses only `sda1` and fails closed if the car enumerates the drive elsewhere.

Inspect the drive root, eject it normally, and do not rename the unusual `.up` filename. Do not use
a drive containing `._*`, `.DS_Store`, a firmware package, installer, tweak, or another `.up` file.

## First run in the target car

This is not yet validated on the exact hardware. Keep the first attempt controlled:

1. Park outdoors or in a safely ventilated place, apply the parking brake, and do not drive during
   the attempt.
2. Confirm `70.00.100 NA N` on the About screen again.
3. Use accessory mode with a healthy battery. If a proper vehicle-compatible battery maintainer is
   already available and understood, use it according to its instructions.
4. Remove the navigation SD card and disconnect every other USB device. Do not connect diagnostic,
   OBD, serial, CAN, or LIN hardware.
5. After the stock HMI has fully booted, insert only the prepared drive into the armrest smartphone
   USB port used for Android Auto/CarPlay.
6. Leave the vehicle stationary and allow scanner activity to settle. Do not remove the drive while
   it is active. Then shut accessory power down normally, wait for the CMU to turn off, and remove
   the drive.

The installed CMU remains physically connected to the car's normal internal networks. This project
does not claim otherwise. The narrower guarantee is that neither the launcher nor collector names,
opens, queries, or writes VIP, CAN, or LIN interfaces.

If no `mazda-cmu-report` directory appears, the v70 trigger may not work or the fixed mount may not
match. Stop. Do not add a second launcher, cycle through mounts, open the diagnostic update menu, or
substitute a tweak/autorun package.

## Report contents and validation

Each fixed-file read and checksum is capped at 256 KiB and five seconds. Production uses the
BusyBox 1.19-compatible `timeout -t SECS -s KILL` form. If the CMU lacks the required `timeout`,
`dd`, or `cksum` applet, the payload fails closed. It does not substitute an unbounded command.
Open-ended commands such as `ps`, `df`, `ifconfig`, and `busybox --list` are not run.

A completed report has:

- `manifest.tsv` using schema 2 and build ID
  `mazda-cmu-inspect-70.00.100-na-report-v1`;
- exactly one row for every expected source, in fixed order;
- the byte length, POSIX `cksum`, and exact output filename for each successful capture;
- a final `result<TAB>complete` record;
- `sync-complete`, created between two successful stock `/bin/sync` calls, with no subsequent
  report writes.

The separate completion marker distinguishes a fully flushed run from a manifest that was merely
written. POSIX `cksum` detects accidental truncation or corruption; it is not a cryptographic
authenticity claim.

The report covers CMU and kernel versions; CPU and memory information; mounts and partitions;
loaded modules and kernel configuration when exposed by `/proc`; input, framebuffer, DRM, and USB
inventories; and only the explicitly relevant `usbnet`, `asix`, `cdc_ether`, and `cdc_ncm`
filenames from the exact running-kernel release directory. It does not read `/proc/net`, enumerate
processes, scan a module directory, or execute network tools.

Back on the Mac, validate before opening individual files or considering transport work:

```bash
cargo run --locked -p mazda-cmu-inspect -- \
  analyze-report /Volumes/MAZDA_CMU/mazda-cmu-report
```

The analyzer is read-only. It rejects a missing or incorrect completion marker, missing or
duplicate rows, unknown statuses, any timeout or I/O error, wrong sizes or checksums, unexpected
files, symlinks, captures over 1 MiB, the wrong build ID, module paths from another kernel, or any
firmware metadata other than raw version `MAZ_CMU-150_70.00.100`, patch `A`, flavor `NA`, and
software part `SWI10-24818-807R02`.

Allowed nonfatal capture statuses are `not_found`, `not_regular_file`, `permission_denied`, and
`dependency_failed`. Missing evidence is never a reason to elevate privileges or broaden the
payload.

Report content is sensitive. Review and redact hostnames, mount paths, and network details before
publishing it.

## Remote transport remains disabled

No direct Mac transport, USB-Ethernet probe, or remote shell is implemented. A report may show an
ASIX, CDC Ethernet, or CDC NCM module for the running kernel, but that is compatibility evidence
only. Inserting an adapter is not passive: CMU hotplug can load a module, configure an interface,
or write stock logs.

Do not insert networking hardware, load a module, change an interface, start a server, install an
authorized key, or connect the CMU to another network based on this unvalidated build. Any later
transport must be separately designed and reviewed after a genuine report from this exact car. A
later shell would require a separate explicit feature with key-only authentication, a dedicated
isolated interface, no persistence, and deterministic teardown.

## Stop conditions

Stop without working around the condition if:

- the screen is not exactly `70.00.100 NA N`;
- the internal version, patch, NA flavor, or software part gate refuses the run;
- the prepared drive is not scanned automatically or no report appears;
- any firmware update or installation screen appears;
- either `result<TAB>complete` or `sync-complete` is missing;
- any capture reports `timeout` or `io_error`;
- the display, camera, audio, power behavior, or stock HMI becomes abnormal;
- continuing would require a firmware package, alternate mount, persistence, remount, reboot,
  watchdog change, VIP command, vehicle-bus access, networking change, or another exploit.
