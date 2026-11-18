use axum::{
    extract::{Request, State},
    http::{header, HeaderMap, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use subtle::ConstantTimeEq;

#[derive(Debug, Clone)]
pub struct SecurityState {
    pub port: u16,
    pub token: String,
}

pub fn constant_time_eq(a: &str, b: &str) -> bool {
    a.as_bytes().ct_eq(b.as_bytes()).into()
}

pub fn extract_token(headers: &HeaderMap) -> Option<&str> {
    if let Some(custom) = headers.get("X-DeskPulse-Token").and_then(|v| v.to_str().ok()) {
        let trimmed = custom.trim();
        if !trimmed.is_empty() {
            return Some(trimmed);
        }
    }
    if let Some(auth) = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
    {
        if let Some(rest) = auth.strip_prefix("Bearer ").or_else(|| auth.strip_prefix("bearer ")) {
            let trimmed = rest.trim();
            if !trimmed.is_empty() {
                return Some(trimmed);
            }
        }
    }
    None
}

fn is_allowed_host(host: &str, port: u16) -> bool {
    host.eq_ignore_ascii_case(&format!("127.0.0.1:{port}"))
        || host.eq_ignore_ascii_case(&format!("localhost:{port}"))
}

fn is_allowed_origin(origin: &str, port: u16) -> bool {
    origin.eq_ignore_ascii_case(&format!("http://127.0.0.1:{port}"))
        || origin.eq_ignore_ascii_case(&format!("http://localhost:{port}"))
}

pub async fn guard_middleware(
    State(state): State<SecurityState>,
    req: Request,
    next: Next,
) -> Response {
    let headers = req.headers();

    let Some(host) = headers.get(header::HOST).and_then(|v| v.to_str().ok()) else {
        return (StatusCode::FORBIDDEN, "Missing Host").into_response();
    };
    if !is_allowed_host(host.trim(), state.port) {
        return (StatusCode::FORBIDDEN, "Forbidden Host").into_response();
    }

    if let Some(origin) = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()) {
        let trimmed = origin.trim();
        if !trimmed.is_empty() && !is_allowed_origin(trimmed, state.port) {
            return (StatusCode::FORBIDDEN, "Forbidden Origin").into_response();
        }
    }

    if let Some(sec_fetch) = headers.get("Sec-Fetch-Site").and_then(|v| v.to_str().ok()) {
        let site = sec_fetch.trim();
        if site.eq_ignore_ascii_case("cross-site") || site.eq_ignore_ascii_case("same-site") {
            return (StatusCode::FORBIDDEN, "Forbidden Sec-Fetch-Site").into_response();
        }
    }

    if req.uri().path().starts_with("/api/") {
        let valid = extract_token(headers)
            .map(|tok| constant_time_eq(tok, &state.token))
            .unwrap_or(false);
        if !valid {
            return (StatusCode::UNAUTHORIZED, "Unauthorized").into_response();
        }
    }

    next.run(req).await.into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::Body,
        http::{Request as HttpRequest, StatusCode},
        middleware,
        routing::get,
        Router,
    };
    use tower::ServiceExt;

    fn test_app() -> Router {
        let state = SecurityState {
            port: 8765,
            token: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".to_string(),
        };
        Router::new()
            .route("/", get(|| async { StatusCode::OK }))
            .route("/api/state", get(|| async { StatusCode::OK }))
            .layer(middleware::from_fn_with_state(state, guard_middleware))
    }

    #[tokio::test]
    async fn test_allows_root_html_without_token_on_localhost() {
        let resp = test_app()
            .oneshot(
                HttpRequest::builder()
                    .uri("/")
                    .header("Host", "127.0.0.1:8765")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn test_blocks_dns_rebinding_host() {
        let resp = test_app()
            .oneshot(
                HttpRequest::builder()
                    .uri("/")
                    .header("Host", "evil.com:8765")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_blocks_cross_site_sec_fetch_site() {
        let resp = test_app()
            .oneshot(
                HttpRequest::builder()
                    .uri("/")
                    .header("Host", "127.0.0.1:8765")
                    .header("Sec-Fetch-Site", "cross-site")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_blocks_foreign_origin() {
        let resp = test_app()
            .oneshot(
                HttpRequest::builder()
                    .uri("/api/state")
                    .header("Host", "127.0.0.1:8765")
                    .header("Origin", "https://evil.com")
                    .header("X-DeskPulse-Token", "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn test_rejects_api_without_token_or_with_query_token() {
        let resp = test_app()
            .oneshot(
                HttpRequest::builder()
                    .uri("/api/state?token=0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef")
                    .header("Host", "127.0.0.1:8765")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn test_accepts_bearer_and_custom_header_token() {
        let tok = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

        let bearer_resp = test_app()
            .oneshot(
                HttpRequest::builder()
                    .uri("/api/state")
                    .header("Host", "localhost:8765")
                    .header("Authorization", format!("Bearer {tok}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(bearer_resp.status(), StatusCode::OK);

        let custom_resp = test_app()
            .oneshot(
                HttpRequest::builder()
                    .uri("/api/state")
                    .header("Host", "127.0.0.1:8765")
                    .header("X-DeskPulse-Token", tok)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(custom_resp.status(), StatusCode::OK);
    }
}
