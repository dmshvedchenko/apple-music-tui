use std::io;

use thiserror::Error;

use crate::auth::AuthError;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("invalid command line: {0}")]
    InvalidArguments(String),

    #[error("backend '{0}' is not implemented yet; run with --backend mock")]
    UnavailableBackend(String),

    #[error("terminal operation failed: {0}")]
    Terminal(#[source] io::Error),

    #[error("terminal input failed: {0}")]
    Input(#[source] io::Error),

    #[error("background task failed: {0}")]
    Task(#[from] tokio::task::JoinError),

    #[error(transparent)]
    Authentication(#[from] AuthError),
}
