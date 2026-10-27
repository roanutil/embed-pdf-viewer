//! Whole-session steps stay linear in the record count: `forget` and a
//! removal look ids up in sets, as the TypeScript does with `Set`.

use std::rc::Rc;
use std::time::Instant;

use epdf_annotation_core::types::ById;
use epdf_annotation_core::update::changes::without_records;
use epdf_annotation_core::update::{forget, initial_model};
use epdf_annotation_core::*;
use serde_json::json;

const N: usize = 20_000;

fn model(n: usize) -> Model {
    let template = ModelAnnotation::read(&json!({
        "id": "obj:0",
        "source": "vector",
        "annotation": { "subtype": "square", "page": { "kind": "objectNumber", "objectNumber": 1 } },
    }))
    .expect("a record");
    let mut by_id = ById::new();
    let mut order = Vec::new();
    for i in 0..n {
        let id = format!("obj:{i}");
        by_id.insert(id.clone(), Rc::new(ModelAnnotation { id: id.clone(), ..template.clone() }));
        order.push(id);
    }
    Model {
        selected: Rc::new(order.clone()),
        by_id: Rc::new(by_id),
        order: Rc::new(order),
        ..initial_model()
    }
}

#[test]
fn forgetting_every_record_is_linear() {
    let m = model(N);
    let ids: Vec<Id> = m.order.iter().cloned().collect();
    let started = Instant::now();
    let next = forget(&m, &ids);
    assert!(next.selected.is_empty());
    assert!(started.elapsed().as_secs_f64() < 2.0, "took {:?}", started.elapsed());
}

#[test]
fn removing_every_record_is_linear() {
    let m = model(N);
    let ids: Vec<Id> = m.order.iter().cloned().collect();
    let started = Instant::now();
    let next = without_records(&m, &ids);
    assert!(next.by_id.is_empty() && next.order.is_empty() && next.selected.is_empty());
    assert!(started.elapsed().as_secs_f64() < 2.0, "took {:?}", started.elapsed());
}
