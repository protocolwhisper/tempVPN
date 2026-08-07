use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde::Serialize;
use thiserror::Error;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Error)]
pub enum Error {
    #[error("configuration error: {0}")]
    Config(String),

    #[error("cryptography error: {0}")]
    Crypto(String),

    #[error("unsupported database schema {found}; maximum supported version is {supported}")]
    UnsupportedSchema { found: i64, supported: i64 },

    #[error("{0} not found")]
    NotFound(&'static str),

    #[error("conflict: {0}")]
    Conflict(&'static str),

    #[error("invalid {0}")]
    Invalid(&'static str),

    #[error("authentication required")]
    Unauthorized,

    #[error("operation is outside the authenticated identity scope")]
    Forbidden,

    #[error("certificate error: {0}")]
    Certificate(String),

    #[error(transparent)]
    Database(#[from] rusqlite::Error),

    #[error(transparent)]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Serialize)]
struct ErrorResponse<'a> {
    error: &'a str,
}

impl IntoResponse for Error {
    fn into_response(self) -> Response {
        let status = match &self {
            Self::NotFound(_) => StatusCode::NOT_FOUND,
            Self::Conflict(_) => StatusCode::CONFLICT,
            Self::Unauthorized => StatusCode::UNAUTHORIZED,
            Self::Forbidden => StatusCode::FORBIDDEN,
            Self::Invalid(_) | Self::Config(_) | Self::Crypto(_) | Self::Certificate(_) => {
                StatusCode::BAD_REQUEST
            }
            Self::UnsupportedSchema { .. } | Self::Database(_) | Self::Io(_) => {
                StatusCode::SERVICE_UNAVAILABLE
            }
        };
        let message = self.to_string();
        (status, Json(ErrorResponse { error: &message })).into_response()
    }
}
