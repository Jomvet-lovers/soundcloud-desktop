use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};

pub fn json_response(status: StatusCode, body: String) -> Response {
    (
        status,
        [(header::CONTENT_TYPE, HeaderValue::from_static("application/json; charset=utf-8"))],
        body,
    )
        .into_response()
}
