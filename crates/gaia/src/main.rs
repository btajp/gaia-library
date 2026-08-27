use clap::Parser;
use serde_json::json;

mod cli;

fn main() {
    // ログは stderr へ。stdout は serve の JSON-RPC と JSON 出力専用。
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();
    let json_requested = std::env::args_os().skip(1).any(|arg| arg == "--json");
    let cli = match cli::Cli::try_parse() {
        Ok(cli) => cli,
        Err(error) if json_requested && error.use_stderr() => {
            eprintln!(
                "{}",
                json!({
                    "code": "invalid_params",
                    "message": error.to_string(),
                    "details": {"kind": format!("{:?}", error.kind())},
                })
            );
            std::process::exit(error.exit_code());
        }
        Err(error) => error.exit(),
    };
    let json_output = cli.json;
    if let Err(e) = cli::run(cli) {
        let tool_error = e.downcast_ref::<gaia_core::error::ToolError>();
        if json_output {
            let error = tool_error
                .map(gaia_core::error::ToolError::to_json)
                .unwrap_or_else(|| {
                    json!({
                        "code": "internal",
                        "message": format!("{e:#}"),
                        "details": null,
                    })
                });
            eprintln!("{error}");
        } else if let Some(error) = tool_error {
            eprintln!("error: {error}");
            if let Some(details) = &error.details {
                eprintln!("details: {details:#}");
            }
        } else {
            eprintln!("error: {e:#}");
        }
        std::process::exit(1);
    }
}
