use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("coordinator configuration error: {0}")]
    Configuration(String),

    #[error("coordinator is unavailable: {0}")]
    Unavailable(String),

    #[error("coordinator rejected the request with HTTP {status}: {message}")]
    Rejected { status: u16, message: String },

    #[error("coordinator returned an invalid response: {0}")]
    Protocol(String),

    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Http(#[from] reqwest::Error),
}
