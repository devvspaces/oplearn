//! Application error type. Serialized to the renderer as `{ code, message }`.

use serde::Serialize;
use specta::Type;

#[derive(Debug, Clone, Copy, Serialize, Type, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    Db,
    Io,
    Http,
    Quota,
    YtDlp,
    YtDlpMissing,
    NotFound,
    Invalid,
    Keyring,
    Cancelled,
    Timeout,
    Internal,
}

#[derive(Debug, Clone, Serialize, Type, thiserror::Error)]
#[error("{code:?}: {message}")]
pub struct AppError {
    pub code: ErrorCode,
    pub message: String,
}

impl AppError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::Invalid, message)
    }
    pub fn not_found(what: impl Into<String>) -> Self {
        Self::new(ErrorCode::NotFound, format!("{} not found", what.into()))
    }
    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::Internal, message)
    }
}

impl From<rusqlite::Error> for AppError {
    fn from(e: rusqlite::Error) -> Self {
        match e {
            rusqlite::Error::QueryReturnedNoRows => {
                Self::new(ErrorCode::NotFound, "record not found")
            }
            other => Self::new(ErrorCode::Db, other.to_string()),
        }
    }
}

impl From<std::io::Error> for AppError {
    fn from(e: std::io::Error) -> Self {
        Self::new(ErrorCode::Io, e.to_string())
    }
}

impl From<reqwest::Error> for AppError {
    fn from(e: reqwest::Error) -> Self {
        if e.is_timeout() {
            Self::new(ErrorCode::Timeout, e.to_string())
        } else {
            Self::new(ErrorCode::Http, e.to_string())
        }
    }
}

impl From<serde_json::Error> for AppError {
    fn from(e: serde_json::Error) -> Self {
        Self::new(ErrorCode::Internal, format!("json: {e}"))
    }
}

impl From<keyring::Error> for AppError {
    fn from(e: keyring::Error) -> Self {
        Self::new(ErrorCode::Keyring, e.to_string())
    }
}

pub type AppResult<T> = Result<T, AppError>;
