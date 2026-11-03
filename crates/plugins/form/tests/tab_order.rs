//! The shared engine rule, over arbitrary annotation JSON and widget rows.
use epdf_plugin_form::engine::{compare_structure_positions, tab_order_of};
use serde_json::{Value, json};
fn rows() -> Vec<Value> {
    vec![
        json!({"name":3,"subtype":"widget","page":{"objectNumber":1},"rect":{"x":300,"y":400,"width":100,"height":20},"structurePosition":[0,1,0]}),
        json!({"name":1,"subtype":"widget","page":{"objectNumber":1},"rect":{"x":300,"y":100,"width":100,"height":20},"structurePosition":null}),
        json!({"name":2,"subtype":"widget","page":{"objectNumber":1},"rect":{"x":50,"y":102,"width":100,"height":20},"structurePosition":[0,0,2,1]}),
        json!({"name":4,"subtype":"widget","page":{"objectNumber":1},"rect":{"x":50,"y":250,"width":100,"height":20},"structurePosition":[0,0,5]}),
    ]
}
fn names(rows: &[Value], order: Value) -> Vec<Value> {
    tab_order_of(rows, &[json!({"ref":{"objectNumber":1},"tabOrder":order})])
        .iter()
        .map(|r| r["name"].clone())
        .collect()
}
#[test]
fn each_page_tab_policy_uses_the_pdf_order() {
    let rows = rows();
    assert_eq!(names(&rows, Value::Null), vec![json!(3), json!(1), json!(2), json!(4)]);
    assert_eq!(names(&rows, json!("annotations")), vec![json!(3), json!(1), json!(2), json!(4)]);
    assert_eq!(names(&rows, json!("rows")), vec![json!(2), json!(1), json!(4), json!(3)]);
    assert_eq!(names(&rows, json!("columns")), vec![json!(2), json!(4), json!(1), json!(3)]);
    assert_eq!(names(&rows, json!("structure")), vec![json!(2), json!(4), json!(3), json!(1)]);
    let mut mixed = rows.clone();
    let mut link = rows[0].clone();
    link["name"] = json!(5);
    link["subtype"] = json!("link");
    mixed.insert(0, link);
    assert_eq!(
        names(&mixed, json!("widgets")),
        vec![json!(3), json!(1), json!(2), json!(4), json!(5)]
    );
}
#[test]
fn missing_structure_positions_keep_page_order_and_missing_pages_disappear() {
    let rows: Vec<_> = rows()
        .into_iter()
        .map(|mut r| {
            r.as_object_mut().unwrap().remove("structurePosition");
            r
        })
        .collect();
    assert_eq!(names(&rows, json!("structure")), vec![json!(3), json!(1), json!(2), json!(4)]);
    assert!(tab_order_of(&rows, &[]).is_empty());
    assert!(compare_structure_positions(&[0, 2], &[0, 2, 0]).is_lt());
    assert!(compare_structure_positions(&[0, 1, 0], &[0, 0, 9, 9]).is_gt());
    assert!(compare_structure_positions(&[3, 1], &[3, 1]).is_eq());
}

#[test]
fn ordering_preserves_shared_annotation_identity() {
    let rows: Vec<_> = rows().into_iter().map(std::rc::Rc::new).collect();
    let sorted = tab_order_of(&rows, &[json!({"ref":{"objectNumber":1},"tabOrder":"rows"})]);
    assert!(std::rc::Rc::ptr_eq(&sorted[0], &rows[2]));
    assert!(std::rc::Rc::ptr_eq(&sorted[1], &rows[1]));
    assert!(std::rc::Rc::ptr_eq(&sorted[2], &rows[3]));
    assert!(std::rc::Rc::ptr_eq(&sorted[3], &rows[0]));
}
