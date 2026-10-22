//! Captures of the dictionaries an op writes, and putting them back
//! (`services/src/features/changes/internal/objectCaptures.ts`, over the
//! dictionary captures of `captures.ts`).
//!
//! An op captures each dictionary before it writes it (every key, and under
//! `deep` the objects the layer holds), and once it has written, the
//! dictionary as it left it, in canonical bytes: the guard. Its reverse puts
//! the dictionaries back while each still reads as the op left it.

use pdfium::Document;

use super::record::CapturedObject;
use crate::error::{EngineError, EngineErrorCode};

/// A dictionary an op is about to write, captured as it is (`None`: it
/// doesn't exist yet) (`PendingCapture`).
#[derive(Debug, Clone, PartialEq)]
pub struct PendingCapture {
    pub object_number: u32,
    pub deep: Vec<String>,
    pub before: Option<Vec<u8>>,
}

/// Capture each dictionary before an op writes it: every key, and under
/// `deep` (such as `/AP`) the objects the layer holds (`captureBefore`).
/// Each object once; `0` (a direct object) is passed over.
pub fn capture_before(document: &Document, objects: &[(u32, &[&str])]) -> Vec<PendingCapture> {
    let mut captured: Vec<PendingCapture> = Vec::new();
    for (object_number, deep) in objects {
        if *object_number == 0 || captured.iter().any(|c| c.object_number == *object_number) {
            continue;
        }
        captured.push(PendingCapture {
            object_number: *object_number,
            deep: deep.iter().map(|key| (*key).to_owned()).collect(),
            before: document.export_dict_raw(*object_number, &deep.join(" ")),
        });
    }
    captured
}

/// Complete captures once the op has written: each dictionary as the op
/// left it, the guard, in canonical bytes (`captureAfter`).
pub fn capture_after(
    document: &Document,
    pending: Vec<PendingCapture>,
) -> Result<Vec<CapturedObject>, EngineError> {
    pending
        .into_iter()
        .map(|object| {
            let after = canonical(document, object.object_number).ok_or_else(|| {
                EngineError::with_message(
                    EngineErrorCode::Unknown,
                    format!("the capture of object {} failed", object.object_number),
                )
            })?;
            Ok(CapturedObject {
                object_number: object.object_number,
                deep: object.deep,
                before: object.before,
                after,
            })
        })
        .collect()
}

/// Whether every dictionary still reads exactly as the op left it
/// (`unchangedSince`).
pub fn unchanged_since(document: &Document, objects: &[CapturedObject]) -> bool {
    objects
        .iter()
        .all(|object| canonical(document, object.object_number).as_ref() == Some(&object.after))
}

/// Put every dictionary back as it was before the op (one the op made is
/// left as it is: nothing reaches it any more), and answer the captures
/// that redo the op: each dictionary as it was just before, and as the
/// revert left it (`revertObjects`). The imports run in a layer
/// transaction, this engine's own when none is open (Task 8.30 opens one
/// around every write).
pub fn revert_objects(
    document: &Document,
    objects: &[CapturedObject],
) -> Result<Vec<CapturedObject>, EngineError> {
    let redo = revert_objects_pending(document, objects)?;
    capture_after(document, redo)
}

/// [`revert_objects`], answering the redo's captures before the guard is
/// taken, for a revert that writes once more before it is done: an outline
/// item counts its ancestors again (`revertObjects`, then `captureAfter`).
pub fn revert_objects_pending(
    document: &Document,
    objects: &[CapturedObject],
) -> Result<Vec<PendingCapture>, EngineError> {
    let keys: Vec<Vec<&str>> =
        objects.iter().map(|object| object.deep.iter().map(String::as_str).collect()).collect();
    let wanted: Vec<(u32, &[&str])> = objects
        .iter()
        .zip(&keys)
        .map(|(object, deep)| (object.object_number, deep.as_slice()))
        .collect();
    let redo = capture_before(document, &wanted);
    let own_transaction = !document.layer_in_transaction();
    if own_transaction && !document.layer_begin_transaction() {
        return Err(EngineError::with_message(
            EngineErrorCode::Unknown,
            "EPDFLayer_BeginTransaction refused",
        ));
    }
    for object in objects {
        let Some(before) = &object.before else { continue };
        if !document.import_dict_raw(before) {
            if own_transaction {
                document.layer_abort_transaction();
            }
            return Err(EngineError::with_message(
                EngineErrorCode::Unknown,
                "EPDFDoc_ImportDictRaw refused a capture",
            ));
        }
    }
    if own_transaction && !document.layer_commit_transaction() {
        return Err(EngineError::with_message(
            EngineErrorCode::Unknown,
            "EPDFLayer_CommitTransaction refused",
        ));
    }
    Ok(redo)
}

/// The dictionary's canonical bytes, `None` when there is none at that
/// number (`canonicalDict` with no deep or skipped keys).
fn canonical(document: &Document, object_number: u32) -> Option<Vec<u8>> {
    document.export_dict_canonical(object_number, "", "")
}
