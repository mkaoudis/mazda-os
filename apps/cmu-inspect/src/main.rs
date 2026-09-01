use std::env;
use std::path::Path;
use std::process::ExitCode;

#[cfg(test)]
use mazda_cmu_inspect::ObservationStatus;
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
        format_driver_observation(analysis)
    );
    println!(
        "USB-network modules currently loaded: {}",
        format_loaded_module_observation(analysis)
    );
    println!(
        "USB-network compatibility evidence present: {}",
        analysis.has_usb_network_compatibility_evidence()
    );
    println!(
        "Hardware insertion is not authorized by this report; USB hotplug can change CMU state."
    );
}

fn format_driver_observation(analysis: &ReportAnalysis) -> String {
    let drivers = analysis
        .available_usb_network_drivers
        .iter()
        .map(|driver| match driver {
            UsbNetworkDriver::Asix => "asix",
            UsbNetworkDriver::CdcEther => "cdc_ether",
            UsbNetworkDriver::CdcNcm => "cdc_ncm",
        })
        .collect::<Vec<_>>()
        .join(", ");
    let incomplete = [
        (
            "module-files/usb-network",
            analysis.usb_network_driver_files_status,
        ),
        ("proc/modules", analysis.loaded_usb_network_modules_status),
    ]
    .into_iter()
    .filter(|(_, status)| !status.is_complete())
    .map(|(source, status)| format!("{source}={status}"))
    .collect::<Vec<_>>()
    .join(", ");

    match (drivers.is_empty(), incomplete.is_empty()) {
        (true, true) => "none found".to_owned(),
        (true, false) => format!("observation unavailable: {incomplete}"),
        (false, true) => drivers,
        (false, false) => format!("{drivers} (additional observation unavailable: {incomplete})"),
    }
}

fn format_loaded_module_observation(analysis: &ReportAnalysis) -> String {
    if analysis.loaded_usb_network_modules.is_empty() {
        if analysis.loaded_usb_network_modules_status.is_complete() {
            "none found".to_owned()
        } else {
            format!(
                "observation unavailable: {}",
                analysis.loaded_usb_network_modules_status
            )
        }
    } else if analysis.loaded_usb_network_modules_status.is_complete() {
        analysis.loaded_usb_network_modules.join(", ")
    } else {
        format!(
            "{} (observation {})",
            analysis.loaded_usb_network_modules.join(", "),
            analysis.loaded_usb_network_modules_status
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn analysis_with_statuses(
        driver_files: ObservationStatus,
        loaded_modules: ObservationStatus,
    ) -> ReportAnalysis {
        ReportAnalysis {
            firmware: "70.00.100A-NA".to_owned(),
            software_part_number: "SWI10-24818-807R02".to_owned(),
            available_usb_network_drivers: Vec::new(),
            usb_network_driver_files_status: driver_files,
            loaded_usb_network_modules: Vec::new(),
            loaded_usb_network_modules_status: loaded_modules,
        }
    }

    #[test]
    fn complete_empty_observations_say_none_found() {
        let analysis = analysis_with_statuses(ObservationStatus::Ok, ObservationStatus::Ok);

        assert_eq!(format_driver_observation(&analysis), "none found");
        assert_eq!(format_loaded_module_observation(&analysis), "none found");
    }

    #[test]
    fn unavailable_observations_retain_their_statuses() {
        let analysis = analysis_with_statuses(
            ObservationStatus::NotFound,
            ObservationStatus::PermissionDenied,
        );

        assert_eq!(
            format_driver_observation(&analysis),
            "observation unavailable: module-files/usb-network=not_found, proc/modules=permission_denied"
        );
        assert_eq!(
            format_loaded_module_observation(&analysis),
            "observation unavailable: permission_denied"
        );
    }
}
