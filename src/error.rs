use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("{command} is not implemented yet; this scaffold is reserved for {plan}")]
    NotImplemented {
        command: &'static str,
        plan: &'static str,
    },

    #[error("{0}")]
    Config(String),

    #[error("{0}")]
    Command(String),

    #[error(transparent)]
    Unexpected(#[from] anyhow::Error),
}
