use axum::{response::IntoResponse, http::{StatusCode, header}};

#[derive(Debug, thiserror::Error)]
pub enum GititError {
    #[error("template error: {0}")]
    LiquidError(#[from] liquid::Error),
    #[error("git error: {0}")]
    GitError(#[from] git2::Error),
    #[error("not found")]
    NotFound,
    #[error("redirect to: {0}")]
    Redirect(String),
    #[error("highlighting error: {0}")]
    HighlightingError(#[from] syntect::Error),
    #[error("missing config")]
    MissingConfig,
    #[error("io error: {0}")]
    IOError(#[from] std::io::Error),
    #[error("toml parser error: {0}")]
    TomlError(#[from] toml::de::Error),
}

impl IntoResponse for GititError {
    fn into_response(self) -> axum::response::Response {
        tracing::error!("{}", self);
        let (status, body) = match self {
            GititError::LiquidError(_) => (StatusCode::INTERNAL_SERVER_ERROR, "templating error"),
            GititError::GitError(_) => (StatusCode::INTERNAL_SERVER_ERROR, "git error"),
            GititError::NotFound => (StatusCode::NOT_FOUND, "not found"),
            GititError::HighlightingError(_) => (StatusCode::INTERNAL_SERVER_ERROR, "highlighting error"),
            GititError::Redirect(target) => {
                return (StatusCode::TEMPORARY_REDIRECT, [(header::LOCATION, target)]).into_response();
            }
            _ => (StatusCode::INTERNAL_SERVER_ERROR, "internal server error")
        };
        (status, body).into_response()
    }
}

pub type Result<T> = std::result::Result<T, GititError>;
