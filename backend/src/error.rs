use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::{json, Map, Value};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AppError {
    #[error("bad request: {0}")]
    BadRequest(String),

    #[error("unauthorized: {0}")]
    Unauthorized(String),

    #[error("forbidden: {0}")]
    Forbidden(String),

    #[error("not found: {0}")]
    NotFound(String),

    #[error("SoundCloud API error (status {status})")]
    ScApi { status: u16, body: Value },

    #[error("SoundCloud API unreachable: {0}")]
    ScUnreachable(String),

    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),

    #[error("redis error: {0}")]
    Redis(#[from] deadpool_redis::redis::RedisError),

    #[error("redis pool error: {0}")]
    RedisPool(#[from] deadpool_redis::PoolError),

    #[error("http client error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("internal error: {0}")]
    Internal(String),
}

impl AppError {
    pub fn unauthorized(msg: impl Into<String>) -> Self {
        Self::Unauthorized(msg.into())
    }

    pub fn forbidden(msg: impl Into<String>) -> Self {
        Self::Forbidden(msg.into())
    }

    pub fn bad_request(msg: impl Into<String>) -> Self {
        Self::BadRequest(msg.into())
    }

    pub fn not_found(msg: impl Into<String>) -> Self {
        Self::NotFound(msg.into())
    }

    pub fn internal(msg: impl Into<String>) -> Self {
        Self::Internal(msg.into())
    }

    pub fn status(&self) -> StatusCode {
        match self {
            Self::BadRequest(_) => StatusCode::BAD_REQUEST,
            Self::Unauthorized(_) => StatusCode::UNAUTHORIZED,
            Self::Forbidden(_) => StatusCode::FORBIDDEN,
            Self::NotFound(_) => StatusCode::NOT_FOUND,
            Self::ScApi { status, .. } => {
                StatusCode::from_u16(*status).unwrap_or(StatusCode::BAD_GATEWAY)
            }
            Self::ScUnreachable(_) => StatusCode::BAD_GATEWAY,
            Self::Db(_)
            | Self::Redis(_)
            | Self::RedisPool(_)
            | Self::Http(_)
            | Self::Internal(_) => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status = self.status();

        if status.is_server_error() {
            tracing::error!(error = %self, "request failed");
        } else {
            tracing::debug!(error = %self, status = %status, "request rejected");
        }

        let body = match &self {
            Self::ScApi { status: _, body } => {
                let mut merged = Map::new();
                merged.insert("statusCode".into(), json!(status.as_u16()));
                merged.insert("error".into(), json!("SoundCloud API error"));
                match body {
                    Value::Object(obj) => {
                        for (k, v) in obj {
                            merged.insert(k.clone(), v.clone());
                        }
                    }
                    Value::Null => {}
                    other => {
                        merged.insert("message".into(), json!(other.to_string()));
                    }
                }
                Value::Object(merged)
            }
            _ => json!({
                "statusCode": status.as_u16(),
                "message": self.public_message(),
                "error": status.canonical_reason().unwrap_or("Error"),
            }),
        };

        (status, Json(body)).into_response()
    }
}

impl AppError {
    fn public_message(&self) -> String {
        match self {
            Self::BadRequest(m)
            | Self::Unauthorized(m)
            | Self::Forbidden(m)
            | Self::NotFound(m)
            | Self::ScUnreachable(m)
            | Self::Internal(m) => m.clone(),
            Self::ScApi { body, .. } => body.to_string(),
            Self::Db(_) | Self::Redis(_) | Self::RedisPool(_) | Self::Http(_) => {
                "Internal server error".to_string()
            }
        }
    }
}

pub type AppResult<T> = Result<T, AppError>;
