use epdf_ffi::document::{DocumentEventOrigin, DocumentEventOriginKind};
use serde_json::json;
#[test]
fn remote_transaction_and_undo_origin_are_preserved() {
    let origin=DocumentEventOrigin::from_json(&json!({"kind":"remote","sessionId":"session","sub":"author","ts":1234.5,"serverId":42,"tx":{"id":"transaction","index":1,"count":3},"undoOf":"operation"})).unwrap();
    assert_eq!(origin.kind, DocumentEventOriginKind::Remote);
    assert_eq!(origin.subject.as_deref(), Some("author"));
    assert_eq!(origin.timestamp_ms, 1234.5);
    assert_eq!(origin.server_id, Some(42.0));
    let transaction = origin.transaction.unwrap();
    assert_eq!(
        (transaction.id.as_str(), transaction.index, transaction.count),
        ("transaction", 1, 3)
    );
    assert_eq!(origin.undo_of.as_deref(), Some("operation"));
    assert!(DocumentEventOrigin::from_json(&json!({"kind":"guessed"})).is_err());
}
