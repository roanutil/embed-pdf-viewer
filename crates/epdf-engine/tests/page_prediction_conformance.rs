//! `packages/engine/core/src/conformance/runPagePredictionConformance.ts`:
//! the kernel's page predictions (`epdf_kernel::page_predict`, which the page
//! view replays) against this engine. Each op's predicted pages are the pages
//! a read returns once the engine runs it; so are an undo's and a redo's,
//! from the helpers the page-edit plugin stages them with. An op the engine
//! refuses leaves the predicted list as it is.
//!
//! The document is upstream's `PAGE_PREDICTION_FIXTURE_PDF`: four pages that
//! read differently (a Letter page; an A4 page turned a quarter; a page with
//! a crop box, a user unit and a tab order; a square page with a trim box,
//! turned three quarters), labelled in three sections: i, ii; 1; App-B. It
//! opens once, and every case undoes what it did.

use std::rc::Rc;

use epdf_engine::host::Host;
use epdf_kernel::page_predict::{
    PageList, pages_after, pages_after_restore, pages_relabelled, reorder_back_of,
    restored_pages_of, rotate_back_of,
};
use serde_json::{Value, json};

fn fixture() -> Vec<u8> {
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R /PageLabels << /Nums [0 << /S /r >> 2 << /S /D >> \
         3 << /S /A /P (App-) /St 2 >>] >> >>",
        "<< /Type /Pages /Kids [3 0 R 4 0 R 5 0 R 6 0 R] /Count 4 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595 842] /Rotate 90 >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 400 300] /CropBox [10 10 390 290] \
         /UserUnit 2 /Tabs /R >>",
        "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 500 500] /TrimBox [20 20 480 480] \
         /Rotate 270 >>",
    ];
    let mut out = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, object) in objects.iter().enumerate() {
        offsets.push(out.len());
        out.extend(format!("{} 0 obj\n{object}\nendobj\n", i + 1).as_bytes());
    }
    let xref = out.len();
    out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes());
    for offset in offsets {
        out.extend(format!("{offset:010} 00000 n \n").as_bytes());
    }
    out.extend(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
            objects.len() + 1
        )
        .as_bytes(),
    );
    out
}

struct Doc {
    host: Host,
    ops: u32,
}

impl Doc {
    fn open() -> Self {
        let mut host = Host::new();
        let response = host.call_filled(
            &json!({ "kind": "open.fatMem", "jobId": 1, "docId": "d" }).to_string(),
            vec![fixture()],
        );
        assert!(!response.json.contains("\"error\""), "{}", response.json);
        Self { host, ops: 0 }
    }

    /// The request's result, or its error.
    fn send(&mut self, mut request: Value) -> Result<Value, Value> {
        request["jobId"] = json!(7);
        request["docId"] = json!("d");
        let response = self.host.call_filled(&request.to_string(), Vec::new());
        let envelope: Value = serde_json::from_str(&response.json).expect("JSON");
        match envelope.get("error") {
            Some(error) => Err(error.clone()),
            None => Ok(envelope["result"].clone()),
        }
    }

    /// `doc.apply(change)`: its op id, or the refusal.
    fn apply(&mut self, change: Value) -> Result<String, Value> {
        self.ops += 1;
        let op_id = format!("op-{}", self.ops);
        self.send(json!({ "kind": "document.apply", "opId": op_id, "change": change }))?;
        Ok(op_id)
    }

    fn read(&mut self) -> Rc<PageList> {
        let list = self.send(json!({ "kind": "pages.list" })).expect("pages.list");
        PageList::new(Rc::new(list["pages"].clone()), Rc::new(list["labels"].clone()))
    }

    /// `count` fresh object numbers, reserved then taken.
    fn take(&mut self, count: u32) -> Vec<u32> {
        if count == 0 {
            return Vec::new();
        }
        let reserved =
            self.send(json!({ "kind": "objectNumbers.reserve", "count": count })).expect("numbers");
        let first = reserved["range"]["first"].as_u64().unwrap() as u32;
        (first..first + count).collect()
    }
}

fn r(number: u64) -> Value {
    json!({ "kind": "objectNumber", "objectNumber": number })
}

/// The refs of the pages at these display positions.
fn at(pages: &Value, indexes: &[usize]) -> Value {
    Value::Array(indexes.iter().map(|&i| pages[i]["ref"].clone()).collect())
}

type MakeOp = fn(&Value, &[u32]) -> Value;

/// An op made from the pages as they are, with names for any pages it makes.
struct Case {
    name: &'static str,
    /// How many fresh object numbers the op needs.
    numbers: u32,
    op: MakeOp,
}

const CASES: &[Case] = &[
    Case {
        name: "rotate pages with different turns",
        numbers: 0,
        op: |p, _| json!({ "type": "pages.rotate", "pages": at(p, &[1, 2]), "rotation": 180 }),
    },
    Case {
        name: "rotate a page to the turn it has",
        numbers: 0,
        op: |p, _| json!({ "type": "pages.rotate", "pages": at(p, &[0]), "rotation": p[0]["rotation"] }),
    },
    Case {
        name: "move pages after a neighbour, in the order given",
        numbers: 0,
        op: |p, _| {
            json!({ "type": "pages.reorder", "pages": at(p, &[3, 0]),
                "position": { "after": p[1]["ref"] } })
        },
    },
    Case {
        name: "move a page to the start",
        numbers: 0,
        op: |p, _| json!({ "type": "pages.reorder", "pages": at(p, &[2]), "position": "start" }),
    },
    Case {
        name: "move a page to the end",
        numbers: 0,
        op: |p, _| json!({ "type": "pages.reorder", "pages": at(p, &[0]), "position": "end" }),
    },
    Case {
        name: "move pages before a neighbour",
        numbers: 0,
        op: |p, _| {
            json!({ "type": "pages.reorder", "pages": at(p, &[2, 3]),
                "position": { "before": p[0]["ref"] } })
        },
    },
    Case {
        name: "move a page to where it is",
        numbers: 0,
        op: |p, _| {
            json!({ "type": "pages.reorder", "pages": at(p, &[1]),
                "position": { "after": p[0]["ref"] } })
        },
    },
    Case {
        name: "move a section's first page to the end",
        numbers: 0,
        op: |p, _| json!({ "type": "pages.reorder", "pages": at(p, &[2]), "position": "end" }),
    },
    Case {
        name: "delete a page in the middle",
        numbers: 0,
        op: |p, _| json!({ "type": "pages.delete", "pages": at(p, &[1]) }),
    },
    Case {
        name: "delete the only page of a section",
        numbers: 0,
        op: |p, _| json!({ "type": "pages.delete", "pages": at(p, &[2]) }),
    },
    Case {
        name: "delete pages apart from each other, named out of order",
        numbers: 0,
        op: |p, _| json!({ "type": "pages.delete", "pages": at(p, &[3, 0]) }),
    },
    Case {
        name: "delete pages next to each other, the first among them",
        numbers: 0,
        op: |p, _| json!({ "type": "pages.delete", "pages": at(p, &[0, 1]) }),
    },
    Case {
        name: "insert blank pages after a neighbour",
        numbers: 2,
        op: |p, n| {
            json!({ "type": "pages.insertBlank",
                "spec": { "size": { "width": 300, "height": 200 }, "count": 2 },
                "position": { "after": p[1]["ref"] }, "objectNumbers": n })
        },
    },
    Case {
        name: "insert a blank page at the start",
        numbers: 1,
        op: |_, n| {
            json!({ "type": "pages.insertBlank", "spec": { "size": { "width": 612, "height": 792 } },
                "position": "start", "objectNumbers": n })
        },
    },
    Case {
        name: "insert a blank page with no position (the end)",
        numbers: 1,
        op: |_, n| {
            json!({ "type": "pages.insertBlank",
                "spec": { "size": { "width": 841.89, "height": 595.28 }, "count": 1 },
                "objectNumbers": n })
        },
    },
    Case {
        name: "copy pages after a neighbour, in the order given",
        numbers: 2,
        op: |p, n| {
            json!({ "type": "pages.copy", "pages": at(p, &[3, 0]),
                "position": { "after": p[1]["ref"] }, "objectNumbers": n })
        },
    },
    Case {
        name: "copy a page to the start",
        numbers: 1,
        op: |p, n| {
            json!({ "type": "pages.copy", "pages": at(p, &[2]), "position": "start",
                "objectNumbers": n })
        },
    },
    Case {
        name: "copy a page twice, with no position (the end)",
        numbers: 2,
        op: |p, n| json!({ "type": "pages.copy", "pages": at(p, &[1, 1]), "objectNumbers": n }),
    },
];

/// An op the engine refuses: the prediction leaves the pages as they are.
const REFUSALS: &[Case] = &[
    Case {
        name: "rotate a page the document does not have",
        numbers: 0,
        op: |_, _| json!({ "type": "pages.rotate", "pages": [r(9_999)], "rotation": 90 }),
    },
    Case {
        name: "rotate a page named twice",
        numbers: 0,
        op: |p, _| json!({ "type": "pages.rotate", "pages": at(p, &[0, 0]), "rotation": 90 }),
    },
    Case {
        name: "rotate no pages",
        numbers: 0,
        op: |_, _| json!({ "type": "pages.rotate", "pages": [], "rotation": 90 }),
    },
    Case {
        name: "move pages after one of them",
        numbers: 0,
        op: |p, _| {
            json!({ "type": "pages.reorder", "pages": at(p, &[0, 1]),
                "position": { "after": p[1]["ref"] } })
        },
    },
    Case {
        name: "move a page next to a page the document does not have",
        numbers: 0,
        op: |p, _| {
            json!({ "type": "pages.reorder", "pages": at(p, &[0]),
                "position": { "before": r(9_999) } })
        },
    },
    Case {
        name: "delete every page",
        numbers: 0,
        op: |p, _| {
            let all: Vec<usize> = (0..p.as_array().unwrap().len()).collect();
            json!({ "type": "pages.delete", "pages": at(p, &all) })
        },
    },
    Case {
        name: "insert a blank page under the number of a page",
        numbers: 0,
        op: |p, _| {
            json!({ "type": "pages.insertBlank", "spec": { "size": { "width": 100, "height": 100 } },
                "objectNumbers": [p[0]["ref"]["objectNumber"]] })
        },
    },
    Case {
        name: "insert two blank pages with one number",
        numbers: 1,
        op: |_, n| {
            json!({ "type": "pages.insertBlank",
                "spec": { "size": { "width": 100, "height": 100 }, "count": 2 },
                "objectNumbers": n })
        },
    },
    Case {
        name: "copy a page the document does not have",
        numbers: 1,
        op: |_, n| json!({ "type": "pages.copy", "pages": [r(9_999)], "objectNumbers": n }),
    },
    Case {
        name: "copy a page under the number of a page",
        numbers: 0,
        op: |p, _| {
            json!({ "type": "pages.copy", "pages": at(p, &[0]),
                "objectNumbers": [p[1]["ref"]["objectNumber"]] })
        },
    },
    Case {
        name: "copy two pages with one number",
        numbers: 1,
        op: |p, n| json!({ "type": "pages.copy", "pages": at(p, &[0, 1]), "objectNumbers": n }),
    },
];

fn array(value: &Value) -> &[Value] {
    value.as_array().map_or(&[], Vec::as_slice)
}

/// What undoing `op` shows, from the list before it and after it: what the
/// page-edit plugin stages as the change's `undo`. The pages go back, then
/// the label sections the edit found.
fn undone_of(before: &Rc<PageList>, after: &Rc<PageList>, op: &Value) -> Rc<PageList> {
    let refs = array(&op["pages"]);
    let replay = |ops: Vec<Value>| {
        ops.iter().fold(after.clone(), |list, op| pages_after(&list, op).expect("a prediction"))
    };
    let relabelled =
        |list: Rc<PageList>| pages_relabelled(&list, &before.labels).expect("a prediction");
    match op["type"].as_str().unwrap() {
        "pages.rotate" => replay(rotate_back_of(array(&before.pages), refs).unwrap()),
        "pages.reorder" => relabelled(replay(reorder_back_of(array(&before.pages), refs).unwrap())),
        "pages.delete" => {
            let restored = restored_pages_of(array(&before.pages), refs).unwrap();
            relabelled(pages_after_restore(after, &Value::Array(restored)).unwrap())
        }
        "pages.insertBlank" | "pages.copy" => {
            let made: Vec<Value> =
                array(&op["objectNumbers"]).iter().map(|n| r(n.as_u64().unwrap())).collect();
            let delete = json!({ "type": "pages.delete", "pages": made });
            relabelled(pages_after(after, &delete).unwrap())
        }
        other => panic!("not a predictable op: {other}"),
    }
}

/// Where the prediction and the read differ, by path; numbers within a
/// thousandth of a point.
fn differences(predicted: &PageList, actual: &PageList) -> Vec<String> {
    fn walk(left: Option<&Value>, right: Option<&Value>, path: String, found: &mut Vec<String>) {
        match (left, right) {
            (Some(Value::Number(a)), Some(Value::Number(b))) => {
                if (a.as_f64().unwrap() - b.as_f64().unwrap()).abs() > 1e-3 {
                    found.push(format!("{path}: predicted {a}, actual {b}"));
                }
            }
            (Some(Value::Object(a)), Some(Value::Object(b))) => {
                let mut keys: Vec<&String> = a.keys().collect();
                keys.extend(b.keys().filter(|k| !a.contains_key(*k)));
                for key in keys {
                    walk(a.get(key), b.get(key), format!("{path}.{key}"), found);
                }
            }
            (Some(Value::Array(a)), Some(Value::Array(b))) => {
                for index in 0..a.len().max(b.len()) {
                    walk(a.get(index), b.get(index), format!("{path}.{index}"), found);
                }
            }
            (left, right) => {
                if left != right {
                    found.push(format!("{path}: predicted {left:?}, actual {right:?}"));
                }
            }
        }
    }
    let mut found = Vec::new();
    walk(Some(&predicted.pages), Some(&actual.pages), ".pages".into(), &mut found);
    walk(Some(&predicted.labels), Some(&actual.labels), ".labels".into(), &mut found);
    found
}

fn assert_same(case: &str, step: &str, predicted: &PageList, actual: &PageList) {
    let found = differences(predicted, actual);
    assert!(found.is_empty(), "{case}: {step}: {found:#?}");
}

#[test]
fn each_page_op_its_undo_and_its_redo_read_as_predicted() {
    let mut doc = Doc::open();
    for case in CASES {
        let before = doc.read();
        let numbers = doc.take(case.numbers);
        let op = (case.op)(&before.pages, &numbers);

        let predicted = pages_after(&before, &op).expect("a prediction");
        let applied = doc.apply(json!({ "ops": [op] })).expect(case.name);
        let after = doc.read();
        assert_same(case.name, "the op", &predicted, &after);

        // What the history shows for an undo and a redo of it, until the engine answers.
        let undone = undone_of(&before, &after, &op);
        let undo = doc.apply(json!({ "undoOf": applied })).expect(case.name);
        let after_undo = doc.read();
        assert_same(case.name, "its undo", &undone, &after_undo);

        let redone = pages_after(&after_undo, &op).expect("a prediction");
        let redo = doc.apply(json!({ "undoOf": undo })).expect(case.name);
        assert_same(case.name, "its redo", &redone, &doc.read());

        // The next case starts from the pages this one started from.
        doc.apply(json!({ "undoOf": redo })).expect(case.name);
        assert_same(case.name, "the redo undone", &before, &doc.read());
    }
}

#[test]
fn an_op_the_engine_refuses_leaves_the_predicted_pages_as_they_are() {
    let mut doc = Doc::open();
    for case in REFUSALS {
        let before = doc.read();
        let numbers = doc.take(case.numbers);
        let op = (case.op)(&before.pages, &numbers);
        let predicted = pages_after(&before, &op).expect("a prediction");
        assert!(Rc::ptr_eq(&predicted, &before), "refused: {}: the prediction moved", case.name);
        assert!(doc.apply(json!({ "ops": [op] })).is_err(), "refused: {}: applied", case.name);
        assert_same(case.name, "refused", &before, &doc.read());
    }
}
