//! `packages/plugin/metadata` in Rust: the document's Info-dict metadata as
//! reactive state. The effect seeds it from the engine and keeps it live off
//! the document event stream (own edits and remote ones alike); the
//! capability is the read/write surface.
//!
//! The file split follows the TypeScript package: `types`, `reducer`,
//! `capability`, `effects`, and the wiring in [`metadata_plugin`].

use std::rc::Rc;
use std::sync::LazyLock;

use epdf_kernel::rt::Promise;
use epdf_kernel::{AnyPlugin, Ctx, EffectCtx, Error, Token, define};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[cfg(feature = "harness")]
pub mod harness;

// ── types ─────────────────────────────────────────────────────────────────────

/// `DocumentMetadata`, as the engine reports it. The plugin never reads
/// inside it, so it stays the engine's JSON.
pub type DocumentMetadata = Value;

/// `MetadataPatch`: three-state per key (absent leaves, null clears, a value
/// sets). Carried through to the engine as written.
pub type MetadataPatch = Value;

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct MetadataState {
    /// Null until the first read resolves.
    pub metadata: Option<DocumentMetadata>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum MetadataAction {
    #[serde(rename = "SET")]
    Set { metadata: Option<DocumentMetadata> },
}

pub static METADATA: LazyLock<Token<MetadataCapability>> = LazyLock::new(|| Token::new("metadata"));

/// Authorization for Info-dict writes (PDF bit 4). The engine enforces the
/// same capability; this is the UI's mirror of it.
const METADATA_MODIFY_CAPABILITY: &str = "doc.metadata.modify";

// ── reducer ───────────────────────────────────────────────────────────────────

pub fn initial_metadata_state() -> MetadataState {
    MetadataState { metadata: None }
}

/// Pure. The only transition replaces the snapshot.
pub fn metadata_reducer(_state: &MetadataState, action: &MetadataAction) -> Option<MetadataState> {
    match action {
        MetadataAction::Set { metadata } => Some(MetadataState { metadata: metadata.clone() }),
    }
}

// ── capability ────────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct MetadataCapability {
    ctx: Ctx<MetadataState, MetadataAction>,
}

impl MetadataCapability {
    fn require_doc(&self) -> Result<Rc<dyn epdf_kernel::engine::DocumentHandle>, Error> {
        self.ctx.doc().ok_or_else(|| Error::new("[metadata] no document bound"))
    }

    /// Whether this session may edit the Info dict.
    pub fn can_edit(&self) -> bool {
        self.ctx.doc().is_some_and(|doc| doc.security().allows(METADATA_MODIFY_CAPABILITY))
    }

    /// Null until the initial read lands.
    pub fn current(&self) -> Option<DocumentMetadata> {
        self.ctx.state().metadata.clone()
    }

    /// Patch the Info dict. The write also emits `metadata.updated`, which
    /// the effect catches; this SET keeps state in step immediately.
    pub fn update(&self, patch: MetadataPatch) -> Promise<()> {
        let this = self.clone();
        Promise::spawn(async move {
            let doc = this.require_doc()?;
            let result: Value = doc.request("metadata.update", None, vec![patch]).await?;
            let metadata = result.get("metadata").cloned();
            this.ctx.dispatch(MetadataAction::Set { metadata });
            Ok(())
        })
    }

    /// Re-read from the engine.
    pub fn reload(&self) -> Promise<()> {
        let this = self.clone();
        Promise::spawn(async move {
            let doc = this.require_doc()?;
            let metadata: Value = doc.request("metadata.read", None, Vec::new()).await?;
            this.ctx.dispatch(MetadataAction::Set { metadata: Some(metadata) });
            Ok(())
        })
    }
}

// ── effects ───────────────────────────────────────────────────────────────────

fn register_metadata_effects(ctx: EffectCtx<MetadataState, MetadataAction>) -> Result<(), Error> {
    let Some(doc) = ctx.doc() else { return Ok(()) };

    // 1. seed. `void read().then(set, () => {})`: the SET lands one turn
    //    after the read settles.
    let read = doc.call("metadata.read", None, Vec::new());
    let seed = ctx.ctx.clone();
    epdf_kernel::rt::spawn(async move {
        if let Ok(metadata) = read.await {
            seed.dispatch(MetadataAction::Set { metadata: Some(metadata) });
        }
    });

    // 2. live updates, own and remote.
    let live = ctx.ctx.clone();
    let unsubscribe = doc.subscribe(Rc::new(move |event: &Value| {
        if event.get("type").and_then(Value::as_str) == Some("metadata.updated") {
            live.dispatch(MetadataAction::Set { metadata: event.get("metadata").cloned() });
        }
    }));
    ctx.cleanup_fn(move || unsubscribe.call());
    Ok(())
}

// ── plugin ────────────────────────────────────────────────────────────────────

/// `metadataPlugin()`.
pub fn metadata_plugin() -> AnyPlugin {
    define::<MetadataState, MetadataAction>("metadata")
        .document_scoped()
        .initial_state(initial_metadata_state)
        .reduce(metadata_reducer)
        .capability(&METADATA, |ctx| Ok(MetadataCapability { ctx }))
        .effects(register_metadata_effects)
        .build()
}
