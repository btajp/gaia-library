use gaia_desktop_lib::updater_signature::verify_artifact;
use std::{env, process::ExitCode};

fn main() -> ExitCode {
    let args = env::args().skip(1).collect::<Vec<_>>();
    if args.len() != 3 {
        eprintln!("使い方: verify-updater-signature <artifact> <signature-file> <public-key>");
        return ExitCode::from(2);
    }
    match verify_artifact(&args[0], &args[1], &args[2]) {
        Ok(()) => {
            println!("updater 生成物の署名を検証しました");
            ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("ERROR: {message}");
            ExitCode::FAILURE
        }
    }
}
