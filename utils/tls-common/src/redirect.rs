use axum::handler::HandlerWithoutStateExt;
use axum::http::{HeaderMap, StatusCode, Uri};
use axum::response::Redirect;
use axum::Router;

/// Router который 301 редиректит любой HTTP запрос на https://<host>:<https_port><path>.
pub fn redirect_router(https_port: u16) -> Router {
    let redirect = move |headers: HeaderMap, uri: Uri| async move {
        let host = headers
            .get(axum::http::header::HOST)
            .and_then(|h| h.to_str().ok())
            .unwrap_or("")
            .to_string();
        if host.is_empty() {
            return Err(StatusCode::BAD_REQUEST);
        }
        let host_no_port = host.split(':').next().unwrap_or(&host).to_string();
        let authority = if https_port == 443 {
            host_no_port
        } else {
            format!("{host_no_port}:{https_port}")
        };
        let pq = uri.path_and_query().map(|p| p.as_str()).unwrap_or("/");
        match Uri::builder()
            .scheme("https")
            .authority(authority)
            .path_and_query(pq)
            .build()
        {
            Ok(u) => Ok(Redirect::permanent(&u.to_string())),
            Err(_) => Err(StatusCode::BAD_REQUEST),
        }
    };
    Router::new().fallback_service(redirect.into_service())
}
