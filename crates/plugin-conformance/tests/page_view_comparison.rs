//! Current recordings compare page-view metadata without compatibility masking.
use serde_json::json;
#[test]
fn an_unrecorded_page_view_member_is_a_real_trace_difference() {
    let expected = [json!({"core":{"instanceId":"a#1","revision":0}})];
    let actual = [
        json!({"core":{"instanceId":"a#1","revision":0,"pageLabels":[],"pendingPages":{"$map":[]}}}),
    ];
    assert!(plugin_conformance::compare_trace(&expected, &actual).is_some());
}
