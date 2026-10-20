use crate::framework::{ToolCtx, ToolExecResult, ToolResult, parse_args, text_result};
use browseros_core::PageId;
use futures_util::future::BoxFuture;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

const MAX_TARGETS: usize = 12;
const DESCRIPTION: &str = "Find visible canvas, image, video, SVG and role=img regions in a page without capturing them. Returns viewport-relative CSS-pixel bounds and an observationId; screenshot.region only accepts crops contained within one of these returned regions. The crop is rejected if the URL, viewport, scroll position, or visual target bounds changed.";

#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct VisualTargetsArgs {
    /// Page id from `tabs` or `navigate`.
    page: u32,
    /// Maximum number of visible regions to return (1-12).
    max_targets: Option<u32>,
}

#[derive(Debug, Deserialize)]
struct EvaluateResult {
    result: RemoteObject,
    exception_details: Option<ExceptionDetails>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VisualTargetSnapshot {
    pub observation_id: String,
    pub regions: Value,
}

/// Structured MCP contract for a fresh set of visible visual targets.
///
/// The text content remains the legacy JSON array of regions. The declared
/// output schema allows MCP hosts to retain the structured observation token
/// even when they suppress structured results for tools without a schema.
#[allow(dead_code)] // Fields are consumed by schemars when advertising the MCP contract.
#[derive(JsonSchema)]
#[serde(rename_all = "camelCase")]
struct VisualTargetsOutput {
    page: u32,
    coordinate_space: String,
    observation_id: String,
    regions: Vec<VisualTargetRegionOutput>,
    count: usize,
}

#[allow(dead_code)] // Fields are consumed by schemars when advertising the MCP contract.
#[derive(JsonSchema)]
#[serde(rename_all = "camelCase")]
struct VisualTargetRegionOutput {
    index: usize,
    #[serde(rename = "type")]
    element_type: String,
    label: String,
    x: i64,
    y: i64,
    width: i64,
    height: i64,
    content_width: f64,
    content_height: f64,
}

#[derive(Debug, Deserialize)]
struct RemoteObject {
    value: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct ExceptionDetails {
    text: String,
}

pub fn definition() -> crate::framework::ToolDef {
    super::def_with_output::<VisualTargetsArgs, VisualTargetsOutput>(
        "visual_targets",
        DESCRIPTION,
        Some(super::read_only_annotations()),
        handler,
    )
}

fn handler<'a>(
    raw: Value,
    ctx: &'a ToolCtx,
    _response: &'a mut crate::response::ToolResponse,
) -> BoxFuture<'a, ToolExecResult<Option<ToolResult>>> {
    Box::pin(async move {
        let args: VisualTargetsArgs = parse_args(raw)?;
        let max_targets = args
            .max_targets
            .unwrap_or(MAX_TARGETS as u32)
            .clamp(1, MAX_TARGETS as u32) as usize;
        let page = ctx.browser.page_session(PageId(args.page)).await?;
        let snapshot = match evaluate_snapshot(&page.session, max_targets).await {
            Ok(snapshot) => snapshot,
            Err(error) => {
                return Ok(Some(crate::framework::error_result(format!(
                    "visual_targets: {error}"
                ))));
            }
        };
        let regions = snapshot.regions;
        let count = regions.as_array().map_or(0, Vec::len);
        let text = serde_json::to_string_pretty(&regions).unwrap_or_else(|_| "[]".to_string());
        let mut structured = json!({
            "page": args.page,
            "coordinateSpace": "visible-viewport-css-pixels",
            "observationId": snapshot.observation_id,
            "regions": regions
        });
        if let Value::Object(object) = &mut structured {
            object.insert("count".to_string(), json!(count));
        }
        Ok(Some(text_result(text, Some(structured))))
    })
}

pub async fn current_observation(
    session: &browseros_core::ProtocolSession,
    observation_id: &str,
) -> ToolExecResult<VisualTargetSnapshot> {
    let max_targets = parse_target_limit(observation_id).ok_or_else(|| {
        crate::framework::ToolError::message(
            "invalid observationId; call visual_targets again and use a returned token".to_string(),
        )
    })?;
    evaluate_snapshot(session, max_targets).await
}

fn parse_target_limit(observation_id: &str) -> Option<usize> {
    let mut parts = observation_id.split(':');
    if parts.next()? != "v1" {
        return None;
    }
    let max_targets = parts.next()?.parse::<usize>().ok()?;
    let hash = parts.next()?;
    if parts.next().is_some()
        || !(1..=MAX_TARGETS).contains(&max_targets)
        || hash.len() != 8
        || !hash.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return None;
    }
    Some(max_targets)
}

async fn evaluate_snapshot(
    session: &browseros_core::ProtocolSession,
    max_targets: usize,
) -> ToolExecResult<VisualTargetSnapshot> {
    let expression = format!(
        r#"(() => {{
          const maxTargets = {max_targets};
          const selectors = 'canvas,img,video,svg,[role="img"]';
          const viewportWidth = window.innerWidth || 0;
          const viewportHeight = window.innerHeight || 0;
          const regions = Array.from(document.querySelectorAll(selectors)).flatMap((element, index) => {{
            const rect = element.getBoundingClientRect();
            const style = getComputedStyle(element);
            const left = Math.max(0, rect.left);
            const top = Math.max(0, rect.top);
            const right = Math.min(viewportWidth, rect.right);
            const bottom = Math.min(viewportHeight, rect.bottom);
            const x = Math.ceil(left);
            const y = Math.ceil(top);
            const safeRight = Math.floor(right);
            const safeBottom = Math.floor(bottom);
            if (style.display === 'none' || style.visibility === 'hidden' || Number(style.opacity) === 0 || safeRight - x < 12 || safeBottom - y < 12) return [];
            const tag = element.tagName.toLowerCase();
            const naturalWidth = element.naturalWidth || element.videoWidth || element.width || 0;
            const naturalHeight = element.naturalHeight || element.videoHeight || element.height || 0;
            return [{{
              index,
              type: tag,
              label: element.getAttribute('aria-label') || element.getAttribute('alt') || element.getAttribute('title') || '',
              x,
              y,
              width: safeRight - x,
              height: safeBottom - y,
              contentWidth: naturalWidth,
              contentHeight: naturalHeight
            }}];
          }}).slice(0, 64).slice(0, maxTargets);
          const state = JSON.stringify({{
            url: location.href,
            viewport: [viewportWidth, viewportHeight, window.scrollX || 0, window.scrollY || 0, window.devicePixelRatio || 1],
            maxTargets,
            regions
          }});
          let hash = 2166136261;
          for (let index = 0; index < state.length; index++) {{
            hash ^= state.charCodeAt(index);
            hash = Math.imul(hash, 16777619);
          }}
          return {{ observationId: `v1:${{maxTargets}}:${{(hash >>> 0).toString(16).padStart(8, '0')}}`, regions }};
        }})()"#
    );
    let mut result: EvaluateResult = session
        .send(
            "Runtime.evaluate",
            json!({
                "expression": expression,
                "returnByValue": true,
                "awaitPromise": false
            }),
        )
        .await?;
    if let Some(exception) = result.exception_details {
        return Err(crate::framework::ToolError::message(exception.text));
    }
    serde_json::from_value(result.result.value.take().unwrap_or_else(|| json!({})))
        .map_err(|error| crate::framework::ToolError::message(error.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_limit_is_read_from_a_well_formed_observation_id() {
        assert_eq!(parse_target_limit("v1:1:0123abcd"), Some(1));
        assert_eq!(parse_target_limit("v1:12:ABCDEF09"), Some(12));
        assert_eq!(parse_target_limit("0123abcd"), None);
        assert_eq!(parse_target_limit("v1:13:0123abcd"), None);
        assert_eq!(parse_target_limit("v1:1:xyz"), None);
    }
}
