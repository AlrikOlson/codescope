//! Streamable HTTP transport for the MCP protocol (MCP 2025-11-25).
//!
//! Provides `POST /mcp` for JSON-RPC request/response, `DELETE /mcp` for session
//! termination, and `GET /mcp` (returns 405 — no server-push notifications).
//!
//! Session management via `Mcp-Session-Id` header. Protocol version validated
//! via `Mcp-Protocol-Version` header after initialization.

use axum::{
    body::Body,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::Response,
};
use std::time::Instant;
use uuid::Uuid;

use crate::mcp::{dispatch_jsonrpc, negotiate_version};
use crate::types::*;

const SESSION_HEADER: &str = "mcp-session-id";
const PROTOCOL_VERSION_HEADER: &str = "mcp-protocol-version";

// ---------------------------------------------------------------------------
// POST /mcp — JSON-RPC dispatch with session management
// ---------------------------------------------------------------------------

/// Streamable HTTP MCP transport endpoint.
///
/// Handles single JSON-RPC requests and batches (arrays). Creates sessions on
/// `initialize`, validates session ID on all other requests.
pub async fn handle_mcp_post(
    State(ctx): State<McpAppContext>,
    headers: HeaderMap,
    body: String,
) -> Result<Response, Response> {
    // Parse JSON body
    let parsed: serde_json::Value = match serde_json::from_str(&body) {
        Ok(v) => v,
        Err(_) => {
            let err = serde_json::json!({
                "jsonrpc": "2.0",
                "id": null,
                "error": { "code": -32700, "message": "Parse error" }
            });
            return Ok(json_response(StatusCode::BAD_REQUEST, &err));
        }
    };

    let is_batch = parsed.is_array();
    let requests: Vec<serde_json::Value> =
        if is_batch { parsed.as_array().unwrap().clone() } else { vec![parsed] };

    // Check if any request is an initialize
    let has_initialize = requests.iter().any(|r| r["method"].as_str() == Some("initialize"));

    // Session validation for non-initialize requests
    let session_id =
        headers.get(SESSION_HEADER).and_then(|v| v.to_str().ok()).map(|s| s.to_string());

    if !has_initialize {
        let sid = match session_id.as_ref() {
            Some(s) if ctx.sessions.contains_key(s) => s.clone(),
            Some(_) => {
                return Err(error_response(
                    StatusCode::BAD_REQUEST,
                    "Invalid or expired session ID",
                ));
            }
            None => {
                return Err(error_response(
                    StatusCode::BAD_REQUEST,
                    "Missing Mcp-Session-Id header. Send 'initialize' first.",
                ));
            }
        };

        // Validate MCP-Protocol-Version header
        if let Some(pv) = headers.get(PROTOCOL_VERSION_HEADER).and_then(|v| v.to_str().ok()) {
            if let Some(session) = ctx.sessions.get(&sid) {
                if pv != session.protocol_version {
                    return Err(error_response(
                        StatusCode::BAD_REQUEST,
                        &format!(
                            "Protocol version mismatch: header '{}' != negotiated '{}'",
                            pv, session.protocol_version
                        ),
                    ));
                }
            }
        }
    }

    // Process requests
    let mut responses: Vec<serde_json::Value> = Vec::new();
    let mut new_session_id: Option<String> = None;

    for req in &requests {
        let method = req["method"].as_str().unwrap_or("");

        if method == "initialize" {
            // Version negotiation
            let client_version = req["params"]["protocolVersion"].as_str().unwrap_or("");
            let negotiated = negotiate_version(client_version);

            // Create session
            let sid = Uuid::new_v4().to_string();
            let session = McpSession::new(negotiated.to_string());
            ctx.sessions.insert(sid.clone(), session);
            new_session_id = Some(sid);

            // Build response via dispatch (reuses the same logic)
            if let Some(resp) = dispatch_jsonrpc(&ctx.state, req, &mut None) {
                responses.push(resp);
            }
        } else if method.starts_with("notifications/") {
            // Notifications produce no response, but update session activity
            if let Some(ref sid) = session_id {
                if let Some(mut s) = ctx.sessions.get_mut(sid) {
                    s.last_activity = Instant::now();
                }
            }
        } else {
            // Regular request — dispatch with session state
            let sid = session_id.as_ref().or(new_session_id.as_ref()).unwrap();

            // Extract session state, dispatch, put it back
            let mut sess_state = ctx.sessions.get_mut(sid).map(|mut s| {
                s.last_activity = Instant::now();
                std::mem::replace(&mut s.session_state, SessionState::new())
            });
            let mut sess_opt = sess_state.take().map(Some).unwrap_or(None);

            if let Some(resp) = dispatch_jsonrpc(&ctx.state, req, &mut sess_opt) {
                responses.push(resp);
            }

            // Restore session state
            if let Some(sess) = sess_opt {
                if let Some(mut s) = ctx.sessions.get_mut(sid) {
                    s.session_state = sess;
                }
            }
        }
    }

    // Build HTTP response
    if responses.is_empty() {
        // All notifications — 202 Accepted
        return Ok(Response::builder().status(StatusCode::ACCEPTED).body(Body::empty()).unwrap());
    }

    let body_json = if is_batch {
        serde_json::to_string(&responses).unwrap()
    } else {
        serde_json::to_string(&responses[0]).unwrap()
    };

    let mut builder =
        Response::builder().status(StatusCode::OK).header("content-type", "application/json");

    if let Some(ref sid) = new_session_id {
        builder = builder.header(SESSION_HEADER, sid);
    }

    Ok(builder.body(Body::from(body_json)).unwrap())
}

// ---------------------------------------------------------------------------
// DELETE /mcp — Session termination
// ---------------------------------------------------------------------------

pub async fn handle_mcp_delete(State(ctx): State<McpAppContext>, headers: HeaderMap) -> StatusCode {
    if let Some(sid) = headers.get(SESSION_HEADER).and_then(|v| v.to_str().ok()) {
        ctx.sessions.remove(sid);
    }
    StatusCode::OK
}

// ---------------------------------------------------------------------------
// GET /mcp — Not supported (no server-push notifications)
// ---------------------------------------------------------------------------

pub async fn handle_mcp_get() -> StatusCode {
    StatusCode::METHOD_NOT_ALLOWED
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn json_response(status: StatusCode, body: &serde_json::Value) -> Response {
    Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .body(Body::from(serde_json::to_string(body).unwrap()))
        .unwrap()
}

fn error_response(status: StatusCode, message: &str) -> Response {
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": null,
        "error": { "code": -32600, "message": message }
    });
    json_response(status, &body)
}
