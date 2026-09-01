use std::env;
use std::path::Path;
use std::process::ExitCode;

use mazda_cmu_inspect::{prepare_usb, SUPPORTED_FIRMWARE};

fn usage() {
    eprintln!(
        "Usage:\n  mazda-cmu-inspect prepare-usb --firmware {SUPPORTED_FIRMWARE} /Volumes/<drive>"
    );
}

fn main() -> ExitCode {
    let arguments = env::args().skip(1).collect::<Vec<_>>();
    if arguments.as_slice() == ["--help"] || arguments.as_slice() == ["-h"] {
        usage();
        return ExitCode::SUCCESS;
    }

    let [command, firmware_flag, firmware, destination] = arguments.as_slice() else {
        usage();
        return ExitCode::from(64);
    };
    if command != "prepare-usb" || firmware_flag != "--firmware" {
        usage();
        return ExitCode::from(64);
    }

    if let Err(error) = prepare_usb(Path::new(destination), firmware) {
        eprintln!("could not prepare CMU inspection USB: {error}");
        return ExitCode::FAILURE;
    }

    println!("Prepared report-only CMU payload for {SUPPORTED_FIRMWARE} at {destination}");
    ExitCode::SUCCESS
}
