//! Reordering, rotating, removing, inserting and naming pages.
//!
//! G13. Ports `services/src/features/pages/PagesMutator.ts`,
//! `PagesInserter.ts` and `internal/pageIndexAt.ts`, with the result types
//! of `core/src/mutation/Page*Result.ts`. The five structure writes run as
//! ops of `document.apply` (`features::changes::pages`); the names are
//! their own kinds.
//!
//! # A page's identity is its object number, and a move does not change it
//!
//! There is no weak page reference in this engine. A page is addressed by
//! its PDF object number, which survives every reorder, so there is no
//! document-level revision token and nothing for a client to re-resolve
//! after a move. `FPDF_MovePages` rewrites pointers in the page tree and
//! leaves each page's `/Annots` untouched, so every annotation ref a caller
//! holds survives the shuffle: shuffling pages must not silently break a
//! pending highlight.
//!
//! # Where pages go is a neighbour
//!
//! A position names a page (`{ before }`, `{ after }`) or an end
//! ([`PagePosition`]), never an index, so a change keeps its meaning when
//! other pages come and go before it applies.
//!
//! # Every operation reads the layout back
//!
//! A reorder changes every page's index, a rotation changes what a page
//! shows, and a delete or an insert changes both for everything after it.
//! There is no useful subset to report, so each returns the whole list, in
//! the file's coordinates; `document.apply` answers it in page space.

use serde::{Deserialize, Serialize};

use super::pages::{PageListSnapshot, list_pages};
use crate::cancel::CancelToken;
use crate::error::{EngineError, EngineErrorCode};
use crate::features::annotation::write::delete::promote_inline_annotations;
use crate::identity::{PageObjectNumber, PageRef};
use crate::mutation::MutationMeta;
use crate::mutation::list_position::{End, ListPosition};
use crate::session::document::DocumentSession;

/// Where pages go among the document's pages (`PagePosition`).
pub type PagePosition = ListPosition<PageRef>;

/// What a reorder answers (`PageReorderResult`): the pages that moved, in
/// their new order, and the new layout.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageReorderResult {
    pub pages: Vec<PageRef>,
    pub layout: PageListSnapshot,
    pub meta: MutationMeta,
}

/// What a rotate, a delete, a name set or a name removal answers
/// (`PageRotateResult`, `PageDeleteResult`, `PageNameResult`): the new
/// layout.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageLayoutResult {
    pub layout: PageListSnapshot,
    pub meta: MutationMeta,
}

/// What an insert answers (`PageInsertResult`): the new pages, in the order
/// they landed, and the new layout. Their object numbers are fresh and are
/// read from the layout AFTER the insert: they do not exist before it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PageInsertResult {
    pub inserted_pages: Vec<PageRef>,
    pub layout: PageListSnapshot,
    pub meta: MutationMeta,
}

/// How many blank pages one call may create (`PAGE_INSERT_BLANK_MAX_COUNT`).
///
/// A ceiling rather than a limit on what the format allows: a larger batch
/// is a loop at the call site, and the number is shared with the server so
/// a client never discovers a lower one by surprise.
pub const INSERT_BLANK_MAX_COUNT: u32 = 100;

/// `PagesMutator.reorder`: the pages go together, in the order given, to
/// `position`. `FPDF_MovePages` detaches them and puts them back as a block
/// at an index among the pages left: the neighbour's, or the one after it.
pub fn reorder_pages(
    session: &mut DocumentSession,
    pages: &[PageRef],
    position: &PagePosition,
    cancel: &CancelToken,
) -> Result<PageReorderResult, EngineError> {
    let pages = resolve_page_refs(session, pages)?;
    cancel.check()?;
    require_unique("pages.reorder", &pages)?;
    if let Some(anchor) = position.anchor()
        && pages.contains(&anchor.object_number())
    {
        return Err(EngineError::with_message(
            EngineErrorCode::InvalidArg,
            format!(
                "pages.reorder: the neighbour page {} is one of the pages that move",
                anchor.object_number().get()
            ),
        )
        .with_details(serde_json::json!({ "field": "position" })));
    }
    let from: Vec<u32> = pages
        .iter()
        .map(|page| {
            cancel.check()?;
            Ok(session.record_by_object_number(*page)?.page_index)
        })
        .collect::<Result<_, EngineError>>()?;
    let count = session.document()?.document().page_count();
    // Where the block goes among the pages left once it is out.
    let at = page_index_at(session, position, count)?;
    let to = at - from.iter().filter(|index| **index < at).count() as u32;

    let signed: Vec<i32> = from.iter().map(|index| *index as i32).collect();
    if !session.document()?.document().move_pages(&signed, to as i32) {
        // `FPDF_MovePages` validates the whole request before touching the
        // tree, so a refusal means nothing moved.
        return Err(EngineError::with_message(
            EngineErrorCode::InvalidArg,
            format!(
                "FPDF_MovePages rejected the request (toIndex={to}, fromIndices=[{}])",
                from.iter().map(u32::to_string).collect::<Vec<_>>().join(",")
            ),
        ));
    }
    session.refresh_page_registry()?;
    let layout = list_pages(session.document()?.document(), cancel)?;
    Ok(PageReorderResult {
        pages: pages.into_iter().map(PageRef::new).collect(),
        layout,
        meta: meta_of(session)?,
    })
}

/// `PagesMutator.rotate`: every page gets `rotation`, absolute ("set to"),
/// never relative, so a retried request can't turn a page twice. Rotation is
/// presentation: page identity and order are untouched.
pub fn rotate_pages(
    session: &mut DocumentSession,
    pages: &[PageRef],
    rotation: &serde_json::Number,
    cancel: &CancelToken,
) -> Result<PageLayoutResult, EngineError> {
    let pages = resolve_page_refs(session, pages)?;
    cancel.check()?;
    require_unique("pages.rotate", &pages)?;
    let quarter_turns = match rotation.as_f64() {
        Some(0.0) => 0,
        Some(90.0) => 1,
        Some(180.0) => 2,
        Some(270.0) => 3,
        _ => {
            return Err(EngineError::with_message(
                EngineErrorCode::InvalidArg,
                format!("pages.rotate rotation must be 0, 90, 180 or 270 (got {rotation})"),
            ));
        }
    };
    let document = session.document()?.document();
    for page in &pages {
        if !document.set_page_rotation_by_object_number(page.get(), quarter_turns) {
            return Err(EngineError::with_message(
                EngineErrorCode::Unknown,
                format!(
                    "EPDFDoc_SetPageRotationByObjectNumber rejected page {} after validation",
                    page.get()
                ),
            ));
        }
    }
    let layout = list_pages(document, cancel)?;
    Ok(PageLayoutResult { layout, meta: meta_of(session)? })
}

/// `PagesMutator.delete`: a deleted page's object number is RETIRED (the
/// fork nulls the object rather than freeing the slot), so per-page state
/// keyed by object number can never silently attach to an unrelated future
/// page. Every surviving page keeps its identity, and its annotations their
/// names; the registrations naming a deleted page go with it.
pub fn delete_pages(
    session: &mut DocumentSession,
    pages: &[PageRef],
    cancel: &CancelToken,
) -> Result<PageLayoutResult, EngineError> {
    let pages = resolve_page_refs(session, pages)?;
    cancel.check()?;
    require_unique("pages.delete", &pages)?;
    // A document with no pages is not a document.
    let total = session.document()?.document().page_count();
    if pages.len() as u32 >= total {
        return Err(EngineError::with_message(
            EngineErrorCode::InvalidArg,
            format!(
                "pages.delete would remove every page ({} of {total}); a document must keep at \
                 least one",
                pages.len()
            ),
        ));
    }
    for page in &pages {
        // Thread confinement means no other job is mid-flight, so a held
        // page here is a leaked acquire: refused loudly rather than deleted
        // under a live handle.
        if session.is_page_held(*page) {
            return Err(EngineError::with_message(
                EngineErrorCode::Unknown,
                format!(
                    "internal invariant violation: pagePtr for page {} is still held during \
                     pages.delete",
                    page.get()
                ),
            ));
        }
    }
    let document = session.document()?.document();
    for page in &pages {
        if !document.delete_page_by_object_number(page.get()) {
            return Err(EngineError::with_message(
                EngineErrorCode::Unknown,
                format!(
                    "EPDFDoc_DeletePageByObjectNumber rejected page {} after validation",
                    page.get()
                ),
            ));
        }
    }
    session.refresh_page_registry()?;
    // A retired number never comes back, so what is kept for it is dead
    // weight that would also answer stale.
    for page in &pages {
        session.drop_page_state(*page)?;
    }
    let layout = list_pages(session.document()?.document(), cancel)?;
    Ok(PageLayoutResult { layout, meta: meta_of(session)? })
}

/// `PagesInserter.insertBlank`: `count` blank pages (from the spec, 1 when
/// left out) of the spec's `size` at `position`, made without loading them
/// (`EPDFPage_InsertBlankRaw`). `object_numbers`, one per page, names them:
/// numbers the session holds, each claimed before the first page is made.
pub fn insert_blank_pages(
    session: &mut DocumentSession,
    spec: &serde_json::Map<String, serde_json::Value>,
    position: &PagePosition,
    object_numbers: Option<&[serde_json::Number]>,
    cancel: &CancelToken,
) -> Result<PageInsertResult, EngineError> {
    cancel.check()?;
    let size = spec.get("size");
    let dimension = |key: &str| size.and_then(|size| size.get(key));
    let (width, height) = (
        dimension("width").and_then(serde_json::Value::as_f64),
        dimension("height").and_then(serde_json::Value::as_f64),
    );
    let (width, height) = match (width, height) {
        (Some(w), Some(h)) if w.is_finite() && h.is_finite() && w > 0.0 && h > 0.0 => (w, h),
        _ => {
            return Err(EngineError::with_message(
                EngineErrorCode::InvalidArg,
                format!(
                    "pages.insertBlank size must have positive finite dimensions, got {}x{}",
                    js_text(dimension("width")),
                    js_text(dimension("height"))
                ),
            ));
        }
    };
    let count = match spec.get("count") {
        None => Some(1),
        Some(count) => count
            .as_f64()
            .filter(|n| n.fract() == 0.0 && *n >= 1.0 && *n <= f64::from(INSERT_BLANK_MAX_COUNT))
            .map(|n| n as u32),
    }
    .ok_or_else(|| {
        EngineError::with_message(
            EngineErrorCode::InvalidArg,
            format!(
                "pages.insertBlank count {} out of range [1, {INSERT_BLANK_MAX_COUNT}]",
                js_text(spec.get("count"))
            ),
        )
    })?;

    let total = session.document()?.document().page_count();
    let at = page_index_at(session, position, total)?;
    let numbers = match object_numbers {
        None => None,
        Some(numbers) => {
            if numbers.len() != count as usize {
                return Err(EngineError::with_message(
                    EngineErrorCode::InvalidArg,
                    format!(
                        "pages.insertBlank needs one object number per page: {} for {count}",
                        numbers.len()
                    ),
                )
                .with_details(serde_json::json!({ "field": "objectNumbers" })));
            }
            let numbers =
                numbers.iter().map(whole_object_number).collect::<Result<Vec<u32>, _>>()?;
            for number in &numbers {
                session.claim_object_number(*number)?;
            }
            Some(numbers)
        }
    };

    // A number of its own needs the write's layer transaction, whose abort
    // takes back the pages this call already made.
    let document = session.document()?.document();
    for created in 0..count {
        let number = numbers.as_ref().map_or(0, |numbers| numbers[created as usize]);
        if !document.insert_blank_page_raw(at + created, width, height, number) {
            // The index was checked: a numbered page refused is a number
            // another object has.
            if number != 0 {
                return Err(crate::session::document::object_number_unavailable(number, "taken"));
            }
            return Err(EngineError::with_message(
                EngineErrorCode::Unknown,
                format!(
                    "EPDFPage_InsertBlankRaw rejected page {}/{count} at index {}",
                    created + 1,
                    at + created
                ),
            ));
        }
    }
    session.refresh_page_registry()?;
    read_inserted(session, at, count, cancel)
}

/// `PagesInserter.insert`: every page of the PDF `bytes`, deep-copied in at
/// `position` (`FPDF_ImportPagesByIndex`). The copy is not a full detach: an
/// imported stream still reads through the source, so the source document
/// is retained on the session, its one owner, until the session closes. An
/// inserted page is born in this document, not in its file, so its inline
/// annotations are born as objects here, named by their numbers.
pub fn insert_pages(
    session: &mut DocumentSession,
    bytes: &[u8],
    position: &PagePosition,
    cancel: &CancelToken,
) -> Result<PageInsertResult, EngineError> {
    cancel.check()?;
    if bytes.is_empty() {
        return Err(EngineError::with_message(
            EngineErrorCode::InvalidArg,
            "pages.insert requires non-empty bytes",
        ));
    }
    let total = session.document()?.document().page_count();
    let at = page_index_at(session, position, total)?;
    let source = pdfium::Document::from_bytes(pdfium::Library::acquire(), bytes, None).map_err(
        |e| match e {
            // A password-protected source is not a broken file.
            pdfium::PdfiumError::Password => EngineError::with_message(
                EngineErrorCode::DocPasswordRequired,
                "pages.insert source PDF requires a password",
            ),
            _ => EngineError::with_message(
                EngineErrorCode::MalformedPdf,
                "pages.insert source PDF could not be opened",
            ),
        },
    )?;
    let count = source.page_count();
    if count == 0 {
        return Err(EngineError::with_message(
            EngineErrorCode::InvalidArg,
            "pages.insert source PDF has no pages",
        ));
    }
    // An empty index list means every page, in order.
    if !session.document()?.document().import_pages(&source, &[], at as i32) {
        return Err(EngineError::with_message(
            EngineErrorCode::Unknown,
            format!("FPDF_ImportPagesByIndex rejected the insert at index {at}"),
        ));
    }
    session.retain_until_close(move || drop(source));
    session.refresh_page_registry()?;
    let result = read_inserted(session, at, count, cancel)?;
    let document = session.document()?.document();
    for (offset, page) in result.inserted_pages.iter().enumerate() {
        promote_inline_annotations(document, at + offset as u32, page.object_number())?;
    }
    Ok(result)
}

/// `PagesMutator.setName`: register `name` for the page, removing `replace`
/// first when it is a different, non-empty name. Names are layout: no page
/// moves and nothing's identity changes.
pub fn set_name(
    session: &mut DocumentSession,
    name: &str,
    page: PageRef,
    replace: Option<&str>,
    cancel: &CancelToken,
) -> Result<PageLayoutResult, EngineError> {
    cancel.check()?;
    if name.is_empty() {
        return Err(EngineError::with_message(
            EngineErrorCode::InvalidArg,
            "pages.setName requires a non-empty name",
        ));
    }
    let page = session.record_by_object_number(page.object_number())?.page_object_number;
    let document = session.document()?.document();
    if let Some(old) = replace.filter(|r| !r.is_empty() && *r != name) {
        // The fork answers false for a key it did not have, which upstream
        // ignores too: the rename still registers `name`.
        document.remove_named_page(old).map_err(named_page_error)?;
    }
    if !document.set_named_page(name, page.get()).map_err(named_page_error)? {
        return Err(EngineError::with_message(
            EngineErrorCode::Unknown,
            format!("EPDFDoc_SetNamedPage rejected '{name}' for page {}", page.get()),
        ));
    }
    let layout = list_pages(document, cancel)?;
    Ok(PageLayoutResult { layout, meta: meta_of(session)? })
}

/// `PagesMutator.removeName`: drop one registration; the page stays.
pub fn remove_name(
    session: &mut DocumentSession,
    name: &str,
    cancel: &CancelToken,
) -> Result<PageLayoutResult, EngineError> {
    cancel.check()?;
    if name.is_empty() {
        return Err(EngineError::with_message(
            EngineErrorCode::InvalidArg,
            "pages.removeName requires a non-empty name",
        ));
    }
    let document = session.document()?.document();
    if !document.remove_named_page(name).map_err(named_page_error)? {
        return Err(EngineError::with_message(
            EngineErrorCode::NotFound,
            format!("pages.removeName: no /Names /Pages registration '{name}'"),
        ));
    }
    let layout = list_pages(document, cancel)?;
    Ok(PageLayoutResult { layout, meta: meta_of(session)? })
}

fn named_page_error(error: pdfium::PdfiumError) -> EngineError {
    EngineError::with_message(EngineErrorCode::InvalidArg, error.to_string())
}

/// `pageIndexAt`: the page index `position` stands for among `count`
/// pages: the start is 0, the end `count`, and a neighbour its own index
/// (`before`) or the next one (`after`). A neighbour that isn't a page of
/// the document is `NotFound`.
pub fn page_index_at(
    session: &mut DocumentSession,
    position: &PagePosition,
    count: u32,
) -> Result<u32, EngineError> {
    Ok(match position {
        ListPosition::End(End::Start) => 0,
        ListPosition::End(End::End) => count,
        ListPosition::Before { before } => {
            session.record_by_object_number(before.object_number())?.page_index
        }
        ListPosition::After { after } => {
            session.record_by_object_number(after.object_number())?.page_index + 1
        }
    })
}

/// `resolvePageRefs`: each ref's page, in order; `NotFound` for one the
/// document doesn't have.
pub fn resolve_page_refs(
    session: &mut DocumentSession,
    pages: &[PageRef],
) -> Result<Vec<PageObjectNumber>, EngineError> {
    pages
        .iter()
        .map(|page| Ok(session.record_by_object_number(page.object_number())?.page_object_number))
        .collect()
}

/// `requireUniquePageObjectNumbers`: at least one page, and none twice.
///
/// A duplicate is refused rather than deduplicated. In a move it would make
/// the destination ambiguous; in a delete the second one names a page that
/// is already gone. Either way the caller meant something this cannot do.
fn require_unique(operation: &str, pages: &[PageObjectNumber]) -> Result<(), EngineError> {
    if pages.is_empty() {
        return Err(EngineError::with_message(
            EngineErrorCode::InvalidArg,
            format!("{operation} requires at least one page"),
        ));
    }
    let mut seen = std::collections::BTreeSet::new();
    for page in pages {
        if !seen.insert(page.get()) {
            return Err(EngineError::with_message(
                EngineErrorCode::InvalidArg,
                format!("{operation} was given duplicate page object number {}", page.get()),
            ));
        }
    }
    Ok(())
}

/// The write's meta: no page is named, and the transaction's stamp
/// (`affectedPages: []`).
fn meta_of(session: &DocumentSession) -> Result<MutationMeta, EngineError> {
    Ok(MutationMeta::local(Vec::new(), &session.write_stamp()?))
}

/// The layout, and which of its pages are the new ones.
fn read_inserted(
    session: &DocumentSession,
    at: u32,
    count: u32,
    cancel: &CancelToken,
) -> Result<PageInsertResult, EngineError> {
    let layout = list_pages(session.document()?.document(), cancel)?;
    let inserted_pages = layout
        .pages
        .iter()
        .skip(at as usize)
        .take(count as usize)
        .map(|page| PageRef::new(page.page_object_number))
        .collect();
    Ok(PageInsertResult { inserted_pages, layout, meta: meta_of(session)? })
}

/// A whole object number, as `claimObjectNumber` takes it.
fn whole_object_number(number: &serde_json::Number) -> Result<u32, EngineError> {
    number
        .as_u64()
        .or_else(|| number.as_f64().filter(|n| n.fract() == 0.0 && *n >= 0.0).map(|n| n as u64))
        .and_then(|n| u32::try_from(n).ok())
        .ok_or_else(|| {
            EngineError::with_message(
                EngineErrorCode::InvalidArg,
                format!("{number} is not an object number"),
            )
            .with_details(serde_json::json!({ "field": "objectNumber" }))
        })
}

/// A value as a JavaScript template literal writes it: `undefined` when it
/// is missing, a string bare.
fn js_text(value: Option<&serde_json::Value>) -> String {
    match value {
        None => "undefined".to_owned(),
        Some(serde_json::Value::String(text)) => text.clone(),
        Some(other) => other.to_string(),
    }
}

/// What flattening one page did (`PageFlattenStatus`). A page that fails
/// fails the whole call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FlattenStatus {
    /// The page's annotations are now content.
    Applied,
    /// There was nothing to flatten, which is not a failure: a page with no
    /// annotations is a page that is already flat.
    Unchanged,
}

/// One page's outcome (`PageFlattenItemResult`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FlattenItem {
    #[serde(rename = "page", with = "crate::identity::page_ref")]
    pub page_object_number: PageObjectNumber,
    pub status: FlattenStatus,
}

/// What a flatten did (`PageFlattenResult`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FlattenResult {
    /// The request as it was made, in order. Kept so an event replay can
    /// see what was asked for and not only what happened.
    #[serde(rename = "pages", with = "crate::identity::page_refs")]
    pub page_object_numbers: Vec<PageObjectNumber>,
    pub usage: FlattenUsage,
    pub results: Vec<FlattenItem>,
    /// The pages the flatten changed, applied or promoted, in the order it
    /// first changed them, and the write's stamp.
    pub meta: MutationMeta,
}

/// Which appearance to bake.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FlattenUsage {
    Display,
    Print,
}

impl FlattenUsage {
    /// Which number the fork takes.
    ///
    /// Not observable on this corpus, and the corpus tried.
    /// `annotation_appearances.pdf` carries a square with `/F 36`, which
    /// prints and never shows on screen, and that is the one annotation the
    /// two usages should disagree about. They do not: both leave the same
    /// four annotations on the page and render the same pixels. So a build
    /// that passed `Display` for both passes every vector, and the mapping
    /// is written the way the fork's header names it rather than the way
    /// this corpus can prove.
    pub(crate) fn native(self) -> pdfium::FlattenUsage {
        match self {
            Self::Display => pdfium::FlattenUsage::Display,
            Self::Print => pdfium::FlattenUsage::Print,
        }
    }
}

/// A page whose flatten failed: `Unknown`, with `PagesFlattener`'s
/// messages.
fn flatten_failure(outcome: pdfium::FlattenOutcome) -> EngineError {
    let message = match outcome {
        pdfium::FlattenOutcome::Unknown(code) => {
            format!("native page flatten returned unexpected code {code}")
        }
        _ => String::from("native page flatten failed after preflight"),
    };
    EngineError::with_message(EngineErrorCode::Unknown, message)
}

/// `PagesFlattener.requireUniquePages`, with its messages.
fn require_unique_pages(pages: &[PageObjectNumber]) -> Result<(), EngineError> {
    if pages.is_empty() {
        return Err(EngineError::with_message(
            EngineErrorCode::InvalidArg,
            "pages.flatten requires at least one page",
        ));
    }
    let mut seen = std::collections::BTreeSet::new();
    for page in pages {
        if !seen.insert(page.get()) {
            return Err(EngineError::with_message(
                EngineErrorCode::InvalidArg,
                format!("pages.flatten was given duplicate page object number {}", page.get()),
            ));
        }
    }
    Ok(())
}

/// Turn each page's annotations into page content (`PagesFlattener`), in
/// the session's open write transaction, whose stamp the `meta` carries.
/// Each page's inline annotations become objects first, so the entries the
/// flatten removes from `/Annots` leave every other position alone.
///
/// All or nothing: a page that fails, or a cancel, refuses the write, and
/// its layer transaction aborts, taking back every page before it.
pub fn flatten_pages(
    session: &DocumentSession,
    pages: &[PageObjectNumber],
    usage: FlattenUsage,
    cancel: &CancelToken,
) -> Result<FlattenResult, EngineError> {
    use crate::access::{DocumentAccess, PageAccess};

    cancel.check()?;
    let owned = session.document()?;
    let indices =
        pages.iter().map(|page| owned.page_index(*page)).collect::<Result<Vec<_>, _>>()?;
    require_unique_pages(pages)?;

    let mut results = Vec::with_capacity(pages.len());
    let mut affected: Vec<PageObjectNumber> = Vec::new();
    // All or nothing (`PagesFlattener`): a page that fails, or a cancel,
    // refuses the write, whose abort takes back every page promoted or
    // flattened before it. Each page loaded here closes before the abort.
    for (page, page_index) in pages.iter().copied().zip(indices) {
        cancel.check()?;
        if promote_inline_annotations(owned.document(), page_index, page)? {
            affected.push(page);
        }
        let status = match owned.page(page)?.view().flatten(usage.native()) {
            pdfium::FlattenOutcome::NothingToDo => FlattenStatus::Unchanged,
            pdfium::FlattenOutcome::Applied => FlattenStatus::Applied,
            // Unexercised: nothing in the corpus makes a flatten fail.
            outcome => return Err(flatten_failure(outcome)),
        };
        if status == FlattenStatus::Applied && !affected.contains(&page) {
            affected.push(page);
        }
        results.push(FlattenItem { page_object_number: page, status });
    }

    let meta = MutationMeta::local(
        affected.into_iter().map(PageRef::new).collect(),
        &session.write_stamp()?,
    );
    Ok(FlattenResult { page_object_numbers: pages.to_vec(), usage, results, meta })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failed_flatten_is_unknown_with_the_flatteners_message() {
        let failed = flatten_failure(pdfium::FlattenOutcome::Failed);
        assert!(failed.is(EngineErrorCode::Unknown), "{failed}");
        assert_eq!(failed.message, "native page flatten failed after preflight");
        assert_eq!(
            flatten_failure(pdfium::FlattenOutcome::Unknown(7)).message,
            "native page flatten returned unexpected code 7",
        );
    }

    #[test]
    fn a_flatten_names_at_least_one_page_and_each_once() {
        let page = |n| PageObjectNumber::new(n).unwrap();
        let empty = require_unique_pages(&[]).unwrap_err();
        assert!(empty.is(EngineErrorCode::InvalidArg), "{empty}");
        assert_eq!(empty.message, "pages.flatten requires at least one page");
        let twice = require_unique_pages(&[page(3), page(4), page(3)]).unwrap_err();
        assert!(twice.is(EngineErrorCode::InvalidArg), "{twice}");
        assert_eq!(twice.message, "pages.flatten was given duplicate page object number 3");
        assert!(require_unique_pages(&[page(3), page(4)]).is_ok());
    }
}
