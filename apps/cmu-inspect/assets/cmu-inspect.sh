#!/bin/sh
# Report-only collector for one 2019.5 Mazda CX-5 GT on 70.00.100 NA N.
#
# This script writes only a new report directory on the removable USB filesystem. It does not
# remount, reboot, persist, configure networking, change services, load modules, or access VIP,
# CAN, or LIN interfaces. The update scanner that launches it is outside this script's control.

PATH=/bin:/sbin:/usr/bin:/usr/sbin
export PATH
umask 077

REPORT_BUILD_ID=mazda-cmu-inspect-70.00.100-na-report-v1
MAX_BYTES=262144
BLOCK_SIZE=4096
BLOCKS_WITH_SENTINEL=65
BLOCKS_WITHOUT_SENTINEL=64
READ_TIMEOUT_SECONDS=5

if [ "${MAZDA_CMU_INSPECT_TESTING:-0}" = "1" ]; then
    CMU_ROOT=${MAZDA_CMU_INSPECT_TEST_ROOT:-}
    USB_ROOT=${MAZDA_CMU_INSPECT_TEST_USB:-}
    READ_TIMEOUT_SECONDS=${MAZDA_CMU_INSPECT_TEST_TIMEOUT_SECONDS:-5}
    [ -n "$CMU_ROOT" ] && [ -n "$USB_ROOT" ] || exit 64
    TIMEOUT_PROGRAM=$(command -v timeout 2>/dev/null)
    DD_PROGRAM=$(command -v dd 2>/dev/null)
    CKSUM_PROGRAM=$(command -v cksum 2>/dev/null)
    [ -x "$TIMEOUT_PROGRAM" ] && [ -x "$DD_PROGRAM" ] && [ -x "$CKSUM_PROGRAM" ] || exit 68
else
    CMU_ROOT=
    case "$0" in
        /tmp/mnt/sda1/cmu-inspect.sh) USB_ROOT=${0%/*} ;;
        *) exit 64 ;;
    esac
    [ -d /jci ] || exit 65
    BUSYBOX=/bin/busybox
    [ -x "$BUSYBOX" ] || exit 68
fi

VERSION_PATH="${CMU_ROOT}/jci/version.ini"
[ -r "$VERSION_PATH" ] && [ -f "$VERSION_PATH" ] || exit 66

FIRMWARE_VERSION=
FIRMWARE_PATCH=
FIRMWARE_FLAVOR=
SOFTWARE_PART_NUMBER=
SEEN_SW_VER=0
SEEN_SW_VER_PATCH=0
SEEN_SW_FLAVOR=0
SEEN_SW_PART_NUMBER=0
CR=$(printf '\r')
while IFS= read -r version_line; do
    case "$version_line" in
        *"$CR") version_line=${version_line%?} ;;
    esac
    case "$version_line" in
        JCI_SW_VER=*)
            [ "$SEEN_SW_VER" -eq 0 ] || exit 67
            SEEN_SW_VER=1
            value=${version_line#*=}
            case "$value" in
                \"*\") value=${value#\"}; value=${value%\"} ;;
                *) exit 67 ;;
            esac
            FIRMWARE_VERSION=${value##*_}
            ;;
        JCI_SW_VER_PATCH=*)
            [ "$SEEN_SW_VER_PATCH" -eq 0 ] || exit 67
            SEEN_SW_VER_PATCH=1
            value=${version_line#*=}
            case "$value" in
                \"*\") value=${value#\"}; value=${value%\"} ;;
                *) exit 67 ;;
            esac
            FIRMWARE_PATCH=$value
            ;;
        JCI_SW_FLAVOR=*)
            [ "$SEEN_SW_FLAVOR" -eq 0 ] || exit 67
            SEEN_SW_FLAVOR=1
            value=${version_line#*=}
            case "$value" in
                \"*\") value=${value#\"}; value=${value%\"} ;;
                *) exit 67 ;;
            esac
            FIRMWARE_FLAVOR=${value##*_}
            ;;
        JCI_SW_PART_NUMBER=*)
            [ "$SEEN_SW_PART_NUMBER" -eq 0 ] || exit 67
            SEEN_SW_PART_NUMBER=1
            value=${version_line#*=}
            case "$value" in
                \"*\") value=${value#\"}; value=${value%\"} ;;
                *) exit 67 ;;
            esac
            SOFTWARE_PART_NUMBER=$value
            ;;
    esac
done <"$VERSION_PATH"

case "$SEEN_SW_VER:$SEEN_SW_VER_PATCH:$SEEN_SW_FLAVOR:$SEEN_SW_PART_NUMBER:$FIRMWARE_VERSION:$FIRMWARE_PATCH:$FIRMWARE_FLAVOR:$SOFTWARE_PART_NUMBER" in
    1:1:1:1:70.00.100:A:NA:SWI10-24818-807R02|1:1:1:1:70.00.100A:A:NA:SWI10-24818-807R02) ;;
    *) exit 67 ;;
esac

if [ "${MAZDA_CMU_INSPECT_TESTING:-0}" != "1" ]; then
    "$BUSYBOX" timeout -t 1 -s KILL "$BUSYBOX" true >/dev/null 2>&1 || exit 68
    "$BUSYBOX" timeout -t 1 -s KILL "$BUSYBOX" dd if=/dev/null of=/dev/null \
        bs=1 count=1 >/dev/null 2>&1 || exit 68
    "$BUSYBOX" timeout -t 1 -s KILL "$BUSYBOX" cksum "$VERSION_PATH" \
        >/dev/null 2>&1 || exit 68
fi

REPORT="$USB_ROOT/mazda-cmu-report"
mkdir "$REPORT" 2>/dev/null || exit 73
MANIFEST="$REPORT/manifest.tsv"
printf 'mazda-cmu-report\t2\nbuild\t%s\nsource\tstatus\tbytes\tcksum\tfile\n' \
    "$REPORT_BUILD_ID" >"$MANIFEST" 2>/dev/null || exit 74

record_status() {
    printf '%s\t%s\t%s\t%s\t%s\n' "$1" "$2" "$3" "$4" "$5" \
        >>"$MANIFEST" 2>/dev/null || exit 74
}

bounded_copy() {
    copy_source=$1
    copy_destination=$2
    copy_blocks=$3
    if [ "${MAZDA_CMU_INSPECT_TESTING:-0}" = "1" ]; then
        "$TIMEOUT_PROGRAM" "$READ_TIMEOUT_SECONDS" "$DD_PROGRAM" \
            if="$copy_source" of="$copy_destination" bs="$BLOCK_SIZE" \
            count="$copy_blocks" 2>/dev/null
    else
        "$BUSYBOX" timeout -t "$READ_TIMEOUT_SECONDS" -s KILL "$BUSYBOX" dd \
            if="$copy_source" of="$copy_destination" bs="$BLOCK_SIZE" \
            count="$copy_blocks" 2>/dev/null
    fi
    copy_status=$?
    case "$copy_status" in
        124|137|143) return 124 ;;
        *) return "$copy_status" ;;
    esac
}

bounded_checksum() {
    checksum_target=$1
    checksum_result="$REPORT/.checksum-result"
    rm -f "$checksum_result"
    if [ "${MAZDA_CMU_INSPECT_TESTING:-0}" = "1" ]; then
        "$TIMEOUT_PROGRAM" "$READ_TIMEOUT_SECONDS" "$CKSUM_PROGRAM" \
            "$checksum_target" >"$checksum_result" 2>/dev/null
    else
        "$BUSYBOX" timeout -t "$READ_TIMEOUT_SECONDS" -s KILL "$BUSYBOX" cksum \
            "$checksum_target" >"$checksum_result" 2>/dev/null
    fi
    checksum_status=$?
    case "$checksum_status" in
        124|137|143)
            rm -f "$checksum_result"
            return 124
            ;;
    esac
    [ "$checksum_status" -eq 0 ] || {
        rm -f "$checksum_result"
        return "$checksum_status"
    }

    CHECKSUM_VALUE=
    CHECKSUM_BYTES=
    checksum_name=
    IFS=' ' read -r CHECKSUM_VALUE CHECKSUM_BYTES checksum_name <"$checksum_result"
    rm -f "$checksum_result"
    case "$CHECKSUM_VALUE:$CHECKSUM_BYTES" in
        *[!0-9:]*|:*|*:) return 1 ;;
    esac
    return 0
}

finalize_capture() {
    source_name=$1
    output_name=$2
    part_file=$3
    requested_status=$4

    bounded_checksum "$part_file"
    checksum_status=$?
    if [ "$checksum_status" -ne 0 ]; then
        rm -f "$part_file"
        if [ "$checksum_status" -eq 124 ]; then
            record_status "$source_name" timeout 0 - -
        else
            record_status "$source_name" io_error 0 - -
        fi
        return
    fi

    final_file="$REPORT/$output_name"
    final_status=$requested_status
    if [ "$CHECKSUM_BYTES" -gt "$MAX_BYTES" ]; then
        if bounded_copy "$part_file" "$final_file" "$BLOCKS_WITHOUT_SENTINEL"; then
            final_status=truncated
        else
            copy_status=$?
            rm -f "$part_file" "$final_file"
            if [ "$copy_status" -eq 124 ]; then
                record_status "$source_name" timeout 0 - -
            else
                record_status "$source_name" io_error 0 - -
            fi
            return
        fi
        rm -f "$part_file"
    elif ! mv "$part_file" "$final_file" 2>/dev/null; then
        rm -f "$part_file"
        record_status "$source_name" io_error 0 - -
        return
    fi

    bounded_checksum "$final_file"
    checksum_status=$?
    if [ "$checksum_status" -ne 0 ]; then
        rm -f "$final_file"
        if [ "$checksum_status" -eq 124 ]; then
            record_status "$source_name" timeout 0 - -
        else
            record_status "$source_name" io_error 0 - -
        fi
        return
    fi
    record_status "$source_name" "$final_status" "$CHECKSUM_BYTES" \
        "$CHECKSUM_VALUE" "$output_name"
}

capture_file() {
    relative_path=$1
    output_name=$2
    source_path="${CMU_ROOT}/$relative_path"
    part_file="$REPORT/.$output_name.part"

    if [ ! -e "$source_path" ]; then
        record_status "$relative_path" not_found 0 - -
    elif [ ! -f "$source_path" ]; then
        record_status "$relative_path" not_regular_file 0 - -
    elif [ ! -r "$source_path" ]; then
        record_status "$relative_path" permission_denied 0 - -
    elif bounded_copy "$source_path" "$part_file" "$BLOCKS_WITH_SENTINEL"; then
        finalize_capture "$relative_path" "$output_name" "$part_file" ok
    else
        copy_status=$?
        rm -f "$part_file"
        if [ "$copy_status" -eq 124 ]; then
            record_status "$relative_path" timeout 0 - -
        else
            record_status "$relative_path" io_error 0 - -
        fi
    fi
}

capture_network_module_files() {
    source_name=module-files/usb-network
    output_name=usb-network-modules.txt
    part_file="$REPORT/.$output_name.part"
    kernel_release_file="$REPORT/kernel-release.txt"

    if [ ! -r "$kernel_release_file" ]; then
        record_status "$source_name" dependency_failed 0 - -
        return
    fi
    KERNEL_RELEASE=
    IFS= read -r KERNEL_RELEASE <"$kernel_release_file"
    case "$KERNEL_RELEASE" in
        ''|*[!A-Za-z0-9._+-]*)
            record_status "$source_name" dependency_failed 0 - -
            return
            ;;
    esac

    module_directory="${CMU_ROOT}/lib/modules/$KERNEL_RELEASE/kernel/drivers/net/usb"
    if [ ! -d "$module_directory" ]; then
        record_status "$source_name" not_found 0 - -
        return
    fi

    : >"$part_file" 2>/dev/null || {
        record_status "$source_name" io_error 0 - -
        return
    }
    for module_name in usbnet asix cdc_ether cdc_ncm; do
        for module_suffix in .ko .ko.gz .ko.xz; do
            module_path="$module_directory/$module_name$module_suffix"
            [ -f "$module_path" ] || continue
            printf '%s\n' "${module_path#"${CMU_ROOT}"/}" >>"$part_file" 2>/dev/null || {
                rm -f "$part_file"
                record_status "$source_name" io_error 0 - -
                return
            }
        done
    done
    finalize_capture "$source_name" "$output_name" "$part_file" ok
}

capture_file jci/version.ini firmware-version.ini
capture_file proc/sys/kernel/osrelease kernel-release.txt
capture_file proc/version kernel-version.txt
capture_file proc/cmdline kernel-command-line.txt
capture_file proc/cpuinfo cpuinfo.txt
capture_file proc/meminfo meminfo.txt
capture_file proc/mounts mounts.txt
capture_file proc/modules modules.txt
capture_file proc/config.gz kernel-config.gz
capture_file proc/partitions partitions.txt
capture_file proc/mtd mtd.txt
capture_file proc/bus/input/devices input-devices.txt
capture_file proc/bus/usb/devices usb-devices.txt
capture_file sys/class/graphics/fb0/name framebuffer-name.txt
capture_file sys/class/graphics/fb0/modes framebuffer-modes.txt
capture_file sys/class/drm/card0/device/uevent drm-device.txt
capture_network_module_files

printf 'result\tcomplete\n' >>"$MANIFEST" 2>/dev/null || exit 74
SYNC_MARKER_PART="$REPORT/.sync-complete.part"
SYNC_MARKER="$REPORT/sync-complete"
printf '%s\n' "$REPORT_BUILD_ID" >"$SYNC_MARKER_PART" 2>/dev/null || exit 74

if [ "${MAZDA_CMU_INSPECT_TESTING:-0}" != "1" ]; then
    [ -x /bin/sync ] || exit 75
    /bin/sync >/dev/null 2>&1 || exit 75
fi
mv "$SYNC_MARKER_PART" "$SYNC_MARKER" 2>/dev/null || exit 74
if [ "${MAZDA_CMU_INSPECT_TESTING:-0}" != "1" ]; then
    /bin/sync >/dev/null 2>&1 || exit 75
fi
exit 0
