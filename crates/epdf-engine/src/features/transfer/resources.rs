//! What an export and an import do with a document around a bundle
//! (`services/src/features/transfer/`): the page table an export builds
//! (`bundlePages.ts`), the resources it keeps (`BundleResources.ts`), and
//! the checks an import runs on a bundle it receives (`checkWireBundle.ts`).

use std::collections::HashSet;

use serde_json::Value;

use super::annotation_bundle::assert_annotation_bundle_manifest;
use super::bundle::{
    Bundle, BundleImportTarget, BundleKind, BundlePage, assert_resource_ids, page_key,
};
use super::form_bundle::assert_form_bundle_manifest;
use super::limits::{BundleLimits, Limit, assert_within_limit};
use super::page_refs::page_refs_in;
use crate::access::OwnedDocument;
use crate::error::EngineError;
use crate::geom::PdfSize;
use crate::identity::{PageObjectNumber, PageRef};

/// The resources of one export: each once, under the SHA-256 of its bytes,
/// however many rows name it, and within the limits as each is kept
/// (`BundleResources`).
pub struct BundleResources {
    kind: BundleKind,
    pub limits: BundleLimits,
    pub by_id: Vec<(String, Vec<u8>)>,
    pub total_bytes: u64,
}

impl BundleResources {
    pub fn new(kind: BundleKind, limits: BundleLimits) -> Self {
        Self { kind, limits, by_id: Vec::new(), total_bytes: 0 }
    }

    /// Keep `bytes`, once: their id (`keep`).
    pub fn keep(&mut self, bytes: Vec<u8>) -> Result<String, EngineError> {
        assert_within_limit(self.kind, &self.limits, Limit::ResourceBytes, bytes.len() as u64)?;
        let id = super::bundle::resource_id_of(&bytes)?;
        if !self.by_id.iter().any(|(held, _)| *held == id) {
            self.total_bytes += bytes.len() as u64;
            self.by_id.push((id.clone(), bytes));
            assert_within_limit(
                self.kind,
                &self.limits,
                Limit::Resources,
                self.by_id.len() as u64,
            )?;
            assert_within_limit(self.kind, &self.limits, Limit::BundleBytes, self.total_bytes)?;
        }
        Ok(id)
    }
}

/// Every page of the document, in order, as an import's target
/// (`session.allRecords()`). A page with no object number can't be named,
/// so it is passed over.
pub fn import_targets(owned: &OwnedDocument) -> Vec<BundleImportTarget> {
    let document = owned.document();
    (0..document.page_count())
        .filter_map(|index| {
            let page = PageObjectNumber::new(document.page_object_number_by_index(index)).ok()?;
            Some(BundleImportTarget { page: PageRef::new(page), position: index })
        })
        .collect()
}

/// A bundle's page table: every page `rows` are on or point at (each page
/// ref anywhere in them), in document order, with its position and the size
/// of its visible box (`bundlePagesOf`).
pub fn bundle_pages_of(
    owned: &OwnedDocument,
    rows: &[Value],
) -> Result<Vec<BundlePage>, EngineError> {
    let named: HashSet<String> =
        rows.iter().flat_map(|row| page_refs_in(row).into_iter().map(page_key)).collect();
    let mut pages = Vec::new();
    for target in import_targets(owned) {
        if !named.contains(&page_key(target.page)) {
            continue;
        }
        let visible = crate::features::pages::visible_box_of(owned, target.page.object_number())?;
        pages.push(BundlePage {
            page: target.page,
            position: target.position,
            size: PdfSize {
                width: visible.right - visible.left,
                height: visible.top - visible.bottom,
            },
        });
    }
    Ok(pages)
}

/// Check a bundle as an import receives it, before anything uses it, the
/// checks cheapest first: the manifest, given the resources' sizes by id;
/// then each resource against its id (`checkWireBundle`).
pub fn check_wire_bundle(
    kind: BundleKind,
    bundle: &Bundle,
    limits: &BundleLimits,
) -> Result<(), EngineError> {
    let sizes = bundle.sizes();
    match kind {
        BundleKind::Annotation => {
            assert_annotation_bundle_manifest(&bundle.manifest, &sizes, limits)?
        }
        BundleKind::Form => assert_form_bundle_manifest(&bundle.manifest, &sizes, limits)?,
    }
    assert_resource_ids(kind, &bundle.resources)
}

/// Refuse an image resource whose decoded size is past `imagePixels`, read
/// from its header before it is decoded: a small PNG can decode to
/// gigabytes (`assertImageWithinLimit`).
pub fn assert_image_within_limit(
    kind: BundleKind,
    limits: &BundleLimits,
    bytes: &[u8],
) -> Result<(), EngineError> {
    if let Some((width, height)) =
        crate::features::binary::sniff(bytes).and_then(|f| f.dimensions())
    {
        assert_within_limit(
            kind,
            limits,
            Limit::ImagePixels,
            u64::from(width) * u64::from(height),
        )?;
    }
    Ok(())
}
