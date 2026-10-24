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
        .acknowledge(
            &lease_id,
            body.generation,
            body.tab_id,
            &body.target_id,
            ack,
        )
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

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct TabStopRequest {
    target_id: String,
}

/// Accepts a Stop intent for the exact live target even when no lease exists.
/// The extension still drives lease release and ACKs before reporting completion.
pub(super) async fn stop_tab(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(tab_id): Path<i64>,
    Json(body): Json<TabStopRequest>,
) -> Result<StatusCode, (StatusCode, Json<Value>)> {
    authorized(&state, &headers)
        .map_err(|status| (status, Json(json!({"error":"unauthorized"}))))?;
    let browser = state.browser.session().await.ok_or_else(|| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error":"browser is unavailable"})),
        )
    })?;
    let pages = browser.pages.list().await.map_err(|_| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error":"browser inventory is unavailable"})),
        )
    })?;
    if !pages
        .iter()
        .any(|page| page.tab_id.0 == tab_id && page.target_id.as_str() == body.target_id.as_str())
    {
        return Err((
            StatusCode::CONFLICT,
            Json(json!({"error":"tab Stop does not match a live target incarnation"})),
        ));
    }
    state
        .agent_control
        .request_tab_stop(tab_id, &body.target_id)
        .await
        .map_err(|error| (StatusCode::CONFLICT, Json(json!({"error":error}))))?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        api::mcp::test_support::{ProfileConnection, current_session, tool_call},
        services::agent_control::AgentControlService,
    };
    use std::sync::Arc;

    #[tokio::test]
    async fn tab_stop_rejects_stale_target_before_recording_stop_intent() -> anyhow::Result<()> {
        let browser = current_session(ProfileConnection::new());
        let page = browser
            .pages
            .list()
            .await?
            .into_iter()
            .next()
            .ok_or_else(|| anyhow::anyhow!("synthetic page missing"))?;
        let mut call = tool_call("tabs", json!({})).await?;
        call.state.browser.set_session_for_testing(browser).await;
        call.state.agent_control =
            Arc::new(AgentControlService::new(Some("test-secret".to_string())));
        let mut headers = HeaderMap::new();
        headers.insert(header::AUTHORIZATION, "Bearer test-secret".parse()?);
        let stale = stop_tab(
            State(call.state.clone()),
            headers.clone(),
            Path(page.tab_id.0),
            Json(TabStopRequest {
                target_id: "old-target".to_string(),
            }),
        )
        .await;
        assert_eq!(
            stale.err().map(|(status, _)| status),
            Some(StatusCode::CONFLICT)
        );
        call.state
            .agent_control
            .begin(
                "owner",
                page.page_id.0,
                page.tab_id.0,
                page.target_id.as_str(),
                false,
            )
            .await
            .map_err(anyhow::Error::msg)?;
        let stopped = stop_tab(
            State(call.state.clone()),
            headers,
            Path(page.tab_id.0),
            Json(TabStopRequest {
                target_id: page.target_id.to_string(),
            }),
        )
        .await;
        assert_eq!(stopped.ok(), Some(StatusCode::NO_CONTENT));
        assert!(
            call.state
                .agent_control
                .begin(
                    "late-owner",
                    page.page_id.0,
                    page.tab_id.0,
                    page.target_id.as_str(),
                    false
                )
                .await
                .is_err()
        );
        Ok(())
    }
}
