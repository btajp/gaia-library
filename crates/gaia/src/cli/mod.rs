//! CLI。全コマンドが ToolService::call を経由する（例外は init / affiliation / client の管理系のみ）。
mod admin_cmd;
mod app;
mod serve;

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};
use serde_json::json;

#[derive(Parser)]
#[command(
    name = "gaia",
    version,
    about = "gaia-library: 仕事の記憶の索引 MCP サーバー"
)]
pub struct Cli {
    /// 設定ファイルのパス（既定: $GAIA_CONFIG → ~/.config/gaia-library/config.toml）
    #[arg(long, global = true)]
    pub config: Option<PathBuf>,
    /// 操作するクライアント名（既定: [cli].default_client）
    #[arg(long, global = true)]
    pub client: Option<String>,
    /// 1 行 JSON で出力（既定は整形済み JSON）
    #[arg(long, global = true)]
    pub json: bool,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Args)]
pub struct InitArgs {
    /// 最初の機密境界名（例: cloudnative）
    #[arg(long)]
    pub affiliation: String,
    #[arg(long)]
    pub identity: Option<String>,
    /// human クライアント名（既定: $USER）
    #[arg(long)]
    pub client_name: Option<String>,
    /// DB パス（既定: ~/.local/share/gaia-library/gaia.db）
    #[arg(long)]
    pub db: Option<PathBuf>,
}

#[derive(Subcommand)]
pub enum Command {
    /// 設定と DB を初期化する
    Init(InitArgs),
    /// MCP サーバーを起動する
    Serve(serve::ServeArgs),
    /// 機密境界（affiliation）の管理
    Affiliation {
        #[command(subcommand)]
        cmd: admin_cmd::AffiliationCmd,
    },
    /// クライアント（識別）の管理
    Client {
        #[command(subcommand)]
        cmd: admin_cmd::ClientCmd,
    },
    /// サーバー情報（get_server_info）
    Info,
    /// 任意ツールの汎用呼び出し
    Call {
        tool: String,
        #[arg(long)]
        args: String,
    },
}

pub fn run(cli: Cli) -> anyhow::Result<()> {
    let compact = cli.json;
    match &cli.command {
        Command::Init(args) => app::init(args, cli.config.as_ref()),
        Command::Client { cmd } => {
            let path = app::resolve_config_path(cli.config.as_ref())?;
            admin_cmd::client(&path, cmd, compact)
        }
        Command::Serve(args) => {
            let app = app::App::open(cli.config.as_ref())?;
            serve::serve(app, cli.client.as_deref(), args)
        }
        Command::Affiliation { cmd } => {
            let app = app::App::open(cli.config.as_ref())?;
            admin_cmd::affiliation(&app, cli.client.as_deref(), cmd, compact)
        }
        Command::Info => {
            let app = app::App::open(cli.config.as_ref())?;
            let client = app.identity(cli.client.as_deref())?;
            let out = app.call(&client, "get_server_info", json!({}))?;
            app::print_json(&out, compact);
            Ok(())
        }
        Command::Call { tool, args } => {
            let app = app::App::open(cli.config.as_ref())?;
            let client = app.identity(cli.client.as_deref())?;
            let value: serde_json::Value =
                serde_json::from_str(args).map_err(|e| anyhow::anyhow!("--args は JSON: {e}"))?;
            let out = app.call(&client, tool, value)?;
            app::print_json(&out, compact);
            Ok(())
        }
    }
}
