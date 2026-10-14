use epdf_ffi::{FormFieldFamily, FormFieldRef};
use serde_json::json;

#[test]
fn an_object_number_ref_round_trips_through_the_plugins_json() {
    let r = FormFieldRef::ObjectNumber { field_object_number: 12 };
    assert_eq!(r.to_json(), json!({ "kind": "objectNumber", "fieldObjectNumber": 12 }));
    assert_eq!(FormFieldRef::from_json(&r.to_json()).unwrap(), r);
}

#[test]
fn a_name_ref_round_trips() {
    let r = FormFieldRef::Fqn { name: "billing.name".into() };
    assert_eq!(r.to_json(), json!({ "kind": "fqn", "name": "billing.name" }));
    assert_eq!(FormFieldRef::from_json(&r.to_json()).unwrap(), r);
}

#[test]
fn a_ref_of_another_kind_is_invalid_state() {
    let error = FormFieldRef::from_json(&json!({ "kind": "index", "index": 1 })).unwrap_err();
    assert_eq!(error.code(), Some("invalid-state"));
}

#[test]
fn families_parse_their_engine_names() {
    assert_eq!(FormFieldFamily::parse("combobox"), FormFieldFamily::Combobox);
    assert_eq!(FormFieldFamily::parse("barcode"), FormFieldFamily::Unknown);
    assert_eq!(FormFieldFamily::Listbox.as_str(), "listbox");
}
