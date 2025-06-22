use std::ffi::OsString;

use clap::{Args, Parser, Subcommand};

use crate::cmd;
use crate::error::Result;
use crate::server::{self, ServerArgs};
use crate::skills::SkillTarget;
use crate::util::{self, MakeSkillArgs};

#[derive(Debug, Parser)]
#[command(
    name = "asana",
    version,
    about = "A staged Rust CLI for Asana REST API operations"
)]
pub struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    #[command(about = "Run Asana REST API command operations")]
    Cmd(PlaceholderArgs),
    #[command(about = "Run a local mock Asana REST API server")]
    Server(ServerArgs),
    #[command(about = "Run utility commands for Asana CLI development")]
    Util(UtilArgs),
}

#[derive(Debug, Args)]
struct PlaceholderArgs {}

#[derive(Debug, Args)]
struct UtilArgs {
    #[command(subcommand)]
    command: UtilCommand,
}

#[derive(Debug, Subcommand)]
enum UtilCommand {
    #[command(about = "Create a ~/.asana/asana.jsonc config")]
    MakeConfig,
    #[command(about = "Validate the ~/.asana/asana.jsonc config")]
    ValidateConfig,
    #[command(about = "Print config and Asana connectivity status")]
    Status(StatusArgs),
    #[command(about = "Generate project skills for AI coding agents")]
    MakeSkill(MakeSkillCliArgs),
}

#[derive(Debug, Args)]
struct StatusArgs {
    #[arg(long, help = "Override asanaBaseUrl from config")]
    base_url: Option<String>,
}

#[derive(Debug, Args)]
struct MakeSkillCliArgs {
    #[arg(value_enum)]
    target: SkillTarget,
}

pub async fn run() -> Result<()> {
    run_from(std::env::args_os()).await
}

pub async fn run_from(args: impl IntoIterator<Item = OsString>) -> Result<()> {
    let args = args.into_iter().collect::<Vec<_>>();
    if args.get(1).and_then(|arg| arg.to_str()) == Some("cmd") {
        return cmd::run_from(&args[2..]).await;
    }

    let cli = Cli::parse_from(args);
    execute(cli).await
}

async fn execute(cli: Cli) -> Result<()> {
    match cli.command {
        Command::Cmd(_) => cmd::run_from(&[]).await,
        Command::Server(args) => server::run(args).await,
        Command::Util(args) => execute_util(args).await,
    }
}

async fn execute_util(args: UtilArgs) -> Result<()> {
    match args.command {
        UtilCommand::MakeConfig => util::make_config().await,
        UtilCommand::ValidateConfig => util::validate_config().await,
        UtilCommand::Status(args) => util::status(args.base_url).await,
        UtilCommand::MakeSkill(args) => {
            util::make_skill(MakeSkillArgs {
                target: args.target,
            })
            .await
        }
    }
}
