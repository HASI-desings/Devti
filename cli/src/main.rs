//! `devti` binary: parses the command line and prints results; see `lib.rs`.

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if matches!(args.first().map(String::as_str), None | Some("help" | "--help" | "-h" | "--version" | "-V")) {
        let api = devti_cli::Endpoint { port: 0, token: String::new() };
        println!("{}", devti_cli::run(&args, &api).unwrap_or_default());
        return ExitCode::SUCCESS;
    }
    let api = match devti_cli::Endpoint::discover() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(2);
        }
    };
    if args[0] == "watch" {
        return match devti_cli::watch(&api, |line| {
            println!("{line}");
            true
        }) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("{e}");
                ExitCode::FAILURE
            }
        };
    }
    match devti_cli::run(&args, &api) {
        Ok(out) => {
            if !out.is_empty() {
                println!("{out}");
            }
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}
