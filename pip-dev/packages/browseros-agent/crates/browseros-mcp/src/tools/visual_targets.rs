use crate::framework::{ToolCtx, ToolExecResult, ToolResult, parse_args, text_result};
use browseros_core::{PageId, TargetId, pages::PageSession};
use futures_util::future::BoxFuture;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

const MAX_TARGETS: usize = 12;
const DESCRIPTION: &str = "Find visible canvas, image, video, SVG and role=img regions in a page without capturing them. Returns viewport-relative CSS-pixel bounds and an observationId bound to this page, target, CDP connection incarnation, and the observed visual element identities; screenshot.region only accepts crops contained within one of these returned regions. The crop is rejected if the URL, viewport, scroll position, visual target identity, or bounds changed.";

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
struct EvaluatedVisualTargets {
    layout_hash: String,
    regions: Value,
    #[serde(default)]
    action_point_valid: Vec<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ObservationBinding {
    page_id: PageId,
    target_id: TargetId,
    connection_epoch: u64,
}

#[derive(Debug, PartialEq, Eq)]
struct ParsedObservationId {
    max_targets: usize,
    binding: ObservationBinding,
    layout_hash: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VisualTargetSnapshot {
    pub observation_id: String,
    pub regions: Value,
    pub action_point_valid: Vec<bool>,
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
        let binding = observation_binding(PageId(args.page), &page);
        let snapshot = match evaluate_snapshot(&page.session, &binding, max_targets, &[]).await {
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
    page_id: PageId,
    page: &PageSession,
    observation_id: &str,
) -> ToolExecResult<VisualTargetSnapshot> {
    current_observation_at_points(page_id, page, observation_id, &[]).await
}

/// Revalidates a visual observation and checks coordinate hit targets in the
/// same Runtime.evaluate call, avoiding an extra CDP round trip before input.
pub async fn current_observation_at_points(
    page_id: PageId,
    page: &PageSession,
    observation_id: &str,
    action_points: &[(f64, f64)],
) -> ToolExecResult<VisualTargetSnapshot> {
    let parsed = parse_observation_id(observation_id).ok_or_else(invalid_observation_error)?;
    let binding = observation_binding(page_id, page);
    if parsed.binding != binding {
        return Err(crate::framework::ToolError::message(
            "observationId belongs to a different page target or CDP connection; call visual_targets again and use its returned token".to_string(),
        ));
    }
    evaluate_snapshot(&page.session, &binding, parsed.max_targets, action_points).await
}

fn invalid_observation_error() -> crate::framework::ToolError {
    crate::framework::ToolError::message(
        "invalid observationId; call visual_targets again and use a returned token".to_string(),
    )
}

fn observation_binding(page_id: PageId, page: &PageSession) -> ObservationBinding {
    ObservationBinding {
        page_id,
        target_id: page.target_id.clone(),
        connection_epoch: page.session.connection_epoch(),
    }
}

fn parse_observation_id(observation_id: &str) -> Option<ParsedObservationId> {
    let mut parts = observation_id.split(':');
    if parts.next()? != "v2" {
        return None;
    }
    let max_targets = parts.next()?.parse::<usize>().ok()?;
    let page_id = PageId(parts.next()?.parse::<u32>().ok()?);
    let connection_epoch = parts.next()?.parse::<u64>().ok()?;
    let target_id = decode_hex(parts.next()?)?;
    let layout_hash = parts.next()?.to_ascii_lowercase();
    if parts.next().is_some()
        || !(1..=MAX_TARGETS).contains(&max_targets)
        || target_id.is_empty()
        || layout_hash.len() != 8
        || !layout_hash.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return None;
    }
    Some(ParsedObservationId {
        max_targets,
        binding: ObservationBinding {
            page_id,
            target_id: TargetId(target_id),
            connection_epoch,
        },
        layout_hash,
    })
}

fn encode_hex(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len() * 2);
    for byte in value.as_bytes() {
        use std::fmt::Write as _;
        let _ = write!(encoded, "{byte:02x}");
    }
    encoded
}

fn decode_hex(encoded: &str) -> Option<String> {
    if encoded.is_empty() || encoded.len() % 2 != 0 {
        return None;
    }
    let bytes = encoded
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let high = (pair[0] as char).to_digit(16)?;
            let low = (pair[1] as char).to_digit(16)?;
            Some(((high << 4) | low) as u8)
        })
        .collect::<Option<Vec<_>>>()?;
    String::from_utf8(bytes).ok()
}

fn observation_id_for(
    max_targets: usize,
    binding: &ObservationBinding,
    layout_hash: &str,
) -> String {
    format!(
        "v2:{max_targets}:{}:{}:{}:{}",
        binding.page_id.0,
        binding.connection_epoch,
        encode_hex(binding.target_id.as_str()),
        layout_hash.to_ascii_lowercase()
    )
}

async fn evaluate_snapshot(
    session: &browseros_core::ProtocolSession,
    binding: &ObservationBinding,
    max_targets: usize,
    action_points: &[(f64, f64)],
) -> ToolExecResult<VisualTargetSnapshot> {
    let action_points_json = serde_json::to_string(action_points).map_err(|error| {
        crate::framework::ToolError::message(format!("could not encode action points: {error}"))
    })?;
    let expression = format!(
        r#"(() => {{
          const maxTargets = {max_targets};
          const actionPoints = {action_points_json};
          const identityKey = Symbol.for('browseros.visualTargets.identityStore');
          let identityStore = window[identityKey];
          if (!identityStore || !(identityStore.ids instanceof WeakMap)) {{
            identityStore = {{ ids: new WeakMap(), next: 1, nonce: Math.random().toString(36).slice(2) }};
            Object.defineProperty(window, identityKey, {{ value: identityStore }});
          }}
          const identityFor = (element) => {{
            let identity = identityStore.ids.get(element);
            if (!identity) {{
              identity = identityStore.nonce + ':' + identityStore.next++;
              identityStore.ids.set(element, identity);
            }}
            return identity;
          }};
          const selectors = 'canvas,img,video,svg,[role="img"]';
          const viewportWidth = window.innerWidth || 0;
          const viewportHeight = window.innerHeight || 0;
          const targets = Array.from(document.querySelectorAll(selectors)).flatMap((element, index) => {{
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
            return [{{ element, region: {{
              index,
              type: tag,
              label: element.getAttribute('aria-label') || element.getAttribute('alt') || element.getAttribute('title') || '',
              x,
              y,
              width: safeRight - x,
              height: safeBottom - y,
              contentWidth: naturalWidth,
              contentHeight: naturalHeight,
              identity: identityFor(element)
            }} }}];
          }}).slice(0, 64).slice(0, maxTargets);
          const regions = targets.map((target) => {{
            const region = Object.assign({{}}, target.region);
            delete region.identity;
            return region;
          }});
          const actionPointValid = actionPoints.map(([x, y]) => {{
            const overlapping = targets.filter((target) =>
              x >= target.region.x && y >= target.region.y &&
              x < target.region.x + target.region.width &&
              y < target.region.y + target.region.height
            );
            if (overlapping.length === 0) return false;
            const hit = document.elementFromPoint(x, y);
            return !!hit && overlapping.some((target) =>
              target.element === hit || (
                target.element.namespaceURI === 'http://www.w3.org/2000/svg' &&
                hit.namespaceURI === 'http://www.w3.org/2000/svg' &&
                target.element.contains(hit)
              )
            );
          }});
          const state = JSON.stringify({{
            url: location.href,
            viewport: [viewportWidth, viewportHeight, window.scrollX || 0, window.scrollY || 0, window.devicePixelRatio || 1],
            maxTargets,
            regions: targets.map((target) => target.region)
          }});
          let hash = 2166136261;
          for (let index = 0; index < state.length; index++) {{
            hash ^= state.charCodeAt(index);
            hash = Math.imul(hash, 16777619);
          }}
          return {{ layoutHash: (hash >>> 0).toString(16).padStart(8, '0'), regions, actionPointValid }};
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
    if session.connection_epoch() != binding.connection_epoch {
        return Err(crate::framework::ToolError::message(
            "CDP connection changed during visual observation; call visual_targets again"
                .to_string(),
        ));
    }
    if let Some(exception) = result.exception_details {
        return Err(crate::framework::ToolError::message(exception.text));
    }
    let evaluated: EvaluatedVisualTargets =
        serde_json::from_value(result.result.value.take().unwrap_or_else(|| json!({})))
            .map_err(|error| crate::framework::ToolError::message(error.to_string()))?;
    if evaluated.layout_hash.len() != 8
        || !evaluated
            .layout_hash
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(crate::framework::ToolError::message(
            "page returned an invalid visual layout fingerprint".to_string(),
        ));
    }
    Ok(VisualTargetSnapshot {
        observation_id: observation_id_for(max_targets, binding, &evaluated.layout_hash),
        regions: evaluated.regions,
        action_point_valid: evaluated.action_point_valid,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_limit_is_read_from_a_well_formed_observation_id() {
        let binding = ObservationBinding {
            page_id: PageId(7),
            target_id: TargetId("target-7".to_string()),
            connection_epoch: 3,
        };
        let token = observation_id_for(12, &binding, "ABCDEF09");
        assert_eq!(parse_observation_id(&token).unwrap().max_targets, 12);
        assert_eq!(parse_observation_id(&token).unwrap().binding, binding);
        assert_eq!(
            parse_observation_id(&token).unwrap().layout_hash,
            "abcdef09"
        );
        assert!(parse_observation_id("0123abcd").is_none());
        assert!(parse_observation_id("v1:12:ABCDEF09").is_none());
        assert!(parse_observation_id("v2:13:7:3:7461726765742d37:0123abcd").is_none());
        assert!(parse_observation_id("v2:1:7:3:xyz:0123abcd").is_none());
    }

    #[test]
    fn identical_layout_observations_are_distinct_across_page_incarnations() {
        let first = ObservationBinding {
            page_id: PageId(1),
            target_id: TargetId("same-url-target-a".to_string()),
            connection_epoch: 9,
        };
        let second = ObservationBinding {
            page_id: PageId(2),
            target_id: TargetId("same-url-target-b".to_string()),
            connection_epoch: 9,
        };
        let first_token = observation_id_for(12, &first, "0123abcd");
        let second_token = observation_id_for(12, &second, "0123abcd");

        assert_ne!(first_token, second_token);
        assert_eq!(
            parse_observation_id(&first_token).unwrap().layout_hash,
            parse_observation_id(&second_token).unwrap().layout_hash
        );
        assert_ne!(
            parse_observation_id(&first_token).unwrap().binding,
            parse_observation_id(&second_token).unwrap().binding
        );
    }

    #[test]
    fn observation_id_binds_page_target_and_connection_epoch() {
        let binding = ObservationBinding {
            page_id: PageId(8),
            target_id: TargetId("target-exact".to_string()),
            connection_epoch: 12,
        };
        let token = observation_id_for(4, &binding, "1234abcd");
        let parsed = parse_observation_id(&token).unwrap();

        assert_eq!(parsed.binding, binding);
        assert_ne!(
            parsed.binding,
            ObservationBinding {
                page_id: PageId(8),
                target_id: TargetId("target-other".to_string()),
                connection_epoch: 12,
            }
        );
        assert_ne!(
            parsed.binding,
            ObservationBinding {
                page_id: PageId(8),
                target_id: TargetId("target-exact".to_string()),
                connection_epoch: 13,
            }
        );
    }
}
