pub mod asana;
pub mod cli;
pub mod cmd;
pub mod config;
pub mod error;
pub mod mock;
pub mod output;
pub mod server;
pub mod skills;
pub mod util;

pub use error::{Error, Result};

pub async fn run() -> Result<()> {
    cli::run().await
}
