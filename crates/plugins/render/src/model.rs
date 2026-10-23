//! `model.ts`: the render ledger, per-page raster versions. Every function
//! here is a pure transition or projection; the controller applies the
//! transitions with `ctx.state.update`.
//!
//! The page-layer rule (`PageRenderLayers`, `PageLayerRights`,
//! `resolvePageLayers`) is engine-core's (`dto/PageRender.ts`). It sits here
//! as a pure copy, beside the one key part the plugin builds from it,
//! because `crates/epdf-engine`, which has the native copy, links PDFium and
//! this crate also builds for wasm32.

use std::collections::BTreeMap;

use epdf_kernel::Error;
use serde::{Deserialize, Serialize};

use crate::contract::InvalidateScope;

/// `RenderState`: per-page versions of what a page picture can draw, keyed
/// by page object number. A picture's version is the sum of the parts it
/// draws, and is part of every raster and tile key.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RenderState {
    /// Content versions, bumped by content facts (redaction, flatten).
    pub content_epochs: BTreeMap<u32, u64>,
    /// Annotation versions, bumped by annotation facts (widgets excepted).
    pub annotation_epochs: BTreeMap<u32, u64>,
    /// Form field versions, bumped by form facts: a fill, a widget's move or
    /// look, a signing.
    pub field_epochs: BTreeMap<u32, u64>,
}

/// `initialRenderState()`.
pub fn initial_render_state() -> RenderState {
    RenderState::default()
}

fn bump(epochs: &BTreeMap<u32, u64>, pons: &[u32]) -> BTreeMap<u32, u64> {
    let mut next = epochs.clone();
    for &pon in pons {
        *next.entry(pon).or_insert(0) += 1;
    }
    next
}

/// `invalidatePages(state, pages, scope)`: bump the pages' versions in the
/// ledger the scope names. `None` is the TypeScript returning `state`
/// itself (no pages). A content fact bumps only the content ledger; every
/// picture counts content (see [`render_epoch_of`]), which is how content
/// invalidation reaches them all.
pub fn invalidate_pages(
    state: &RenderState,
    pons: &[u32],
    scope: InvalidateScope,
) -> Option<RenderState> {
    if pons.is_empty() {
        return None;
    }
    let mut next = state.clone();
    match scope {
        InvalidateScope::Content => next.content_epochs = bump(&state.content_epochs, pons),
        InvalidateScope::Annotations => {
            next.annotation_epochs = bump(&state.annotation_epochs, pons)
        }
        InvalidateScope::Fields => next.field_epochs = bump(&state.field_epochs, pons),
    }
    Some(next)
}

/// `renderEpochOf(state, page, layers)`: the version of one picture of a
/// page, its content plus the annotations and form fields it draws.
pub fn render_epoch_of(state: &RenderState, pon: u32, layers: PageRenderLayers) -> u64 {
    let of = |epochs: &BTreeMap<u32, u64>| epochs.get(&pon).copied().unwrap_or(0);
    of(&state.content_epochs)
        + if layers.include_annotations { of(&state.annotation_epochs) } else { 0 }
        + if layers.include_form_fields { of(&state.field_epochs) } else { 0 }
}

/// `layersKeyOf(layers)`: what a picture draws, as one key part. `a1f0`
/// draws annotations and no form fields.
pub fn layers_key_of(layers: PageRenderLayers) -> String {
    format!("a{}f{}", u8::from(layers.include_annotations), u8::from(layers.include_form_fields))
}

// ── engine-core's page-layer rule ─────────────────────────────────────────────

/// `PageRenderLayers`: what a page picture draws beside the page content.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageRenderLayers {
    pub include_annotations: bool,
    pub include_form_fields: bool,
}

/// `PageLayerRights`: what a session may read of those parts,
/// `doc.annotate.read` and `doc.forms.read`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageLayerRights {
    pub annotations: bool,
    pub form_fields: bool,
}

/// `resolvePageLayers(options, may)`: each option as given, and for each one
/// left out, what the caller may read. A left-out `include_form_fields` is
/// `false` when `include_annotations` is `false`. `true` for something the
/// caller may not read is refused with the engine's `PermissionDenied`,
/// naming the option.
pub fn resolve_page_layers(
    include_annotations: Option<bool>,
    include_form_fields: Option<bool>,
    may: PageLayerRights,
) -> Result<PageRenderLayers, Error> {
    if include_annotations == Some(true) && !may.annotations {
        return Err(Error::permission_denied("doc.annotate.read", Some("includeAnnotations")));
    }
    if include_form_fields == Some(true) && !may.form_fields {
        return Err(Error::permission_denied("doc.forms.read", Some("includeFormFields")));
    }
    Ok(PageRenderLayers {
        include_annotations: include_annotations.unwrap_or(may.annotations),
        include_form_fields: include_form_fields.unwrap_or(if include_annotations == Some(false) {
            false
        } else {
            may.form_fields
        }),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: PageLayerRights = PageLayerRights { annotations: true, form_fields: true };

    #[test]
    fn a_left_out_option_draws_what_the_session_may_read() {
        let layers = resolve_page_layers(None, None, ALL).unwrap();
        assert_eq!(
            layers,
            PageRenderLayers { include_annotations: true, include_form_fields: true }
        );
        let fill_only = PageLayerRights { annotations: false, form_fields: true };
        let layers = resolve_page_layers(None, None, fill_only).unwrap();
        assert_eq!(
            layers,
            PageRenderLayers { include_annotations: false, include_form_fields: true }
        );
        // No annotations leaves the form fields out too, unless asked for.
        let layers = resolve_page_layers(Some(false), None, ALL).unwrap();
        assert_eq!(layers, PageRenderLayers::default());
        let layers = resolve_page_layers(Some(false), Some(true), ALL).unwrap();
        assert_eq!(
            layers,
            PageRenderLayers { include_annotations: false, include_form_fields: true }
        );
    }

    #[test]
    fn asking_for_a_part_the_session_may_not_read_is_refused() {
        let none = PageLayerRights::default();
        let error = resolve_page_layers(Some(true), None, none).unwrap_err();
        assert_eq!(error.member("required").and_then(|v| v.as_str()), Some("doc.annotate.read"));
        let error = resolve_page_layers(None, Some(true), none).unwrap_err();
        assert_eq!(error.member("required").and_then(|v| v.as_str()), Some("doc.forms.read"));
    }

    #[test]
    fn a_picture_versions_on_the_parts_it_draws() {
        let state = RenderState {
            content_epochs: [(11, 2)].into(),
            annotation_epochs: [(11, 3)].into(),
            field_epochs: [(11, 5)].into(),
        };
        let pick = |a, f| PageRenderLayers { include_annotations: a, include_form_fields: f };
        assert_eq!(render_epoch_of(&state, 11, pick(false, false)), 2);
        assert_eq!(render_epoch_of(&state, 11, pick(true, false)), 5);
        assert_eq!(render_epoch_of(&state, 11, pick(false, true)), 7);
        assert_eq!(render_epoch_of(&state, 11, pick(true, true)), 10);
        assert_eq!(render_epoch_of(&state, 99, pick(true, true)), 0);
        assert_eq!(layers_key_of(pick(true, false)), "a1f0");
    }
}
