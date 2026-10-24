//! Authenticated bridge between the bundled extension and MCP tab leases.
use crate::{AppState, services::agent_control::LeaseAck};
use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode, header},
};
use serde::Deserialize;
use serde_json::{Value, json};

fn authorized(state: &AppState, headers: &HeaderMap) -> Result<(), StatusCode> {
    if !state.agent_control.is_enabled() {
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    }
    let bearer = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok());
    if state.agent_control.authorized(bearer) {
        Ok(())
    } else {
        Err(StatusCode::UNAUTHORIZED)
    }
}

/// Returns exact lease incarnations and terminal states to the bundled extension.
/// The extension acts only on Locking/Unlocking; Failed remains visible for Stop/Retry.
pub(super) async fn leases(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, StatusCode> {
    authorized(&state, &headers)?;
    Ok(Json(
        json!({ "leases": state.agent_control.leases_for_extension().await }),
    ))
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct AckRequest {
    generation: u64,
    tab_id: i64,
    target_id: String,
    status: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct ReleaseRequest {
    generation: u64,
    tab_id: i64,
    target_id: String,
}

/// Acknowledges only the exact target incarnation named by the lease.
pub(super) async fn acknowledge(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(lease_id): Path<String>,
    Json(body): Json<AckRequest>,
) -> Result<StatusCode, (StatusCode, Json<Value>)> {
    authorized(&state, &headers)
        .map_err(|status| (status, Json(json!({"error":"unauthorized"}))))?;
    let ack = match body.status.as_str() {
        "locked" => LeaseAck::Locked,
        "unlocked" => LeaseAck::Unlocked,
        "unavailable" => LeaseAck::Unavailable,
        "stale" => LeaseAck::Stale,
        _ => {
            return Err((
                StatusCode::BAD_REQUEST,
                Json(json!({"error":"invalid acknowledgement"})),
            ));
        }
    };
    state
        .agent_control
        .acknowledge(&lease_id, body.generation, body.tab_id, &body.target_id, ack)
        .await
        .map_err(|error| (StatusCode::CONFLICT, Json(json!({"error":error}))))?;
    Ok(StatusCode::NO_CONTENT)
}

/// Lets the explicit extension Stop control request cleanup without borrowing an
/// MCP owner identity. The random bearer and exact tuple are both required.
pub(super) async fn release(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(lease_id): Path<String>,
    Json(body): Json<ReleaseRequest>,
) -> Result<StatusCode, (StatusCode, Json<Value>)> {
    authorized(&state, &headers)
        .map_err(|status| (status, Json(json!({"error":"unauthorized"}))))?;
    state
        .agent_control
        .request_release(&lease_id, body.generation, body.tab_id, &body.target_id)
        .await
        .map_err(|error| (StatusCode::CONFLICT, Json(json!({"error":error}))))?;
    Ok(StatusCode::NO_CONTENT)
}
