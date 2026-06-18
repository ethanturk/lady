//! Binary entry point: `lady-cli <repo-path>`.

use std::path::PathBuf;
use std::process::ExitCode;

fn main() -> ExitCode {
    let Some(arg) = std::env::args_os().nth(1) else {
        eprintln!("usage: lady-cli <repo-path>");
        return ExitCode::FAILURE;
    };
    match lady_cli::report(&PathBuf::from(arg)) {
        Ok(report) => {
            print!("{report}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}
