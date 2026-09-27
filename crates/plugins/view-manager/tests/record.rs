//! `JsRecord::keys`, which the plugin itself never calls: the TypeScript
//! reads `Object.values(panes)` only, so no vector reaches `Object.keys`.

use epdf_plugin_view_manager::JsRecord;

#[test]
fn keys_enumerate_in_property_order() {
    let mut record = JsRecord::new();
    record.insert("b", 1);
    record.insert("1", 2);
    record.insert("a", 3);
    record.insert("0", 4);
    assert_eq!(record.keys().collect::<Vec<_>>(), ["0", "1", "b", "a"]);
    assert_eq!(record.values().copied().collect::<Vec<_>>(), [4, 2, 1, 3]);
}
