#!/bin/sh
# Report-only Mazda Connect Gen-6.5 collector for firmware 74.00.324/A.
#
# This script writes only a new report directory on the removable USB filesystem. It does not
# remount, reboot, persist, configure networking, change services, load modules, or access VIP,
# CAN, or LIN interfaces. The update scanner that launches it is outside this script's control.

PATH=/bin:/sbin:/usr/bin:/usr/sbin
export PATH
umask 077

MAX_BYTES=262144
BLOCK_SIZE=4096
BLOCKS_WITH_SENTINEL=65
BLOCKS_WITHOUT_SENTINEL=64

if [ "${MAZDA_CMU_INSPECT_TESTING:-0}" = "1" ]; then
    CMU_ROOT=${MAZDA_CMU_INSPECT_TEST_ROOT:-}
    USB_ROOT=${MAZDA_CMU_INSPECT_TEST_USB:-}
    [ -n "$CMU_ROOT" ] && [ -n "$USB_ROOT" ] || exit 64
else
    CMU_ROOT=
    case "$0" in
        /tmp/mnt/sd[a-d]1/cmu-inspect.sh) USB_ROOT=${0%/*} ;;
        *) exit 64 ;;
    esac
    [ -d /jci ] || exit 65
fi

VERSION_PATH="${CMU_ROOT}/jci/version.ini"
[ -r "$VERSION_PATH" ] || exit 66
FIRMWARE_LINE=$(grep '^JCI_SW_VER=' "$VERSION_PATH" 2>/dev/null | head -n 1 | tr -d '\r')
FIRMWARE_VALUE=${FIRMWARE_LINE#*=}
FIRMWARE_VALUE=${FIRMWARE_VALUE#\"}
FIRMWARE_VALUE=${FIRMWARE_VALUE%\"}
FIRMWARE_BASE=${FIRMWARE_VALUE##*_}
case "$FIRMWARE_BASE" in
    74.00.324|74.00.324A) ;;
    *) exit 67 ;;
esac

REPORT="$USB_ROOT/mazda-cmu-report"
mkdir "$REPORT" 2>/dev/null || exit 73
MANIFEST="$REPORT/manifest.tsv"
printf 'mazda-cmu-report\t1\nsource\tstatus\tbytes\n' >"$MANIFEST" 2>/dev/null || exit 74

record_status() {
    printf '%s\t%s\t%s\n' "$1" "$2" "$3" >>"$MANIFEST" 2>/dev/null || exit 74
}

finalize_capture() {
    source_name=$1
    output_name=$2
    part_file=$3
    capture_status=$4
    byte_count=$(wc -c <"$part_file" 2>/dev/null | tr -d ' ')
    case "$byte_count" in
        ''|*[!0-9]*) byte_count=0 ;;
    esac

    if [ "$byte_count" -gt "$MAX_BYTES" ]; then
        if dd if="$part_file" of="$REPORT/$output_name" bs="$BLOCK_SIZE" \
            count="$BLOCKS_WITHOUT_SENTINEL" 2>/dev/null; then
            rm -f "$part_file"
            record_status "$source_name" truncated "$MAX_BYTES"
        else
            record_status "$source_name" io_error 0
        fi
    elif mv "$part_file" "$REPORT/$output_name" 2>/dev/null; then
        record_status "$source_name" "$capture_status" "$byte_count"
    else
        record_status "$source_name" io_error 0
    fi
}

capture_file() {
    relative_path=$1
    output_name=$2
    source_path="${CMU_ROOT}/$relative_path"
    part_file="$REPORT/.$output_name.part"

    if [ ! -e "$source_path" ]; then
        record_status "$relative_path" not_found 0
    elif [ ! -f "$source_path" ]; then
        record_status "$relative_path" not_regular_file 0
    elif [ ! -r "$source_path" ]; then
        record_status "$relative_path" permission_denied 0
    elif dd if="$source_path" of="$part_file" bs="$BLOCK_SIZE" \
        count="$BLOCKS_WITH_SENTINEL" 2>/dev/null; then
        finalize_capture "$relative_path" "$output_name" "$part_file" ok
    else
        rm -f "$part_file"
        record_status "$relative_path" io_error 0
    fi
}

capture_command() {
    source_name=$1
    output_name=$2
    program=$3
    shift 3
    part_file="$REPORT/.$output_name.part"
    status_file="$REPORT/.$output_name.status"

    if [ ! -x "$program" ]; then
        record_status "$source_name" not_found 0
        return
    fi

    (
        "$program" "$@" 2>/dev/null
        printf '%s\n' "$?" >"$status_file"
    ) | dd of="$part_file" bs="$BLOCK_SIZE" count="$BLOCKS_WITH_SENTINEL" 2>/dev/null
    command_status=$(cat "$status_file" 2>/dev/null)
    rm -f "$status_file"
    if [ "$command_status" = "0" ]; then
        finalize_capture "$source_name" "$output_name" "$part_file" ok
    else
        finalize_capture "$source_name" "$output_name" "$part_file" command_error
    fi
}

capture_network_module_files() {
    output_name=usb-network-modules.txt
    part_file="$REPORT/.$output_name.part"
    (
        for module_path in "${CMU_ROOT}"/lib/modules/*/kernel/drivers/net/usb/*.ko*; do
            [ -f "$module_path" ] || continue
            printf '%s\n' "${module_path#"${CMU_ROOT}"/}"
        done
    ) | dd of="$part_file" bs="$BLOCK_SIZE" count="$BLOCKS_WITH_SENTINEL" 2>/dev/null
    finalize_capture module-files/usb-network "$output_name" "$part_file" ok
}

capture_file jci/version.ini firmware-version.ini
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
capture_file proc/net/dev network-devices.txt
capture_file proc/net/route network-routes.txt
capture_file proc/net/arp network-arp.txt
capture_file proc/bus/usb/devices usb-devices.txt
capture_file sys/class/graphics/fb0/name framebuffer-name.txt
capture_file sys/class/graphics/fb0/modes framebuffer-modes.txt
capture_file sys/class/drm/card0/device/uevent drm-device.txt
capture_network_module_files

capture_command uname uname.txt /bin/uname -a
capture_command processes processes.txt /bin/ps
capture_command filesystems filesystems.txt /bin/df -k
capture_command interfaces interfaces.txt /sbin/ifconfig -a
capture_command busybox-applets busybox-applets.txt /bin/busybox --list

printf 'result\tcomplete\n' >>"$MANIFEST" 2>/dev/null || exit 74
if [ "${MAZDA_CMU_INSPECT_TESTING:-0}" != "1" ]; then
    [ -x /bin/sync ] || exit 75
    /bin/sync >/dev/null 2>&1 || exit 75
fi
exit 0
