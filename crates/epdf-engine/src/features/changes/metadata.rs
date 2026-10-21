//! The metadata ops and their reverse
//! (`services/src/features/changes/internal/metadataChanges.ts`).

use serde::Serialize;
use serde_json::{Map, Value};

use super::applier::{ChangeContext, Done, assert_expected, left_alone};
use super::item::ChangeItem;
use super::record::{MetadataOp, MetadataRevertStep, ReverseStep};
use crate::auth::scope::DocCapability;
use crate::dto::{CustomMetadataPatch, MetadataPatch};
use crate::error::{EngineError, EngineErrorCode};
use crate::features::metadata::{read_custom_metadata, read_metadata};
use crate::features::metadata_write::{
    CustomMetadataUpdateResult, MetadataUpdateResult, apply_custom_metadata_patch,
    apply_metadata_patch,
};
use crate::mutation::MutationMeta;

/// `metadata.update`; its reverse puts back each entry still holding what
/// it set.
pub fn update_metadata(
    ctx: &mut ChangeContext<'_>,
    patch: &MetadataPatch,
    expect: Option<&MetadataPatch>,
    op_index: usize,
) -> Result<Done, EngineError> {
    ctx.authority.require(DocCapability::DocMetadataModify)?;
    ctx.cancel.check()?;
    let document = ctx.session.document()?.document();
    let before = object(&read_metadata(document, ctx.cancel)?)?;
    if let Some(expect) = expect {
        assert_expected(op_index, &before, &object(expect)?)?;
    }
    let keys: Vec<String> = object(patch)?.keys().cloned().collect();
    if let Err(failed) = apply_metadata_patch(document, patch) {
        // Put back what the patch wrote before the write that failed: the
        // ops before this one are the transaction's to take back, this
        // one's partial write is its own. Best effort: the failure is the
        // answer either way.
        if let Ok(restore) = from_map::<MetadataPatch>(pick(&before, &keys)) {
            let _ = apply_metadata_patch(document, &restore);
        }
        return Err(failed);
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
) -> Result<Done, EngineError> {
    ctx.authority.require(DocCapability::DocMetadataModify)?;
    ctx.cancel.check()?;
    let document = ctx.session.document()?.document();
    let before = object(&read_custom_metadata(document, ctx.cancel)?)?;
    let keys: Vec<String> = patch.0.keys().cloned().collect();
    if let Err(failed) = apply_custom_metadata_patch(document, patch) {
        // As above. A key a PDF cannot hold is refused before anything is
        // written, and the restore is refused the same way.
        if let Ok(restore) = from_map::<CustomMetadataPatch>(pick(&before, &keys)) {
            let _ = apply_custom_metadata_patch(document, &restore);
        }
        return Err(failed);
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
