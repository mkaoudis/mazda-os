use std::io;
use std::path::Path;
use std::process::ExitCode;

fn main() -> ExitCode {
    let report = mazda_cmu_inspect::inspect_root(Path::new("/"));
    if let Err(error) = report.write_json(io::stdout().lock()) {
        eprintln!("could not write CMU inspection report: {error}");
        return ExitCode::FAILURE;
    }

    ExitCode::SUCCESS
}
