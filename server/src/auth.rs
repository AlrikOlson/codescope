//! OAuth discovery and transport security for MCP HTTP transport.
//!
//! - Protected Resource Metadata (RFC 9728) at `/.well-known/oauth-protected-resource/mcp`
//! - Origin header validation (DNS rebinding protection)
//! - Bearer token stub (returns 401 with WWW-Authenticate when auth is enabled)

use axum::{
    extract::State,
    http::{header, HeaderMap, HeaderValue, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};

use crate::types::McpAppContext;

// ---------------------------------------------------------------------------
// Protected Resource Metadata (RFC 9728)
// ---------------------------------------------------------------------------

/// `GET /.well-known/oauth-protected-resource/mcp`
///
/// Returns OAuth discovery metadata so clients can find the authorization server.
/// Served regardless of whether auth is enabled — an empty `authorization_servers`
/// array signals that no auth is required.
pub async fn prm_endpoint(State(ctx): State<McpAppContext>) -> impl IntoResponse {
    let auth_servers = match ctx.config.auth_issuer {
        Some(ref issuer) => serde_json::json!([issuer]),
        None => serde_json::json!([]),
    };

    let body = serde_json::json!({
        "resource": ctx.config.server_url,
        "authorization_servers": auth_servers,
    });

    ([(header::CONTENT_TYPE, "application/json")], serde_json::to_string(&body).unwrap())
}

// ---------------------------------------------------------------------------
// Origin validation middleware (DNS rebinding protection)
// ---------------------------------------------------------------------------

/// Validates the `Origin` header on incoming requests.
///
/// Per MCP 2025-11-25 spec:
/// - If `Origin` is present and not in the allowlist → 403 Forbidden
/// - If `Origin` is absent (non-browser clients) → allow
pub async fn validate_origin(
    State(ctx): State<McpAppContext>,
    headers: HeaderMap,
    request: axum::extract::Request,
    next: Next,
) -> Result<Response, StatusCode> {
    if let Some(origin) = headers.get("origin").and_then(|v| v.to_str().ok()) {
        let allowed = ctx.config.allowed_origins.iter().any(|a| {
            // Exact match (e.g. "http://localhost:8432")
            if origin == a {
                return true;
            }
            // Match "null" for non-browser/file:// contexts
            if a == "null" && origin == "null" {
                return true;
            }
            false
        });

        if !allowed {
            return Err(StatusCode::FORBIDDEN);
        }
    }

    Ok(next.run(request).await)
}

// ---------------------------------------------------------------------------
// Bearer token validation middleware (optional, when --auth-issuer is set)
// ---------------------------------------------------------------------------

/// When auth is enabled (`--auth-issuer`), requires a valid `Authorization: Bearer` header.
/// Returns 401 with `WWW-Authenticate` pointing to the PRM endpoint.
///
/// Full JWT signature validation is deferred — this currently accepts any bearer token.
#[allow(dead_code)]
pub async fn validate_bearer(
    State(ctx): State<McpAppContext>,
    headers: HeaderMap,
    request: axum::extract::Request,
    next: Next,
) -> Result<Response, Response> {
    if !ctx.config.auth_enabled() {
        return Ok(next.run(request).await);
    }

    let has_bearer = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.starts_with("Bearer "));

    if has_bearer {
        // TODO: Validate JWT signature against auth_issuer's JWKS endpoint
        return Ok(next.run(request).await);
    }

    // 401 with WWW-Authenticate header pointing to PRM
    let prm_url = format!("{}/.well-known/oauth-protected-resource/mcp", ctx.config.server_url);
    let www_auth = format!("Bearer resource_metadata=\"{prm_url}\"");

    let mut response = StatusCode::UNAUTHORIZED.into_response();
    if let Ok(val) = HeaderValue::from_str(&www_auth) {
        response.headers_mut().insert("www-authenticate", val);
    }
    Err(response)
}
