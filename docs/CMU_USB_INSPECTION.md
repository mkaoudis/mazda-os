# Car-specific CMU USB inspection

`mazda-cmu-inspect` prepares a **bench-only**, report-only USB payload for one target identity:

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

The gate does not make the entry mechanism safe for an installed or off-target CMU. The stock
update scanner interprets the launcher filename and starts the collector with root privileges
*before* the collector can read or reject `/jci/version.ini`. The human target confirmation and the
internal gate constrain preparation and collection respectively; neither validates nor contains
that prior privileged entry. Do not insert this media into a CMU connected to a vehicle.

The final `N` on the screen is the navigation protocol marker. It is not the firmware patch. The
patch is the `A` in Mazda's package identity `cmu150_NA_70.00.100A`. Mazda's North American service
CDN still identifies the matching failsafe package by that name, while the published NA firmware
component table associates `70.00.100A` with software part `SWI10-24818-807R02`.

The collector is restricted to the Linux application processor. It does not invoke VIP utilities,
open CAN or LIN devices, call vehicle-data APIs, flash firmware, remount filesystems, install
persistence, configure networking, load modules, change services, or reboot. It reads a fixed list
of files under `/jci`, `/proc`, `/sys`, and the running kernel's USB-network module directory. Its
only intentional writes are a fixed launch receipt, a bounded temporary firmware-gate copy, and a
new bounded report directory, all on the removable USB drive. The `cmu-inspect-launch-seen` receipt
is flushed before CMU prerequisites or firmware are inspected. The gate copy is verified absent
before report creation or an unsupported-identity exit.

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
bounded compatibility test, and it belongs only on a spare CMU with vehicle buses physically
disconnected on the bench. Do not describe the v70 entry path as confirmed until an exact-build
bench CMU returns a valid report.

The prepared USB root initially contains exactly:

- `jci-autoupdate`, which asks the stock scanner to inspect update files;
- one otherwise empty `.up` file whose filename invokes only `cmu-inspect.sh` from the fixed
  first-drive mount `/tmp/mnt/sda1`;
- `cmu-inspect.sh`, the fixed report collector.

If the collector starts from the fixed path, it creates `cmu-inspect-launch-seen` beside those three
files using shell built-ins and flushes it before checking `/jci`, BusyBox applets, or firmware. The
exact receipt is:

```text
launch	seen
build	mazda-cmu-inspect-70.00.100-na-report-v3
```

The receipt path is absolute: the launcher runs `/tmp/mnt/sda1/cmu-inspect.sh`, the collector
accepts only that exact `$0`, and it derives `/tmp/mnt/sda1` by removing the final path component.
It does not depend on the scanner's working directory. A valid receipt proves both that the shell
script started and that the fixed `sda1` mount was writable. If the receipt is absent, the launcher
did not reach the script or the drive could not accept/flush the write; those cases cannot be
distinguished from USB evidence alone.

This is root command execution in the CMU's application processor. The payload is report-only, but
the overall mechanism is not passive or literally zero-write: the stock scanner may log activity,
reads may update access times, and the collector's `sync` calls may flush unrelated stock writes
already pending. No Mazda firmware image is included, parsed, modified, or installed.

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
`diskutil` to require FAT32 rather than FAT16, removable media, `FDisk_partition_scheme`, and one
partition only: the selected volume must be the parent disk's `s1`. It also requires the mounted
volume root under `/Volumes`, refuses existing non-metadata content, creates new files without
overwriting, and reads every payload file back byte-for-byte.

The collector and marker are written and verified first. The launcher is then verified under an
inert staging name, and the active `.up` name is created only by the final atomic rename. After any
preparation failure, cleanup removes all payload and matching AppleDouble names and re-lists the
volume. The ordinary preparation error is returned only if that cleanup is verified. Otherwise the
tool emits: **“Media may contain an active launcher; do not insert it into the vehicle. Reformat the
entire device.”** Treat that warning literally; do not insert the suspect drive into any CMU.

Recent macOS releases may attach `com.apple.provenance` to newly created files and represent it on
FAT media as `._*` AppleDouble sidecars. The preparer clears extended attributes from each fixed
payload immediately after creation and removes only its corresponding generated sidecar, then
retains its exact-byte and unexpected-entry checks. Pre-existing sidecars are rejected before any
write, and any sidecar or other unexpected entry that remains is still rejected afterward.

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

## First run on an isolated bench CMU

This is not authorized for an installed CMU. Follow [`BENCH_SETUP.md`](BENCH_SETUP.md), verify the
pinout for the exact spare unit, and keep the first attempt controlled:

1. Use a spare CMU and display physically outside the vehicle. Connect only fused, current-limited
   bench power, ACC, ground, the display, and an already verified passive UART monitor if needed.
2. Leave every vehicle, diagnostic, OBD, CAN, LIN, Ethernet, and USB-network connection physically
   absent. The bench setup must have no electrical connection to a vehicle.
3. Confirm the spare unit's part identity and `70.00.100 NA N` display version before inserting the
   drive. Do not rely on the post-entry firmware gate to protect a different unit.
4. Remove the navigation SD card and every other USB device. After the stock HMI has fully booted,
   insert only the prepared mass-storage drive into the normal smartphone USB port.
5. Watch supply current, display output, and UART output without sending commands. On abnormal
   current or behavior, cut bench power and stop.
6. Allow scanner activity to settle, shut bench power down normally, wait for the CMU to turn off,
   and only then remove the drive.

The narrower software guarantee remains that neither the launcher nor collector names, opens,
queries, or writes VIP, CAN, or LIN interfaces. Physical bench isolation supplies the containment
that the post-entry firmware gate cannot.

If no `mazda-cmu-report` directory appears, inspect `cmu-inspect-launch-seen`. A valid receipt means
the trigger and fixed mount worked but the collector stopped on a prerequisite, firmware gate, or
early report-creation failure. No receipt means the v70 trigger may not work, the fixed mount may
not match, or the USB was not writable. Stop. Do not add a second launcher, cycle through mounts,
open the diagnostic update menu, or substitute a tweak/autorun package.

## Report contents and validation

Each fixed-file read and checksum is capped at 256 KiB and five seconds. Production uses the
BusyBox 1.19-compatible `timeout -t SECS -s KILL` form. If the CMU lacks the required `timeout`,
`dd`, or `cksum` applet, the payload fails closed. It does not substitute an unbounded command.
Open-ended commands such as `ps`, `df`, `ifconfig`, and `busybox --list` are not run.

The applets and timeout form are validated before firmware data is read. The firmware gate copies
at most 256 KiB of `/jci/version.ini`, under the same five-second timeout, to a temporary file on
the USB drive and parses only that copy. The temporary gate file is deleted before either an
unsupported-identity exit or report creation.

A completed report has:

- `manifest.tsv` using schema 3 and build ID
  `mazda-cmu-inspect-70.00.100-na-report-v3`;
- exactly one row for every expected source, in fixed order;
- the byte length, POSIX `cksum`, and exact output filename for each successful capture;
- a final `integrity<TAB>complete` record, which closes the manifest but does not claim successful
  flushing or process completion;
- `flush-complete`, containing an explicit flush receipt and matching build ID.

The launch receipt has its own early `/bin/sync`. Later, the collector writes the manifest integrity
record and a hidden candidate report receipt, then performs two more stock `/bin/sync` calls while
that report receipt is still non-acceptable. Only after both report flushes return success does it
atomically rename the candidate to `flush-complete`; that rename is the last report mutation. If
either report flush fails or execution stops before the rename, no acceptable marker exists and the
hidden candidate may remain. The analyzer requires both the integrity record and the flush receipt,
so it rejects that state without relying on cleanup.

`flush-complete` certifies that both pre-publication flush calls returned success; it does not and
cannot prove the collector process subsequently returned status 0. Because the receipt is published
after the flushes and is not followed by another collector write or flush, interruption can lose the
receipt and cause a safe false negative. Follow the normal bench shutdown procedure before removing
the drive. POSIX `cksum` detects accidental truncation or corruption; it is not a cryptographic
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
software part `SWI10-24818-807R02`. Successful textual evidence used for USB-network analysis must
also be valid UTF-8; malformed captures are rejected rather than treated as empty.

Allowed nonfatal capture statuses are `not_found`, `not_regular_file`, `permission_denied`, and
`dependency_failed`. The analysis retains these statuses and prints `none found` only when every
source needed for that conclusion was captured completely. Otherwise it prints `observation
unavailable: <status>`. Missing evidence is never a reason to elevate privileges or broaden the
payload.

Report content is sensitive. Review and redact hostnames, mount paths, and network details before
publishing it.

## CI and hardware-validation boundary

Linux CI downloads the official static BusyBox 1.19.0 x86-64 binary by a pinned URL and SHA-256,
then runs the collector in a minimal chroot at its production path. That fixture exercises the
production `/bin/busybox` applet calls, the early launch-receipt sync and both report `/bin/sync`
calls, the exact launcher expansion under BusyBox `ash`, and a command that is genuinely killed by
`timeout -t 1 -s KILL`. Host-shell tests remain separate.

Host-shell CI also injects a failure on the second flush and verifies that the collector returns 75,
never publishes `flush-complete`, and leaves a report the analyzer rejects even though the hidden
candidate remains.

The v70 update-scanner trigger itself is explicitly **hardware-unvalidated**. CI does not emulate
the stock Mazda scanner, FAT filename handling, USB enumeration, or root execution on
`70.00.100 NA N`; only a report returned by an exact-build, physically isolated bench CMU can
validate that boundary. A bench result does not authorize repeating the exploit on an installed
vehicle.

## Remote transport remains disabled

No direct Mac transport, USB-Ethernet probe, or remote shell is implemented. A report may show an
ASIX, CDC Ethernet, or CDC NCM module for the running kernel, but that is compatibility evidence
only. Inserting an adapter is not passive: CMU hotplug can load a module, configure an interface,
or write stock logs.

Do not insert networking hardware, load a module, change an interface, start a server, install an
authorized key, or connect the CMU to another network based on this unvalidated build. Any later
transport must be separately designed and reviewed after a genuine report from an exact-build
bench CMU. Its first probe must also remain on the isolated bench. A later shell would require a
separate explicit feature with key-only authentication, a dedicated isolated interface, no
persistence, and deterministic teardown.

## Stop conditions

Stop without working around the condition if:

- the screen is not exactly `70.00.100 NA N`;
- the internal version, patch, NA flavor, or software part gate refuses the run;
- the prepared drive is not scanned automatically or no report appears;
- any firmware update or installation screen appears;
- either `integrity<TAB>complete` or `flush-complete` is missing;
- any capture reports `timeout` or `io_error`;
- the display, camera, audio, power behavior, or stock HMI becomes abnormal;
- continuing would require a firmware package, alternate mount, persistence, remount, reboot,
  watchdog change, VIP command, vehicle-bus access, networking change, another exploit, or an
  installed-vehicle attempt.
