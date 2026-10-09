//! The model's annotation table is shared between steps that leave it alone,
//! as the TypeScript's object spread shares `byId` and `order`.

use std::rc::Rc;

use epdf_annotation_core::*;

fn model_with(n: usize) -> Model {
    let mut m = initial_model();
    for i in 0..n {
        let x = (i % 50) as f64 * 10.0;
        let y = (i / 50) as f64 * 10.0;
        let msg = Msg::CreateAnnot {
            page: PageRef::new(1),
            subtype: "square".into(),
            geom: Geom::Rect {
                rect: Rect { x, y, width: 5.0, height: 5.0 },
                ellipse: false,
                rot: None,
            },
            preset: None,
            props: None,
            flags: None,
            select: None,
        };
        m = apply(&m, &msg).0;
    }
    m
}

#[test]
fn a_step_that_leaves_the_annotations_alone_shares_them() {
    let m = model_with(20);
    assert_eq!(m.by_id.len(), 20);
    let id = m.order[3].clone();
    let (next, _) = update(&m, &Msg::Hover { id: Some(id.clone()) });
    let next = next.expect("hover changes the model");
    assert_eq!(next.hovered, Some(id));
    assert!(Rc::ptr_eq(&m.by_id, &next.by_id));
    assert!(Rc::ptr_eq(&m.order, &next.order));

    // `apply` hands back the model itself when nothing changed.
    let (same, fx) = apply(&next, &Msg::Hover { id: next.hovered.clone() });
    assert!(fx.is_empty());
    assert!(Rc::ptr_eq(&next.by_id, &same.by_id));
}

#[test]
fn a_step_that_edits_the_annotations_leaves_the_old_model_intact() {
    let m = model_with(3);
    let id = m.order[1].clone();
    let next = apply(&m, &Msg::Remove { ids: vec![id.clone()] }).0;
    assert!(!Rc::ptr_eq(&m.by_id, &next.by_id));
    assert!(m.by_id.contains_key(&id) && m.order.contains(&id));
    assert!(!next.by_id.contains_key(&id) && !next.order.contains(&id));
    assert_eq!((m.by_id.len(), next.by_id.len()), (3, 2));
}
