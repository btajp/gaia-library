//! CLI。全コマンドが ToolService::call を経由する（例外は init / affiliation / client の管理系のみ）。
mod admin_cmd;
mod app;
mod query;
mod serve;
mod write;

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
    /// 横断検索（search_context）
    Search(query::SearchArgs),
    /// 人物の詳細（get_person）
    Person {
        #[command(subcommand)]
        cmd: query::GetCmd,
    },
    /// 組織の詳細（get_organization）
    Org {
        #[command(subcommand)]
        cmd: query::GetCmd,
    },
    /// 案件の詳細（get_engagement）
    Engagement {
        #[command(subcommand)]
        cmd: query::GetCmd,
    },
    /// 用語集と語彙ヒント（get_glossary）
    Glossary(query::GlossaryArgs),
    /// 表示名の人物突合（resolve_speakers）
    Speakers(query::SpeakersArgs),
    /// 更新の提案（propose_update）
    Propose(write::ProposeArgs),
    /// 提案の一覧（list_proposals）
    Proposals(write::ProposalsArgs),
    /// 提案の承認（human）
    Approve { proposal_id: i64 },
    /// 提案の却下（human）
    Reject {
        proposal_id: i64,
        #[arg(long)]
        reason: Option<String>,
    },
    /// 提案＋即時承認（human）
    Add {
        #[command(subcommand)]
        cmd: write::AddCmd,
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
        Command::Search(a) => with_app(&cli, |app, client| query::search(app, client, a, compact)),
        Command::Person { cmd } => with_app(&cli, |app, client| {
            query::get_entity(app, client, "get_person", "person_id", cmd, compact)
        }),
        Command::Org { cmd } => with_app(&cli, |app, client| {
            query::get_entity(
                app,
                client,
                "get_organization",
                "organization_id",
                cmd,
                compact,
            )
        }),
        Command::Engagement { cmd } => with_app(&cli, |app, client| {
            query::get_entity(app, client, "get_engagement", "engagement_id", cmd, compact)
        }),
        Command::Glossary(a) => {
            with_app(&cli, |app, client| query::glossary(app, client, a, compact))
        }
        Command::Speakers(a) => {
            with_app(&cli, |app, client| query::speakers(app, client, a, compact))
        }
        Command::Propose(a) => {
            with_app(&cli, |app, client| write::propose(app, client, a, compact))
        }
        Command::Proposals(a) => with_app(&cli, |app, client| {
            write::proposals(app, client, a, compact)
        }),
        Command::Approve { proposal_id } => with_app(&cli, |app, client| {
            let out = app.call(
                client,
                "approve_proposal",
                json!({"proposal_id": proposal_id}),
            )?;
            app::print_json(&out, compact);
            Ok(())
        }),
        Command::Reject {
            proposal_id,
            reason,
        } => with_app(&cli, |app, client| {
            let mut args = json!({"proposal_id": proposal_id});
            if let Some(r) = reason {
                args["reason"] = json!(r);
            }
            let out = app.call(client, "reject_proposal", args)?;
            app::print_json(&out, compact);
            Ok(())
        }),
        Command::Add { cmd } => with_app(&cli, |app, client| write::add(app, client, cmd, compact)),
    }
}

fn with_app(
    cli: &Cli,
    f: impl FnOnce(&app::App, &gaia_core::identity::ClientIdentity) -> anyhow::Result<()>,
) -> anyhow::Result<()> {
    let app = app::App::open(cli.config.as_ref())?;
    let client = app.identity(cli.client.as_deref())?;
    f(&app, &client)
}
