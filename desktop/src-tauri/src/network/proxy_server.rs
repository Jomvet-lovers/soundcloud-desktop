use std::net::SocketAddr;

use warp::Filter;
use warp::hyper::Body;

use crate::network::image_cache;
use crate::network::proxy::{cache_control_for, proxy_request};
use crate::network::server::cors;
use crate::network::upload_grants;

fn upload_response(status: u16) -> warp::http::response::Builder {
    let mut builder = warp::http::Response::builder().status(status);
    for (name, value) in upload_grants::cors_headers() {
        builder = builder.header(name, value);
    }
    builder
}

fn upload_preflight_response() -> warp::http::Response<Body> {
    let mut builder = warp::http::Response::builder().status(204);
    for (name, value) in upload_grants::preflight_cors_headers() {
        builder = builder.header(name, value);
    }
    builder.body(Body::empty()).unwrap()
}

fn routes() -> impl Filter<Extract = impl warp::Reply, Error = warp::Rejection> + Clone {
    // Handled outside the shared CORS wrapper: warp's preflight short-circuit
    // does not know about Private Network Access and would return 200 without
    // the opt-in header.
    let upload_options = warp::path("up")
        .and(warp::path::tail())
        .and(warp::options())
        .map(|_tail: warp::path::Tail| upload_preflight_response());

    let upload_route =
        warp::path("up")
            .and(warp::path::tail())
            .and_then(|tail: warp::path::Tail| async move {
                let token = tail.as_str();
                match upload_grants::read_grant(token).await {
                    Ok((data, grant)) => Ok::<_, warp::Rejection>(
                        upload_response(200)
                            .header("Content-Type", &grant.content_type)
                            .header("Content-Length", data.len())
                            .header("Cache-Control", "no-store")
                            .body(Body::from(data))
                            .unwrap(),
                    ),
                    Err(error) => Ok::<_, warp::Rejection>(
                        upload_response(error.status())
                            .header("Content-Type", "text/plain")
                            .header("Cache-Control", "no-store")
                            .body(Body::from(error.message()))
                            .unwrap(),
                    ),
                }
            });

    let image_route =
        warp::path("img")
            .and(warp::path::tail())
            .and_then(|tail: warp::path::Tail| async move {
                let encoded = tail.as_str();
                let result = image_cache::handle(encoded).await;
                Ok::<_, warp::Rejection>(
                    warp::http::Response::builder()
                        .status(result.status)
                        .header("Content-Type", &result.content_type)
                        .header("Cache-Control", cache_control_for(result.status))
                        .header("Access-Control-Allow-Origin", "*")
                        .body(Body::from(result.data))
                        .unwrap(),
                )
            });

    let proxy_route =
        warp::path("p")
            .and(warp::path::tail())
            .and_then(|tail: warp::path::Tail| async move {
                let encoded_url = tail.as_str();
                let result = proxy_request(encoded_url).await;
                Ok::<_, warp::Rejection>(
                    warp::http::Response::builder()
                        .status(result.status)
                        .header("Content-Type", &result.content_type)
                        .header("Cache-Control", cache_control_for(result.status))
                        .header("Access-Control-Allow-Origin", "*")
                        .body(Body::from(result.data))
                        .unwrap(),
                )
            });

    // `/img` and `/p` keep the existing shared CORS behavior untouched.
    let legacy_routes = image_route.or(proxy_route).with(cors());

    upload_options.or(upload_route).or(legacy_routes)
}

pub async fn start() -> u16 {
    let addr: SocketAddr = ([127, 0, 0, 1], 0).into();
    let (addr, server) = warp::serve(routes()).bind_ephemeral(addr);
    tokio::spawn(server);

    println!("[ProxyServer] http://127.0.0.1:{}", addr.port());
    addr.port()
}

#[cfg(test)]
mod tests {
    use super::routes;
    use crate::network::upload_grants;
    use std::path::PathBuf;

    const UNKNOWN_TOKEN: &str = "00000000-0000-4000-8000-000000000000";

    fn temp_file(contents: &[u8]) -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("scd-proxy-server-{}.bin", uuid::Uuid::new_v4()));
        std::fs::write(&path, contents).unwrap();
        path
    }

    #[tokio::test]
    async fn upload_preflight_returns_204_with_private_network_headers() {
        let response = warp::test::request()
            .method("OPTIONS")
            .path(&format!("/up/{UNKNOWN_TOKEN}"))
            .header("origin", "https://soundcloud.com")
            .header("access-control-request-method", "GET")
            .header("access-control-request-private-network", "true")
            .reply(&routes())
            .await;

        assert_eq!(response.status(), 204);
        assert_eq!(response.headers()["access-control-allow-origin"], "*");
        assert_eq!(
            response.headers()["access-control-allow-private-network"],
            "true"
        );
        assert_eq!(
            response.headers()["access-control-allow-methods"],
            "GET, HEAD, OPTIONS"
        );
        assert_eq!(response.headers()["access-control-allow-headers"], "*");
        assert_eq!(response.headers()["access-control-max-age"], "600");
    }

    #[tokio::test]
    async fn upload_get_returns_private_network_and_expose_headers() {
        let contents = b"one-time audio bytes";
        let path = temp_file(contents);
        let token = upload_grants::issue(
            path.clone(),
            "track.bin".into(),
            "application/octet-stream".into(),
        );

        let response = warp::test::request()
            .method("GET")
            .path(&format!("/up/{token}"))
            .reply(&routes())
            .await;

        assert_eq!(response.status(), 200);
        assert_eq!(response.body().as_ref(), contents.as_slice());
        assert_eq!(response.headers()["access-control-allow-origin"], "*");
        assert_eq!(
            response.headers()["access-control-allow-private-network"],
            "true"
        );
        assert_eq!(
            response.headers()["access-control-expose-headers"],
            "content-length"
        );
        assert_eq!(response.headers()["content-length"], "20");

        let _ = std::fs::remove_file(&path);
    }

    #[tokio::test]
    async fn upload_error_also_carries_private_network_header() {
        let response = warp::test::request()
            .method("GET")
            .path(&format!("/up/{UNKNOWN_TOKEN}"))
            .reply(&routes())
            .await;

        assert_eq!(response.status(), 404);
        assert_eq!(response.headers()["access-control-allow-origin"], "*");
        assert_eq!(
            response.headers()["access-control-allow-private-network"],
            "true"
        );
        assert_eq!(
            response.headers()["access-control-expose-headers"],
            "content-length"
        );
    }
}
