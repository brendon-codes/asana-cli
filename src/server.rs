use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;

use clap::Args;

use crate::error::{Error, Result};
use crate::mock;

#[derive(Debug, Args)]
pub struct ServerArgs {
    #[arg(long, default_value = "127.0.0.1", help = "Host interface to bind")]
    pub host: IpAddr,
    #[arg(
        long,
        default_value_t = 0,
        help = "TCP port to bind; 0 picks a free port"
    )]
    pub port: u16,
    #[arg(
        long,
        help = "Override the mock data directory; defaults to <repo-root>/.asana/data"
    )]
    pub data_dir: Option<PathBuf>,
}

pub async fn run(args: ServerArgs) -> Result<()> {
    let data_dir = match args.data_dir {
        Some(data_dir) => data_dir,
        None => {
            let current_dir = std::env::current_dir().map_err(|error| {
                Error::Command(format!("failed to read current directory: {error}"))
            })?;
            crate::config::find_repo_root_from(current_dir)?.join(".asana/data")
        }
    };

    let bind = SocketAddr::new(args.host, args.port);
    mock::server::serve_until_ctrl_c(bind, data_dir).await
}
