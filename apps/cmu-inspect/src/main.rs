use std::env;
use std::path::Path;
use std::process::ExitCode;

use mazda_cmu_inspect::{
    analyze_report, prepare_usb, ReportAnalysis, UsbNetworkDriver, TARGET_CONFIRMATION,
    TARGET_DISPLAY_VERSION,
};

fn usage() {
    eprintln!(
        "Usage:\n  mazda-cmu-inspect prepare-usb --target {TARGET_CONFIRMATION} /Volumes/<drive>\n  mazda-cmu-inspect analyze-report /Volumes/<drive>/mazda-cmu-report\n\nTarget: 2019.5 Mazda CX-5 GT with display version {TARGET_DISPLAY_VERSION}"
    );
}

fn main() -> ExitCode {
    let arguments = env::args().skip(1).collect::<Vec<_>>();
    if arguments.as_slice() == ["--help"] || arguments.as_slice() == ["-h"] {
        usage();
        return ExitCode::SUCCESS;
    }

    if let [command, report_directory] = arguments.as_slice() {
        if command == "analyze-report" {
            return analyze(Path::new(report_directory));
        }
    }

    let [command, target_flag, target, destination] = arguments.as_slice() else {
        usage();
        return ExitCode::from(64);
    };
    if command != "prepare-usb" || target_flag != "--target" {
        usage();
        return ExitCode::from(64);
    }

    if let Err(error) = prepare_usb(Path::new(destination), target) {
        eprintln!("could not prepare CMU inspection USB: {error}");
        return ExitCode::FAILURE;
    }

    println!(
        "Prepared report-only CMU payload for 2019.5 CX-5 GT / {TARGET_DISPLAY_VERSION} at {destination}"
    );
    ExitCode::SUCCESS
}

fn analyze(report_directory: &Path) -> ExitCode {
    match analyze_report(report_directory) {
        Ok(analysis) => {
            print_analysis(&analysis);
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("could not analyze CMU report: {error}");
            ExitCode::FAILURE
        }
    }
}

fn print_analysis(analysis: &ReportAnalysis) {
    println!("Firmware: {} (supported)", analysis.firmware);
    println!(
        "Software part: {} (supported)",
        analysis.software_part_number
    );
    println!(
        "USB-network drivers available: {}",
        if analysis.available_usb_network_drivers.is_empty() {
            "none".to_owned()
        } else {
            analysis
                .available_usb_network_drivers
                .iter()
                .map(|driver| match driver {
                    UsbNetworkDriver::Asix => "asix",
                    UsbNetworkDriver::CdcEther => "cdc_ether",
                    UsbNetworkDriver::CdcNcm => "cdc_ncm",
                })
                .collect::<Vec<_>>()
                .join(", ")
        }
    );
    println!(
        "USB-network modules currently loaded: {}",
        if analysis.loaded_usb_network_modules.is_empty() {
            "none".to_owned()
        } else {
            analysis.loaded_usb_network_modules.join(", ")
        }
    );
    println!(
        "USB-network compatibility evidence present: {}",
        analysis.has_usb_network_compatibility_evidence()
    );
    println!(
        "Hardware insertion is not authorized by this report; USB hotplug can change CMU state."
    );
}
