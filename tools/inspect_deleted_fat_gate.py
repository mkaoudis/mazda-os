#!/usr/bin/env python3
"""Read-only FAT32 search for a deleted CMU firmware-gate temp file.

The CMU collector creates `.cmu-version-gate.<pid>` in the USB root when it
reaches the firmware gate, then deletes it before either rejecting the firmware
or creating the report. FAT deletion normally leaves directory-entry metadata
behind until another file reuses that slot.

This tool opens a FAT32 partition or image with O_RDONLY, follows the FAT32 root
directory cluster chain, reconstructs deleted VFAT long filenames, and reports
only metadata. It never writes, repairs, mounts, or unmounts the source.

A recovered exact name is positive evidence that the collector reached creation
of the gate copy. No recovered name is inconclusive because deleted entries may
already have been reused or cleared.
"""

from __future__ import annotations

import argparse
import errno
import io
import os
import re
import struct
import subprocess
import sys
from dataclasses import dataclass
from datetime import datetime
from typing import BinaryIO, Iterable, Optional, Sequence


TARGET_NAME = re.compile(r"^\.cmu-version-gate\.[0-9]+$", re.IGNORECASE)
PARTIAL_MARKERS = (".cmu-version", "version-gate")
LFN_ATTRIBUTE = 0x0F
DELETED_MARKER = 0xE5
FAT32_END_OF_CHAIN = 0x0FFFFFF8
RAW_IO_FALLBACK_ALIGNMENT = 4096


class FatError(Exception):
    """An invalid or unreadable FAT32 source."""


@dataclass(frozen=True)
class Fat32Layout:
    bytes_per_sector: int
    sectors_per_cluster: int
    reserved_sectors: int
    fat_count: int
    fat_sectors: int
    active_fat: int
    total_sectors: int
    cluster_count: int
    root_cluster: int

    @property
    def cluster_size(self) -> int:
        return self.bytes_per_sector * self.sectors_per_cluster

    @property
    def fat_offset(self) -> int:
        sectors = self.reserved_sectors + self.active_fat * self.fat_sectors
        return sectors * self.bytes_per_sector

    @property
    def data_offset(self) -> int:
        sectors = self.reserved_sectors + self.fat_count * self.fat_sectors
        return sectors * self.bytes_per_sector

    @property
    def maximum_cluster(self) -> int:
        return self.cluster_count + 1

    def cluster_offset(self, cluster: int) -> int:
        return self.data_offset + (cluster - 2) * self.cluster_size


@dataclass(frozen=True)
class LongNamePart:
    offset: int
    checksum: int
    code_units: tuple[int, ...]


@dataclass(frozen=True)
class DeletedEntry:
    long_name: Optional[str]
    short_alias: str
    short_entry_offset: int
    long_entry_offsets: tuple[int, ...]
    start_cluster: int
    size: int
    created: str
    modified: str


@dataclass(frozen=True)
class OrphanLongName:
    long_name: str
    offsets: tuple[int, ...]


def read_exact(stream: BinaryIO, offset: int, size: int, description: str) -> bytes:
    try:
        stream.seek(offset)
        data = stream.read(size)
    except OSError as error:
        if error.errno != errno.EINVAL:
            raise FatError(
                f"could not read {description} at 0x{offset:x}: {error}"
            ) from error

        # macOS raw character devices reject reads whose offset or size is not
        # aligned to the device's I/O block. Read a surrounding 4 KiB window
        # and return only the requested bytes. O_RDONLY is preserved.
        alignment = RAW_IO_FALLBACK_ALIGNMENT
        aligned_offset = offset - (offset % alignment)
        requested_end = offset + size
        aligned_end = ((requested_end + alignment - 1) // alignment) * alignment
        aligned_size = aligned_end - aligned_offset
        try:
            stream.seek(aligned_offset)
            aligned_data = stream.read(aligned_size)
        except OSError as aligned_error:
            raise FatError(
                f"could not read aligned window for {description} at "
                f"0x{aligned_offset:x}: {aligned_error}"
            ) from aligned_error
        if len(aligned_data) != aligned_size:
            raise FatError(
                f"short aligned read for {description} at 0x{aligned_offset:x}: "
                f"wanted {aligned_size} bytes, got {len(aligned_data)}"
            )
        relative_offset = offset - aligned_offset
        data = aligned_data[relative_offset : relative_offset + size]
    if len(data) != size:
        raise FatError(
            f"short read for {description} at 0x{offset:x}: "
            f"wanted {size} bytes, got {len(data)}"
        )
    return data


def parse_fat32_layout(stream: BinaryIO) -> Fat32Layout:
    boot = read_exact(stream, 0, 512, "FAT boot sector")
    if boot[510:512] != b"\x55\xaa":
        raise FatError("missing 0x55AA boot-sector signature")

    bytes_per_sector = struct.unpack_from("<H", boot, 11)[0]
    sectors_per_cluster = boot[13]
    reserved_sectors = struct.unpack_from("<H", boot, 14)[0]
    fat_count = boot[16]
    root_entry_count = struct.unpack_from("<H", boot, 17)[0]
    total_sectors_16 = struct.unpack_from("<H", boot, 19)[0]
    fat_sectors_16 = struct.unpack_from("<H", boot, 22)[0]
    total_sectors_32 = struct.unpack_from("<I", boot, 32)[0]
    fat_sectors = struct.unpack_from("<I", boot, 36)[0]
    extended_flags = struct.unpack_from("<H", boot, 40)[0]
    root_cluster = struct.unpack_from("<I", boot, 44)[0]

    if bytes_per_sector not in (512, 1024, 2048, 4096):
        raise FatError(f"invalid bytes-per-sector value: {bytes_per_sector}")
    if (
        sectors_per_cluster == 0
        or sectors_per_cluster > 128
        or sectors_per_cluster & (sectors_per_cluster - 1)
    ):
        raise FatError(f"invalid sectors-per-cluster value: {sectors_per_cluster}")
    if reserved_sectors == 0 or fat_count == 0:
        raise FatError("invalid reserved-sector or FAT count")
    if root_entry_count != 0 or fat_sectors_16 != 0 or fat_sectors == 0:
        raise FatError("source does not have a FAT32 BIOS parameter block")

    total_sectors = total_sectors_16 or total_sectors_32
    non_data_sectors = reserved_sectors + fat_count * fat_sectors
    if total_sectors <= non_data_sectors:
        raise FatError("FAT32 data region is empty or outside the volume")
    cluster_count = (total_sectors - non_data_sectors) // sectors_per_cluster
    if cluster_count < 65525:
        raise FatError(
            f"volume has {cluster_count} data clusters and is not FAT32 by cluster count"
        )
    if root_cluster < 2 or root_cluster > cluster_count + 1:
        raise FatError(f"root cluster {root_cluster} is outside the FAT32 data region")

    active_fat = 0
    if extended_flags & 0x0080:
        active_fat = extended_flags & 0x000F
        if active_fat >= fat_count:
            raise FatError(f"active FAT index {active_fat} exceeds FAT count {fat_count}")

    fat_capacity = fat_sectors * bytes_per_sector // 4
    if fat_capacity <= cluster_count + 1:
        raise FatError("FAT is too small for the declared data-cluster count")

    return Fat32Layout(
        bytes_per_sector=bytes_per_sector,
        sectors_per_cluster=sectors_per_cluster,
        reserved_sectors=reserved_sectors,
        fat_count=fat_count,
        fat_sectors=fat_sectors,
        active_fat=active_fat,
        total_sectors=total_sectors,
        cluster_count=cluster_count,
        root_cluster=root_cluster,
    )


def root_directory_clusters(
    stream: BinaryIO, layout: Fat32Layout
) -> Iterable[tuple[int, int]]:
    cluster = layout.root_cluster
    visited: set[int] = set()

    while True:
        if cluster < 2 or cluster > layout.maximum_cluster:
            raise FatError(f"root-directory chain points outside the volume: {cluster}")
        if cluster in visited:
            raise FatError(f"loop in root-directory cluster chain at cluster {cluster}")
        visited.add(cluster)
        yield cluster, layout.cluster_offset(cluster)

        fat_entry = read_exact(
            stream,
            layout.fat_offset + cluster * 4,
            4,
            f"FAT entry for cluster {cluster}",
        )
        next_cluster = struct.unpack("<I", fat_entry)[0] & 0x0FFFFFFF
        if next_cluster >= FAT32_END_OF_CHAIN:
            return
        if next_cluster == 0x0FFFFFF7:
            raise FatError(f"bad cluster marker in root-directory chain at {cluster}")
        if next_cluster < 2:
            raise FatError(
                f"unexpected free/reserved cluster {next_cluster} in root-directory chain"
            )
        cluster = next_cluster


def decode_lfn_part(entry: bytes, offset: int) -> LongNamePart:
    units: list[int] = []
    for start, length in ((1, 10), (14, 12), (28, 4)):
        chunk = entry[start : start + length]
        units.extend(struct.unpack(f"<{length // 2}H", chunk))
    return LongNamePart(offset=offset, checksum=entry[13], code_units=tuple(units))


def reconstruct_long_name(parts_in_disk_order: Sequence[LongNamePart]) -> str:
    units: list[int] = []
    for part in reversed(parts_in_disk_order):
        units.extend(part.code_units)

    trimmed: list[int] = []
    for unit in units:
        if unit == 0x0000:
            break
        if unit != 0xFFFF:
            trimmed.append(unit)
    encoded = b"".join(struct.pack("<H", unit) for unit in trimmed)
    return encoded.decode("utf-16le", errors="replace")


def decode_short_alias(entry: bytes) -> str:
    raw = bytearray(entry[:11])
    raw[0] = ord("?")
    base = bytes(raw[:8]).decode("cp437", errors="replace").rstrip(" ")
    extension = bytes(raw[8:11]).decode("cp437", errors="replace").rstrip(" ")
    return f"{base}.{extension}" if extension else base


def decode_fat_datetime(date_value: int, time_value: int) -> str:
    if date_value == 0:
        return "unset"
    year = 1980 + ((date_value >> 9) & 0x7F)
    month = (date_value >> 5) & 0x0F
    day = date_value & 0x1F
    hour = (time_value >> 11) & 0x1F
    minute = (time_value >> 5) & 0x3F
    second = (time_value & 0x1F) * 2
    try:
        return datetime(year, month, day, hour, minute, second).isoformat(sep=" ")
    except ValueError:
        return f"invalid(date=0x{date_value:04x}, time=0x{time_value:04x})"


def deleted_entry_from_bytes(
    entry: bytes, offset: int, pending: Sequence[LongNamePart]
) -> DeletedEntry:
    cluster_high = struct.unpack_from("<H", entry, 20)[0]
    cluster_low = struct.unpack_from("<H", entry, 26)[0]
    created_time = struct.unpack_from("<H", entry, 14)[0]
    created_date = struct.unpack_from("<H", entry, 16)[0]
    modified_time = struct.unpack_from("<H", entry, 22)[0]
    modified_date = struct.unpack_from("<H", entry, 24)[0]
    return DeletedEntry(
        long_name=reconstruct_long_name(pending) if pending else None,
        short_alias=decode_short_alias(entry),
        short_entry_offset=offset,
        long_entry_offsets=tuple(part.offset for part in pending),
        start_cluster=(cluster_high << 16) | cluster_low,
        size=struct.unpack_from("<I", entry, 28)[0],
        created=decode_fat_datetime(created_date, created_time),
        modified=decode_fat_datetime(modified_date, modified_time),
    )


def scan_deleted_root_entries(
    stream: BinaryIO, layout: Fat32Layout
) -> tuple[list[DeletedEntry], list[OrphanLongName], int, int]:
    deleted: list[DeletedEntry] = []
    orphaned: list[OrphanLongName] = []
    pending: list[LongNamePart] = []
    cluster_total = 0
    slot_total = 0

    def save_orphan() -> None:
        nonlocal pending
        if pending:
            orphaned.append(
                OrphanLongName(
                    long_name=reconstruct_long_name(pending),
                    offsets=tuple(part.offset for part in pending),
                )
            )
            pending = []

    for _cluster, cluster_offset in root_directory_clusters(stream, layout):
        cluster_total += 1
        cluster_data = read_exact(
            stream, cluster_offset, layout.cluster_size, "root-directory cluster"
        )
        for relative_offset in range(0, len(cluster_data), 32):
            slot_total += 1
            entry = cluster_data[relative_offset : relative_offset + 32]
            entry_offset = cluster_offset + relative_offset
            first_byte = entry[0]
            attribute = entry[11]

            if attribute == LFN_ATTRIBUTE:
                if first_byte == DELETED_MARKER and entry[12] == 0:
                    part = decode_lfn_part(entry, entry_offset)
                    if pending and pending[-1].checksum != part.checksum:
                        save_orphan()
                    pending.append(part)
                else:
                    save_orphan()
                continue

            if first_byte == DELETED_MARKER:
                deleted.append(deleted_entry_from_bytes(entry, entry_offset, pending))
                pending = []
            else:
                save_orphan()

    save_orphan()
    return deleted, orphaned, cluster_total, slot_total


def mounted_at(source: str) -> Optional[str]:
    if not source.startswith("/dev/"):
        return None
    name = os.path.basename(source)
    if name.startswith("rdisk"):
        name = name[1:]
    block_device = f"/dev/{name}"
    try:
        result = subprocess.run(
            ["/sbin/mount"], check=True, capture_output=True, text=True
        )
    except (OSError, subprocess.CalledProcessError):
        return None
    prefix = f"{block_device} on "
    for line in result.stdout.splitlines():
        if line.startswith(prefix):
            return line[len(prefix) :].split(" (", 1)[0]
    return None


def offsets_text(offsets: Sequence[int]) -> str:
    return ", ".join(f"0x{offset:x}" for offset in offsets) or "none"


def print_deleted_entry(entry: DeletedEntry) -> None:
    print(f"  long name:       {entry.long_name or '(no recoverable LFN)'}")
    print(f"  short alias:     {entry.short_alias}")
    print(f"  LFN offsets:     {offsets_text(entry.long_entry_offsets)}")
    print(f"  short offset:    0x{entry.short_entry_offset:x}")
    print(f"  start cluster:   {entry.start_cluster}")
    print(f"  recorded size:   {entry.size} bytes")
    print(f"  created:         {entry.created}")
    print(f"  modified:        {entry.modified}")


def target_like(name: Optional[str]) -> bool:
    if not name:
        return False
    lowered = name.lower()
    return any(marker in lowered for marker in PARTIAL_MARKERS)


def inspect(source: str, allow_mounted: bool, show_all_deleted: bool) -> int:
    mount_point = mounted_at(source)
    if mount_point and not allow_mounted:
        block_name = os.path.basename(source)
        if block_name.startswith("r"):
            block_name = block_name[1:]
        raise FatError(
            f"{source} is mounted at {mount_point}. Unmount it first for a stable "
            f"read-only snapshot:\n  diskutil unmount /dev/{block_name}\n"
            "Use --allow-mounted only if you accept that background writes may reuse evidence."
        )

    try:
        descriptor = os.open(source, os.O_RDONLY)
    except PermissionError as error:
        raise FatError(
            f"permission denied opening {source} read-only; rerun with sudo"
        ) from error
    except OSError as error:
        raise FatError(f"could not open {source} read-only: {error}") from error

    with os.fdopen(descriptor, "rb", buffering=0, closefd=True) as stream:
        layout = parse_fat32_layout(stream)
        deleted, orphaned, cluster_total, slot_total = scan_deleted_root_entries(
            stream, layout
        )

    print(f"Opened read-only: {source}")
    print(
        "FAT32 layout: "
        f"{layout.bytes_per_sector}-byte sectors, "
        f"{layout.sectors_per_cluster} sectors/cluster, "
        f"root cluster {layout.root_cluster}, "
        f"active FAT {layout.active_fat + 1}/{layout.fat_count}"
    )
    print(
        f"Scanned {cluster_total} root-directory cluster(s), "
        f"{slot_total} directory slots, {len(deleted)} deleted short entries."
    )

    exact_entries = [
        entry
        for entry in deleted
        if entry.long_name and TARGET_NAME.fullmatch(entry.long_name)
    ]
    exact_orphans = [
        entry for entry in orphaned if TARGET_NAME.fullmatch(entry.long_name)
    ]
    if exact_entries or exact_orphans:
        print("\nCONFIRMED POSITIVE EVIDENCE")
        print(
            "A deleted `.cmu-version-gate.<pid>` VFAT name survived. The collector "
            "was launched far enough to create its firmware-gate copy."
        )
        for entry in exact_entries:
            print("\nDeleted gate-copy entry:")
            print_deleted_entry(entry)
        for entry in exact_orphans:
            print("\nDeleted gate-copy LFN sequence; its short entry was reused or cleared:")
            print(f"  long name:       {entry.long_name}")
            print(f"  LFN offsets:     {offsets_text(entry.offsets)}")
        print(
            "\nThis proves gate-copy creation, not that the firmware matched. With no "
            "report directory, rejection at the gate is likely, but a failure immediately "
            "after a successful gate remains possible."
        )
    else:
        partial_entries = [entry for entry in deleted if target_like(entry.long_name)]
        partial_orphans = [entry for entry in orphaned if target_like(entry.long_name)]
        if partial_entries or partial_orphans:
            print("\nPARTIAL, NON-CONCLUSIVE EVIDENCE")
            print(
                "Deleted LFN fragments resemble the gate filename, but the complete exact "
                "name could not be reconstructed."
            )
            for entry in partial_entries:
                print("\nCandidate deleted entry:")
                print_deleted_entry(entry)
            for entry in partial_orphans:
                print("\nCandidate orphaned LFN sequence:")
                print(f"  reconstructed:   {entry.long_name}")
                print(f"  offsets:         {offsets_text(entry.offsets)}")
        else:
            print("\nNO RECOVERABLE GATE ENTRY FOUND — RESULT IS INCONCLUSIVE")
            print(
                "The file may never have been created, or its deleted directory slots may "
                "already have been reused or cleared. Absence cannot prove non-execution."
            )

    if show_all_deleted:
        print("\nAll deleted root-directory entries:")
        if not deleted:
            print("  (none)")
        for index, entry in enumerate(deleted, start=1):
            print(f"\nDeleted entry {index}:")
            print_deleted_entry(entry)

    return 0


def make_deleted_lfn_part(text: str, checksum: int) -> bytes:
    units = [ord(character) for character in text]
    if len(units) < 13:
        units.append(0)
    units.extend([0xFFFF] * (13 - len(units)))
    entry = bytearray(32)
    entry[0] = DELETED_MARKER
    entry[11] = LFN_ATTRIBUTE
    entry[12] = 0
    entry[13] = checksum
    entry[26:28] = b"\x00\x00"
    cursor = 0
    for start, count in ((1, 5), (14, 6), (28, 2)):
        for index in range(count):
            struct.pack_into("<H", entry, start + index * 2, units[cursor])
            cursor += 1
    return bytes(entry)


def self_test() -> int:
    bytes_per_sector = 512
    reserved_sectors = 32
    fat_sectors = 600
    total_sectors = 70000
    data_start_sector = reserved_sectors + fat_sectors
    image = bytearray((data_start_sector + 8) * bytes_per_sector)

    struct.pack_into("<H", image, 11, bytes_per_sector)
    image[13] = 1
    struct.pack_into("<H", image, 14, reserved_sectors)
    image[16] = 1
    struct.pack_into("<H", image, 17, 0)
    struct.pack_into("<H", image, 19, 0)
    struct.pack_into("<H", image, 22, 0)
    struct.pack_into("<I", image, 32, total_sectors)
    struct.pack_into("<I", image, 36, fat_sectors)
    struct.pack_into("<H", image, 40, 0)
    struct.pack_into("<I", image, 44, 2)
    image[82:90] = b"FAT32   "
    image[510:512] = b"\x55\xaa"

    fat_offset = reserved_sectors * bytes_per_sector
    for cluster, value in ((0, 0x0FFFFFF8), (1, 0x0FFFFFFF), (2, 0x0FFFFFFF)):
        struct.pack_into("<I", image, fat_offset + cluster * 4, value)

    expected_name = ".cmu-version-gate.4242"
    logical_parts = [
        expected_name[index : index + 13]
        for index in range(0, len(expected_name), 13)
    ]
    directory_entries = [
        make_deleted_lfn_part(part, 0x5A) for part in reversed(logical_parts)
    ]
    short_entry = bytearray(32)
    short_entry[:11] = b"CMUVER~1TMP"
    short_entry[0] = DELETED_MARKER
    short_entry[11] = 0x20
    struct.pack_into("<H", short_entry, 26, 3)
    struct.pack_into("<I", short_entry, 28, 1234)
    directory_entries.append(bytes(short_entry))

    root_offset = data_start_sector * bytes_per_sector
    for index, entry in enumerate(directory_entries):
        start = root_offset + index * 32
        image[start : start + 32] = entry

    class AlignmentEnforcingBytesIO(io.BytesIO):
        def read(self, size: int = -1) -> bytes:
            if (
                size < 0
                or self.tell() % RAW_IO_FALLBACK_ALIGNMENT != 0
                or size % RAW_IO_FALLBACK_ALIGNMENT != 0
            ):
                raise OSError(errno.EINVAL, "simulated raw-device alignment requirement")
            return super().read(size)

    stream = AlignmentEnforcingBytesIO(image)
    layout = parse_fat32_layout(stream)
    deleted, _orphaned, _clusters, _slots = scan_deleted_root_entries(stream, layout)
    if len(deleted) != 1 or deleted[0].long_name != expected_name:
        raise FatError(f"self-test failed: reconstructed entries were {deleted!r}")
    print("Self-test passed: deleted VFAT long filename reconstructed correctly.")
    return 0


def parse_arguments(argv: Sequence[str]) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description=(
            "Read-only search of a FAT32 root directory for a deleted "
            ".cmu-version-gate.<pid> entry."
        )
    )
    parser.add_argument(
        "source",
        nargs="?",
        help="FAT32 partition device (for example /dev/rdisk4s1) or image file",
    )
    parser.add_argument(
        "--allow-mounted",
        action="store_true",
        help="scan a mounted device despite the risk that background writes reuse evidence",
    )
    parser.add_argument(
        "--show-all-deleted",
        action="store_true",
        help="print every deleted root-directory entry, not just gate-related evidence",
    )
    parser.add_argument(
        "--self-test",
        action="store_true",
        help="run the parser against an in-memory FAT32 fixture and exit",
    )
    arguments = parser.parse_args(argv)
    if not arguments.self_test and not arguments.source:
        parser.error("source is required unless --self-test is used")
    return arguments


def main(argv: Sequence[str]) -> int:
    arguments = parse_arguments(argv)
    try:
        if arguments.self_test:
            return self_test()
        return inspect(
            arguments.source,
            allow_mounted=arguments.allow_mounted,
            show_all_deleted=arguments.show_all_deleted,
        )
    except FatError as error:
        print(f"error: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
