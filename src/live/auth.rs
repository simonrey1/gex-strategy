use axum::{
    body::Body,
    http::{Request, Response, StatusCode, header},
    middleware::Next,
};
use base64::Engine as _;

/// Expected value: `"user:password"`.
static AUTH_CRED: std::sync::OnceLock<String> = std::sync::OnceLock::new();

pub fn set_basic_auth(cred: String) {
    AUTH_CRED.set(cred).expect("set_basic_auth called twice");
}

pub async fn basic_auth_middleware(req: Request<Body>, next: Next) -> Response<Body> {
    let expected = match AUTH_CRED.get() {
        Some(c) => c,
        None => return next.run(req).await,
    };

    if let Some(val) = req.headers().get(header::AUTHORIZATION) {
        if let Ok(s) = val.to_str() {
            if let Some(encoded) = s.strip_prefix("Basic ") {
                if let Ok(decoded) = base64::engine::general_purpose::STANDARD.decode(encoded) {
                    if let Ok(pair) = std::str::from_utf8(&decoded) {
                        if pair == expected.as_str() {
                            return next.run(req).await;
                        }
                    }
                }
            }
        }
    }

    Response::builder()
        .status(StatusCode::UNAUTHORIZED)
        .header(header::WWW_AUTHENTICATE, "Basic realm=\"gex\"")
        .body(Body::from("Unauthorized"))
        .unwrap()
}

/// Server config passed from CLI.
#[derive(Clone, Default)]
pub struct ServerConfig {
    pub port: u16,
    pub tls_cert: Option<String>,
    pub tls_key: Option<String>,
}

impl ServerConfig {
    pub fn has_tls(&self) -> bool {
        self.tls_cert.is_some() && self.tls_key.is_some()
    }
}
