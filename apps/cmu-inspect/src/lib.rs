//! Allowlisted, read-only inspection of a Mazda Connect CMU.
//!
//! The collector deliberately has no transport, command execution, privilege escalation,
//! device access, or filesystem-write capability. It reads a fixed set of Linux metadata
//! files plus process names and returns a deterministic report.

use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::Path;

const MAX_SOURCE_BYTES: usize = 256 * 1024;
const SCHEMA_VERSION: u8 = 1;

const FILE_SOURCES: &[&str] = &[
    "proc/version",
    "proc/cmdline",
    "proc/cpuinfo",
    "proc/meminfo",
    "proc/mounts",
    "proc/modules",
    "proc/bus/input/devices",
    "etc/os-release",
    "etc/issue",
    "sys/class/graphics/fb0/name",
    "sys/class/graphics/fb0/modes",
    "sys/class/drm/card0/device/uevent",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status {
    Ok,
    Truncated,
    NotFound,
    PermissionDenied,
    NotRegularFile,
    IoError,
}

impl Status {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Truncated => "truncated",
            Self::NotFound => "not_found",
            Self::PermissionDenied => "permission_denied",
            Self::NotRegularFile => "not_regular_file",
            Self::IoError => "io_error",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Observation {
    source: &'static str,
    status: Status,
    content: Option<String>,
}

impl Observation {
    const fn unavailable(source: &'static str, status: Status) -> Self {
        Self {
            source,
            status,
            content: None,
        }
    }
}

/// A complete inspection report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    observations: Vec<Observation>,
}

impl Report {
    /// Writes this report as JSON.
    ///
    /// # Errors
    ///
    /// Returns an error if the destination cannot be written.
    pub fn write_json(&self, mut destination: impl Write) -> io::Result<()> {
        write!(
            destination,
            "{{\n  \"schema_version\": {SCHEMA_VERSION},\n  \"observations\": ["
        )?;

        for (index, observation) in self.observations.iter().enumerate() {
            if index > 0 {
                write!(destination, ",")?;
            }

            write!(destination, "\n    {{\"source\":")?;
            write_json_string(&mut destination, observation.source)?;
            write!(destination, ",\"status\":")?;
            write_json_string(&mut destination, observation.status.as_str())?;
            write!(destination, ",\"content\":")?;
            if let Some(content) = &observation.content {
                write_json_string(&mut destination, content)?;
            } else {
                write!(destination, "null")?;
            }
            write!(destination, "}}")?;
        }

        write!(destination, "\n  ]\n}}\n")
    }
}

/// Reads the fixed CMU inspection allowlist beneath `root`.
///
/// Passing `/` inspects the running system. An alternate root exists so the exact behavior can
/// be exercised against fixtures without privileged or vehicle hardware access.
#[must_use]
pub fn inspect_root(root: &Path) -> Report {
    let mut observations = FILE_SOURCES
        .iter()
        .map(|source| inspect_file(root, source))
        .collect::<Vec<_>>();
    observations.push(inspect_process_names(root));
    Report { observations }
}

fn inspect_file(root: &Path, source: &'static str) -> Observation {
    match read_limited_file(&root.join(source)) {
        Ok((status, bytes)) => Observation {
            source,
            status,
            content: Some(String::from_utf8_lossy(&bytes).into_owned()),
        },
        Err(status) => Observation::unavailable(source, status),
    }
}

fn inspect_process_names(root: &Path) -> Observation {
    const SOURCE: &str = "proc/processes";

    let entries = match fs::read_dir(root.join("proc")) {
        Ok(entries) => entries,
        Err(error) => return Observation::unavailable(SOURCE, status_from_error(&error)),
    };

    let mut processes = Vec::new();
    for entry in entries {
        let Ok(entry) = entry else {
            continue;
        };
        let Some(file_name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        let Ok(process_id) = file_name.parse::<u32>() else {
            continue;
        };
        let Ok((_, bytes)) = read_limited_file(&entry.path().join("comm")) else {
            // Processes can exit while /proc is being inspected, so an unreadable entry is not
            // a failure of the report as a whole.
            continue;
        };
        let name = String::from_utf8_lossy(&bytes)
            .trim_end_matches(['\r', '\n'])
            .chars()
            .map(|character| {
                if character.is_control() {
                    ' '
                } else {
                    character
                }
            })
            .collect::<String>();
        processes.push((process_id, name));
    }
    processes.sort_unstable_by_key(|(process_id, _)| *process_id);

    let mut content = String::new();
    for (process_id, name) in processes {
        use std::fmt::Write as _;
        let _ = writeln!(content, "{process_id}\t{name}");
    }

    Observation {
        source: SOURCE,
        status: Status::Ok,
        content: Some(content),
    }
}

fn read_limited_file(path: &Path) -> Result<(Status, Vec<u8>), Status> {
    let metadata = fs::metadata(path).map_err(|error| status_from_error(&error))?;
    if !metadata.is_file() {
        return Err(Status::NotRegularFile);
    }

    let file = File::open(path).map_err(|error| status_from_error(&error))?;
    let read_limit = u64::try_from(MAX_SOURCE_BYTES + 1).expect("read limit fits in u64");
    let mut bytes = Vec::new();
    file.take(read_limit)
        .read_to_end(&mut bytes)
        .map_err(|error| status_from_error(&error))?;

    if bytes.len() > MAX_SOURCE_BYTES {
        bytes.truncate(MAX_SOURCE_BYTES);
        Ok((Status::Truncated, bytes))
    } else {
        Ok((Status::Ok, bytes))
    }
}

fn status_from_error(error: &io::Error) -> Status {
    match error.kind() {
        io::ErrorKind::NotFound => Status::NotFound,
        io::ErrorKind::PermissionDenied => Status::PermissionDenied,
        _ => Status::IoError,
    }
}

fn write_json_string(destination: &mut impl Write, value: &str) -> io::Result<()> {
    write!(destination, "\"")?;
    for character in value.chars() {
        match character {
            '"' => write!(destination, "\\\"")?,
            '\\' => write!(destination, "\\\\")?,
            '\u{08}' => write!(destination, "\\b")?,
            '\u{0c}' => write!(destination, "\\f")?,
            '\n' => write!(destination, "\\n")?,
            '\r' => write!(destination, "\\r")?,
            '\t' => write!(destination, "\\t")?,
            control if control <= '\u{1f}' => {
                write!(destination, "\\u{:04x}", u32::from(control))?;
            }
            printable => write!(destination, "{printable}")?,
        }
    }
    write!(destination, "\"")
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    struct TestRoot(PathBuf);

    impl TestRoot {
        fn new() -> Self {
            let unique = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("clock is after Unix epoch")
                .as_nanos();
            let path = std::env::temp_dir()
                .join(format!("mazda-cmu-inspect-{}-{unique}", std::process::id()));
            fs::create_dir_all(&path).expect("create test root");
            Self(path)
        }

        fn write(&self, relative: &str, content: &[u8]) {
            let path = self.0.join(relative);
            fs::create_dir_all(path.parent().expect("fixture has a parent"))
                .expect("create fixture parent");
            fs::write(path, content).expect("write fixture");
        }
    }

    impl Drop for TestRoot {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).expect("remove test root");
        }
    }

    #[test]
    fn report_reads_only_allowlisted_sources_and_sorts_processes() {
        let root = TestRoot::new();
        root.write("proc/version", b"Linux test\n");
        root.write("proc/20/comm", b"later\n");
        root.write("proc/3/comm", b"first\n");
        root.write("not-allowlisted", b"must not appear");

        let report = inspect_root(&root.0);

        let version = report
            .observations
            .iter()
            .find(|observation| observation.source == "proc/version")
            .expect("version observation");
        assert_eq!(version.status, Status::Ok);
        assert_eq!(version.content.as_deref(), Some("Linux test\n"));

        let processes = report
            .observations
            .iter()
            .find(|observation| observation.source == "proc/processes")
            .expect("process observation");
        assert_eq!(processes.content.as_deref(), Some("3\tfirst\n20\tlater\n"));
        assert!(report
            .observations
            .iter()
            .all(|observation| observation.source != "not-allowlisted"));
    }

    #[test]
    fn unavailable_and_oversized_sources_are_explicit() {
        let root = TestRoot::new();
        root.write("proc/cpuinfo", &vec![b'x'; MAX_SOURCE_BYTES + 1]);

        let report = inspect_root(&root.0);
        let cpu = report
            .observations
            .iter()
            .find(|observation| observation.source == "proc/cpuinfo")
            .expect("cpu observation");
        assert_eq!(cpu.status, Status::Truncated);
        assert_eq!(
            cpu.content.as_ref().map(String::len),
            Some(MAX_SOURCE_BYTES)
        );

        let memory = report
            .observations
            .iter()
            .find(|observation| observation.source == "proc/meminfo")
            .expect("memory observation");
        assert_eq!(memory.status, Status::NotFound);
        assert_eq!(memory.content, None);
    }

    #[test]
    fn json_output_escapes_untrusted_source_content() {
        let report = Report {
            observations: vec![Observation {
                source: "fixture",
                status: Status::Ok,
                content: Some("quote=\" slash=\\ line=\n nul=\0 snowman=☃".to_owned()),
            }],
        };
        let mut output = Vec::new();

        report.write_json(&mut output).expect("write report");

        assert_eq!(
            String::from_utf8(output).expect("valid UTF-8"),
            "{\n  \"schema_version\": 1,\n  \"observations\": [\n    {\"source\":\"fixture\",\"status\":\"ok\",\"content\":\"quote=\\\" slash=\\\\ line=\\n nul=\\u0000 snowman=☃\"}\n  ]\n}\n"
        );
    }
}
