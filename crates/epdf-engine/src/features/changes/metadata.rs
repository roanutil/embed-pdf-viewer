//! The metadata ops and their reverse
//! (`services/src/features/changes/internal/metadataChanges.ts`).

use serde::Serialize;
use serde_json::{Map, Value};

use super::applier::{ChangeContext, Done, assert_expected, left_alone};
use super::item::ChangeItem;
use super::op::PatchKeys;
use super::record::{MetadataOp, MetadataRevertStep, ReverseStep};
use crate::auth::scope::DocCapability;
use crate::dto::{CustomMetadataPatch, MetadataPatch};
use crate::error::{EngineError, EngineErrorCode};
use crate::features::metadata::{read_custom_metadata, read_metadata};
use crate::features::metadata_write::{
    CustomMetadataUpdateResult, MetadataUpdateResult, apply_custom_metadata_patch,
    apply_metadata_patch, check_custom_keys,
};
use crate::mutation::MutationMeta;

/// `metadata.update`; its reverse puts back each entry still holding what
/// it set.
pub fn update_metadata(
    ctx: &mut ChangeContext<'_>,
    patch: &MetadataPatch,
    expect: Option<&MetadataPatch>,
    keys: &PatchKeys,
    op_index: usize,
) -> Result<Done, EngineError> {
    ctx.authority.require(DocCapability::DocMetadataModify)?;
    ctx.cancel.check()?;
    let document = ctx.session.document()?.document();
    let before = object(&read_metadata(document, ctx.cancel)?)?;
    if let Some(expect) = expect {
        assert_expected(op_index, &before, &object(expect)?)?;
    }
    let keys = wire_keys(keys, object(patch)?.keys())?;
    if let Err(failed) = apply_metadata_patch(document, patch) {
        // Put back what the patch wrote before the write that failed: the
        // ops before this one are the transaction's to take back, this
        // one's partial write is its own.
        let restore = from_map::<MetadataPatch>(pick(&before, &keys))
            .and_then(|restore| apply_metadata_patch(document, &restore));
        return Err(taken_back_or(failed, restore));
    }
    let metadata = read_metadata(document, ctx.cancel)?;
    let after = object(&metadata)?;
    let meta = MutationMeta::local(Vec::new(), &ctx.session.write_stamp()?);
    Ok(Done {
        item: ChangeItem::MetadataUpdate {
            result: MetadataUpdateResult { metadata, meta },
            skipped: Vec::new(),
        },
        reverse: vec![revert_of(MetadataOp::Update, &keys, &before, &after)],
    })
}

/// `metadata.updateCustom`; its reverse puts back each key still holding
/// what it set.
pub fn update_custom_metadata(
    ctx: &mut ChangeContext<'_>,
    patch: &CustomMetadataPatch,
    keys: &PatchKeys,
) -> Result<Done, EngineError> {
    ctx.authority.require(DocCapability::DocMetadataModify)?;
    ctx.cancel.check()?;
    let document = ctx.session.document()?.document();
    let before = object(&read_custom_metadata(document, ctx.cancel)?)?;
    let keys = wire_keys(keys, patch.0.keys())?;
    // A key a PDF cannot hold is refused before anything is written, so
    // only a failed write has anything to take back.
    check_custom_keys(&keys)?;
    if let Err(failed) = apply_custom_metadata_patch(document, patch) {
        let restore = from_map::<CustomMetadataPatch>(pick(&before, &keys))
            .and_then(|restore| apply_custom_metadata_patch(document, &restore));
        return Err(taken_back_or(failed, restore));
    }
    let custom = read_custom_metadata(document, ctx.cancel)?;
    let after = object(&custom)?;
    let meta = MutationMeta::local(Vec::new(), &ctx.session.write_stamp()?);
    Ok(Done {
        item: ChangeItem::MetadataUpdateCustom {
            result: CustomMetadataUpdateResult { custom, meta },
            skipped: Vec::new(),
        },
        reverse: vec![revert_of(MetadataOp::UpdateCustom, &keys, &before, &after)],
    })
}

/// Put back each entry still holding what the change set; the rest are
/// left alone (`revertMetadata`).
pub fn revert_metadata(
    ctx: &mut ChangeContext<'_>,
    step: &MetadataRevertStep,
) -> Result<Done, EngineError> {
    ctx.authority.require(DocCapability::DocMetadataModify)?;
    ctx.cancel.check()?;
    let document = ctx.session.document()?.document();
    let current = match step.op {
        MetadataOp::Update => object(&read_metadata(document, ctx.cancel)?)?,
        MetadataOp::UpdateCustom => object(&read_custom_metadata(document, ctx.cancel)?)?,
    };
    let keys: Vec<String> = step.left.keys().cloned().collect();
    let kept: Vec<String> = keys
        .iter()
        .filter(|key| entry_of(&current, key) == entry_of(&step.left, key))
        .cloned()
        .collect();
    if kept.is_empty() {
        return left_alone(ctx, step.op.type_name());
    }
    let skipped: Vec<String> = keys.into_iter().filter(|key| !kept.contains(key)).collect();
    let restore = pick(&step.restore, &kept);
    let reverse = ReverseStep::MetadataRevert(MetadataRevertStep {
        op: step.op,
        restore: pick(&step.left, &kept),
        left: restore.clone(),
    });
    let meta = || -> Result<MutationMeta, EngineError> {
        Ok(MutationMeta::local(Vec::new(), &ctx.session.write_stamp()?))
    };
    let item = match step.op {
        MetadataOp::Update => {
            let patch: MetadataPatch = from_map(restore)?;
            apply_metadata_patch(document, &patch)?;
            let metadata = read_metadata(document, ctx.cancel)?;
            ChangeItem::MetadataUpdate {
                result: MetadataUpdateResult { metadata, meta: meta()? },
                skipped,
            }
        }
        MetadataOp::UpdateCustom => {
            let patch: CustomMetadataPatch = from_map(restore)?;
            apply_custom_metadata_patch(document, &patch)?;
            let custom = read_custom_metadata(document, ctx.cancel)?;
            ChangeItem::MetadataUpdateCustom {
                result: CustomMetadataUpdateResult { custom, meta: meta()? },
                skipped,
            }
        }
    };
    Ok(Done { item, reverse: vec![reverse] })
}

/// The patch's keys in wire order, as the op was given them; the patch's
/// own order for an op built in-process, which carries none. Both name the
/// same keys.
fn wire_keys<'a>(
    wire: &PatchKeys,
    patch: impl Iterator<Item = &'a String>,
) -> Result<Vec<String>, EngineError> {
    let mut own: Vec<String> = patch.cloned().collect();
    if wire.0.is_empty() {
        return Ok(own);
    }
    let mut sorted = wire.0.clone();
    sorted.sort();
    own.sort();
    if sorted != own {
        return Err(unexpected(format!(
            "the patch's keys {own:?} are not its wire keys {sorted:?}"
        )));
    }
    Ok(wire.0.clone())
}

/// A partial write's own refusal, or, when what it wrote could not be put
/// back, `Unknown` saying so (as `transaction::roll_back` reports it).
fn taken_back_or(failed: EngineError, restore: Result<(), EngineError>) -> EngineError {
    match restore {
        Ok(()) => failed,
        Err(e) => EngineError::with_message(
            EngineErrorCode::Unknown,
            format!(
                "a refused write could not be taken back: {e} (the write was refused: {failed})"
            ),
        ),
    }
}

/// The reverse of a metadata write: the entries it changed, their old values
/// (`null` for a custom key that wasn't there), and the values it left
/// (`revertOf`).
fn revert_of(
    op: MetadataOp,
    keys: &[String],
    before: &Map<String, Value>,
    after: &Map<String, Value>,
) -> ReverseStep {
    let changed: Vec<String> =
        keys.iter().filter(|key| entry_of(before, key) != entry_of(after, key)).cloned().collect();
    ReverseStep::MetadataRevert(MetadataRevertStep {
        op,
        restore: pick(before, &changed),
        left: pick(after, &changed),
    })
}

/// An entry's value; a custom key that isn't there reads as null, as a patch
/// removes one (`entryOf`).
fn entry_of(values: &Map<String, Value>, key: &str) -> Value {
    values.get(key).cloned().unwrap_or(Value::Null)
}

fn pick(values: &Map<String, Value>, keys: &[String]) -> Map<String, Value> {
    keys.iter().map(|key| (key.clone(), entry_of(values, key))).collect()
}

fn object<T: Serialize>(value: &T) -> Result<Map<String, Value>, EngineError> {
    match serde_json::to_value(value) {
        Ok(Value::Object(map)) => Ok(map),
        Ok(other) => Err(unexpected(format!("{other} is not an object"))),
        Err(e) => Err(unexpected(e.to_string())),
    }
}

fn from_map<T: serde::de::DeserializeOwned>(map: Map<String, Value>) -> Result<T, EngineError> {
    serde_json::from_value(Value::Object(map)).map_err(|e| unexpected(e.to_string()))
}

fn unexpected(message: String) -> EngineError {
    EngineError::with_message(
        EngineErrorCode::Unknown,
        format!("a metadata revert could not be read back: {message}"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_partial_write_that_cannot_be_taken_back_says_so() {
        let failed = EngineError::with_message(EngineErrorCode::Unknown, "failed to write /Author");
        assert_eq!(taken_back_or(failed.clone(), Ok(())), failed);
        let lost = taken_back_or(
            failed,
            Err(EngineError::with_message(EngineErrorCode::Unknown, "failed to write /Title")),
        );
        assert_eq!(lost.code, EngineErrorCode::Unknown);
        assert!(lost.message.contains("could not be taken back"), "{lost}");
        assert!(lost.message.contains("/Title") && lost.message.contains("/Author"), "{lost}");
    }

    #[test]
    fn wire_keys_keep_the_wire_order_and_must_name_the_patch() {
        let patch = ["author".to_owned(), "title".to_owned()];
        let wire = PatchKeys(vec!["title".into(), "author".into()]);
        assert_eq!(wire_keys(&wire, patch.iter()).unwrap(), ["title", "author"]);
        assert_eq!(wire_keys(&PatchKeys::default(), patch.iter()).unwrap(), ["author", "title"]);
        assert!(wire_keys(&PatchKeys(vec!["title".into()]), patch.iter()).is_err());
    }
}
