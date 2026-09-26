use serde_json::json;
use xcb_core::Id;

#[test]
fn pinned_xcb_parser_drops_model_scope_required_by_overseer() {
    let value = json!({"rateLimitsByLimitId": {
        "base_model_inference": {"limitId":"base_model_inference",
            "normalModelSlug":"gpt-5.6-luna",
            "primary":{"usedPercent":25,"resetsAt":1800500000}}
    }});
    let points = xcb_runtime::codex::parse_quotas(&value,
        &Id::new("test-pool").unwrap(), 1_800_000_000_000).unwrap();
    assert_eq!(points.len(), 1);
    let normalized = serde_json::to_value(&points[0]).unwrap();
    assert!(normalized.get("model").is_none(), "XCB's point has no model scope");
    assert_eq!(points[0].window.as_str(), "base_model_inference.primary");
}

#[test]
fn pinned_xcb_parser_drops_block_with_missing_reset() {
    let value = json!({"rateLimits":{"primary":{"usedPercent":100}}});
    let points = xcb_runtime::codex::parse_quotas(&value,
        &Id::new("test-pool").unwrap(), 1_800_000_000_000).unwrap();
    assert!(points.is_empty(), "Overseer retains this as a confirmed block with unknown reset");
}

#[test]
fn pinned_xcb_parser_rejects_denial_without_windows_as_unavailable() {
    let value = json!({"ordinaryUsageAllowed":false,"rateLimits":null});
    assert!(xcb_runtime::codex::parse_quotas(&value,
        &Id::new("test-pool").unwrap(), 1_800_000_000_000).is_err());
}
