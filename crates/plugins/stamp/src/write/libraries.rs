//! `write/libraries.ts`: library writes. A library is a PDF: its `/Title`
//! is the name, its `/Names /Pages` registry the assets, its pages the
//! artwork, PieceInfo only what has no standard home. Every whole-PDF
//! rewrite rides the per-library mutation queue.

use std::collections::HashSet;
use std::rc::Rc;

use epdf_kernel::bytes::Bytes;
use epdf_kernel::cancellable::cancellable;
use epdf_kernel::engine::DocumentHandle;
use epdf_kernel::error::code;
use epdf_kernel::geometry::PageRef;
use epdf_kernel::js::number_to_string;
use epdf_kernel::rt::Promise;
use epdf_kernel::{Error, undefined};
use epdf_plugin_annotation::binary::{resolve_binary_source, sniff_binary_metadata};
use serde_json::{Value, json};

use crate::blank_library::blank_library_pdf;
use crate::contract::*;
use crate::controller::StampCapability;
use crate::convention::*;
use crate::js::{
    adopt, entry_name, entry_string, entry_string_array, member, nullish, optional_object,
};
use crate::model::{add_asset, add_library, remove_library, set_library};
use crate::services::asset_engine::{bytes_of, call, close, page_call, uid};
use crate::services::binaries::AssetBinary;
use crate::services::errors::{CAPABILITY, not_found, stamp_error, throw_if_cancelled, verb};

/// `kindFromPdfName(name)`.
fn kind_from_pdf_name(name: Option<&str>) -> Option<StampAssetKind> {
    match name?.to_lowercase().as_str() {
        "stamp" => Some(StampAssetKind::Stamp),
        "signature" => Some(StampAssetKind::Signature),
        "initials" => Some(StampAssetKind::Initials),
        _ => None,
    }
}

/// `allocateId(preferred, prefix, taken)`: the preferred id when it is free,
/// else a fresh one.
pub(crate) fn allocate_id(
    preferred: Option<&str>,
    prefix: &str,
    taken: &mut HashSet<String>,
) -> String {
    let mut id = preferred.filter(|id| !id.is_empty()).map(str::to_string);
    if id.as_ref().is_none_or(|id| taken.contains(id)) {
        loop {
            let next = uid(prefix);
            if !taken.contains(&next) {
                id = Some(next);
                break;
            }
        }
    }
    let id = id.unwrap_or_default();
    taken.insert(id.clone());
    id
}

/// One page an import registers as a stamp.
struct Descriptor {
    page: Value,
    index: f64,
    name: String,
    label: String,
}

/// What an import commits once its document has closed.
#[derive(Clone)]
struct Imported {
    library: StampLibrary,
    canonical_bytes: Option<Vec<u8>>,
    assets: Vec<(StampAsset, Option<Vec<u8>>, StampAssetPreview)>,
}

/// A layout's page, by object number.
fn page_by_object_number(pages: &[Value], page: &Value) -> Option<Value> {
    let number = page.get("objectNumber")?.as_f64()?;
    // `new Map(entries)`: a later entry for a number wins.
    pages
        .iter()
        .rev()
        .find(|layout| {
            layout.get("ref").and_then(|r| r.get("objectNumber")).and_then(Value::as_f64)
                == Some(number)
        })
        .cloned()
}

fn size_of(layout: &Value) -> StampSize {
    let size = layout.get("size");
    let n = |k: &str| size.and_then(|s| s.get(k)).and_then(Value::as_f64).unwrap_or(f64::NAN);
    StampSize { width: n("width"), height: n("height") }
}

fn page_ref(value: &Value) -> Result<PageRef, Error> {
    PageRef::from_json(value)
        .ok_or_else(|| Error::named("TypeError", format!("[stamp] not a page ref: {value}")))
}

impl StampCapability {
    /// `createLibrary(name, options)` without the `verb` wrapper: the asset
    /// writes create a library of their own through it.
    pub(crate) fn create_library_now(
        &self,
        name: &str,
        options: CreateLibraryOptions,
    ) -> Promise<StampLibrary> {
        let this = self.clone();
        let name = name.to_string();
        Promise::spawn(async move {
            let signal = options.signal.clone();
            throw_if_cancelled(signal.as_ref())?;
            let kind = options.kind.clone().unwrap_or_else(|| DEFAULT_LIBRARY_KIND.to_string());
            let mut taken: HashSet<String> = this.state().libraries.keys().cloned().collect();
            let id = allocate_id(options.id.as_deref(), "stamp-lib", &mut taken);
            let doc = cancellable(
                CAPABILITY,
                signal.clone(),
                this.open_asset_document(blank_library_pdf()),
            )
            .await?;
            let body = async {
                call(&doc, "metadata.update", vec![json!({ "title": name })]).await?;
                call(
                    &doc,
                    "pieceInfo.update",
                    vec![
                        json!(STAMP_LIBRARY_PIECEINFO_APP),
                        stamp_library_piece_info(
                            &id,
                            Some(&kind),
                            options.categories.as_deref(),
                            None,
                        ),
                    ],
                )
                .await?;
                Ok(bytes_of(&call(&doc, "download", Vec::new()).await?))
            }
            .await;
            let bytes = close(&doc, body).await?;
            throw_if_cancelled(signal.as_ref())?;
            let library =
                StampLibrary::created(id.clone(), name.clone(), kind, options.categories.clone());
            this.0.binaries.library_binaries.borrow_mut().insert(id.clone(), bytes);
            this.update_state(|s| add_library(s, &library));
            this.0.events.library_changed.emit(&StampLibraryChangedEvent {
                library_id: id.clone(),
                reason: StampLibraryChangeReason::Created,
            });
            this.0
                .events
                .library_created
                .emit(&StampLibraryEvent { library_id: id, library: Some(library.clone()) });
            Ok(library)
        })
    }

    /// `createLibrary(name, options)`: an empty library. Fires
    /// `onLibraryCreated` and `onLibraryChanged`.
    pub fn create_library(
        &self,
        name: &str,
        options: CreateLibraryOptions,
    ) -> Promise<StampLibrary> {
        verb(self.create_library_now(name, options))
    }

    /// `importLibrary(source, options)`: a PDF becomes a library (one asset
    /// per page). Rejects `invalid-input` for bytes that aren't a PDF,
    /// `unsupported` without a local asset engine. Fires `onLibraryCreated`,
    /// then `onAssetCreated` per stamp.
    pub fn import_library(
        &self,
        source: Value,
        options: ImportLibraryOptions,
    ) -> Promise<StampLibrary> {
        let this = self.clone();
        verb(Promise::spawn(async move {
            let signal = options.signal.clone();
            throw_if_cancelled(signal.as_ref())?;
            let resolved =
                cancellable(CAPABILITY, signal.clone(), resolve_binary_source(&source)).await?;
            let meta = sniff_binary_metadata(&resolved.bytes);
            if meta.map(|m| m.mime_type) != Some("application/pdf") {
                return Err(stamp_error(
                    code::INVALID_INPUT,
                    "importLibrary needs PDF bytes (use createAsset for a raster image)",
                ));
            }
            let doc =
                cancellable(CAPABILITY, signal.clone(), this.open_asset_document(resolved.bytes))
                    .await?;
            let body = this.import_into(&doc, &options).await;
            let imported = close(&doc, body).await?;
            throw_if_cancelled(signal.as_ref())?;
            let library_id = imported.library.id.clone();
            this.0
                .binaries
                .library_binaries
                .borrow_mut()
                .insert(library_id.clone(), imported.canonical_bytes.clone());
            this.update_state(|s| add_library(s, &imported.library));
            for (asset, bytes, preview) in &imported.assets {
                this.0.binaries.binaries.borrow_mut().insert(
                    asset.id.clone(),
                    AssetBinary { bytes: bytes.clone(), preview: Some(preview.clone()) },
                );
                this.update_state(|s| add_asset(s, asset));
            }
            this.0.events.library_changed.emit(&StampLibraryChangedEvent {
                library_id: library_id.clone(),
                reason: StampLibraryChangeReason::Imported,
            });
            this.0.events.library_created.emit(&StampLibraryEvent {
                library_id: library_id.clone(),
                library: Some(imported.library.clone()),
            });
            for (asset, _, _) in &imported.assets {
                this.0.events.asset_created.emit(&StampAssetEvent {
                    asset_id: asset.id.clone(),
                    library_id: library_id.clone(),
                    asset: Some(asset.clone()),
                });
            }
            Ok(this.state().libraries.get(&library_id).cloned().unwrap_or(imported.library))
        }))
    }

    /// The body of `importLibrary`'s `try`, on the opened document.
    async fn import_into(
        &self,
        doc: &Rc<dyn DocumentHandle>,
        options: &ImportLibraryOptions,
    ) -> Result<Imported, Error> {
        let layout = call(doc, "pages.list", Vec::new()).await?;
        if member(&layout, "pageCount")?.as_f64() == Some(0.0) {
            return Err(stamp_error(
                code::INVALID_INPUT,
                "a canonical stamp library PDF must contain at least one page",
            ));
        }
        let pages: Vec<Value> = member(&layout, "pages")?.as_array().cloned().unwrap_or_default();

        // Library identity: /Title (Acrobat), then format version 1
        // PieceInfo, then the caller's fallback.
        let catalog = call(doc, "pieceInfo.get", vec![json!(STAMP_LIBRARY_PIECEINFO_APP)]).await?;
        let catalog_entries = optional_object(&catalog, "entries");
        let mut taken: HashSet<String> = self.state().libraries.keys().cloned().collect();
        let library_id =
            allocate_id(entry_string(&catalog_entries, "Id").as_deref(), "stamp-lib", &mut taken);
        let doc_meta = call(doc, "metadata.get", Vec::new()).await?;
        let title = member(&doc_meta, "title")?;
        let title_text = title.as_str().filter(|t| !t.is_empty()).map(str::to_string);
        let library_name = title_text
            .clone()
            .or_else(|| entry_string(&catalog_entries, "Name"))
            .or_else(|| options.name.clone())
            .unwrap_or_else(|| "Stamps".to_string());
        if title_text.is_none() {
            call(doc, "metadata.update", vec![json!({ "title": library_name })]).await?;
        }
        let library_categories = options
            .categories
            .clone()
            .or_else(|| entry_string_array(&catalog_entries, "Categories"));
        let library_locale = entry_string(&catalog_entries, "Locale");
        // What the file is for: the caller's word, else the file's, else a
        // plain stamp library.
        let library_kind = options.library_kind.clone().unwrap_or_else(|| {
            library_kind_from_pdf_name(entry_name(&catalog_entries, "Kind").as_deref())
        });

        // The registry: /Names /Pages keys `identifier=label` naming pages.
        let named_pages = layout.get("namedPages").filter(|v| !nullish(v));
        let entries: Vec<Value> =
            named_pages.and_then(Value::as_array).cloned().unwrap_or_default();
        let is_page = |entry: &Value| {
            entry.get("target").and_then(|t| t.get("kind")).and_then(Value::as_str) == Some("page")
        };
        let registry: Vec<&Value> = entries.iter().filter(|e| is_page(e)).collect();
        let hidden = entries.iter().filter(|e| !is_page(e)).count();
        if hidden > 0 {
            crate::console::warn(vec![Value::String(format!(
                "[stamp] library '{library_name}' carries {hidden} hidden-template or dangling registration(s); they are not stamps and were ignored"
            ))]);
        }

        let mut descriptors: Vec<Descriptor> = Vec::new();
        if !registry.is_empty() {
            let mut seen = HashSet::new();
            for entry in registry {
                let key = entry.get("name").and_then(Value::as_str).unwrap_or_default();
                let StampKey { name, label } = parse_stamp_key(key);
                if name.is_empty() {
                    return Err(stamp_error(
                        code::INVALID_INPUT,
                        format!("library '{library_name}': empty stamp identifier in key '{key}'"),
                    ));
                }
                if !seen.insert(name.clone()) {
                    return Err(stamp_error(
                        code::INVALID_INPUT,
                        format!("library '{library_name}': duplicate stamp identifier '{name}'"),
                    ));
                }
                let target = entry
                    .get("target")
                    .and_then(|t| t.get("page"))
                    .cloned()
                    .unwrap_or_else(undefined);
                let page = page_by_object_number(&pages, &target).ok_or_else(|| {
                    Error::named("TypeError", "Cannot read properties of undefined (reading 'ref')")
                })?;
                descriptors.push(Descriptor {
                    page: page.get("ref").cloned().unwrap_or_else(undefined),
                    index: page.get("index").and_then(Value::as_f64).unwrap_or(f64::NAN),
                    name,
                    label,
                });
            }
            // Display order is page order, never the tree's key-sorted order.
            descriptors
                .sort_by(|a, b| a.index.partial_cmp(&b.index).unwrap_or(std::cmp::Ordering::Equal));
        } else if named_pages.is_none() {
            return Err(stamp_error(
                code::UNSUPPORTED,
                "the asset engine reports no named-page registry; an engine with named pages is needed to import libraries",
            ));
        } else {
            // A plain PDF: every page is a stamp. Register it so the
            // canonical copy is Acrobat-readable on export. Format version 1
            // PieceInfo keys (`Name`, `Subject`) name the stamps of a library
            // without a registry.
            for page in &pages {
                let reference = page.get("ref").cloned().unwrap_or_else(undefined);
                let index = page.get("index").and_then(Value::as_f64).unwrap_or(f64::NAN);
                let pon = page_ref(&reference)?.page_object_number;
                let version1 =
                    page_call(doc, "page.pieceInfo.get", pon, vec![json!(STAMP_PIECEINFO_APP)])
                        .await?;
                let version1 = (!nullish(&version1))
                    .then(|| version1.get("entries").and_then(Value::as_object).cloned())
                    .flatten();
                let name = version1
                    .as_ref()
                    .and_then(|e| entry_string(e, "Name"))
                    .unwrap_or_else(|| format!("Stamp{}", number_to_string(index + 1.0)));
                let label = match version1.as_ref().and_then(|e| entry_string(e, "Subject")) {
                    Some(label) => label,
                    None => match &options.asset_name {
                        Some(asset_name) => asset_name(index)?,
                        None => None,
                    }
                    .unwrap_or_else(|| name.clone()),
                };
                descriptors.push(Descriptor { page: reference, index, name, label });
            }
            for descriptor in &descriptors {
                call(
                    doc,
                    "pages.setName",
                    vec![json!({
                        "name": stamp_key(&descriptor.name, &descriptor.label),
                        "page": descriptor.page,
                    })],
                )
                .await?;
            }
        }

        call(
            doc,
            "pieceInfo.update",
            vec![
                json!(STAMP_LIBRARY_PIECEINFO_APP),
                stamp_library_piece_info(
                    &library_id,
                    Some(&library_kind),
                    library_categories.as_deref(),
                    library_locale.as_deref(),
                ),
            ],
        )
        .await?;

        let mut assets = Vec::new();
        for descriptor in &descriptors {
            let page = page_ref(&descriptor.page)?;
            let pon = page.page_object_number;
            let read =
                page_call(doc, "page.pieceInfo.get", pon, vec![json!(STAMP_PIECEINFO_APP)]).await?;
            let entries = optional_object(&read, "entries");
            let kind = options
                .kind
                .or_else(|| kind_from_pdf_name(entry_name(&entries, "Kind").as_deref()))
                .unwrap_or(StampAssetKind::Stamp);
            let subject = entry_string(&entries, "SubjectOverride");
            let categories = entry_string_array(&entries, "Categories");
            let layout = page_by_object_number(&pages, &descriptor.page).unwrap_or(Value::Null);
            let asset = StampAsset::new(
                asset_id_for(&library_id, &descriptor.name),
                library_id.clone(),
                kind,
                descriptor.name.clone(),
                descriptor.label.clone(),
                size_of(&layout),
                page,
                subject.clone(),
                categories.clone(),
            );
            page_call(
                doc,
                "page.pieceInfo.update",
                pon,
                vec![
                    json!(STAMP_PIECEINFO_APP),
                    stamp_piece_info(kind, subject.as_deref(), categories.as_deref()),
                ],
            )
            .await?;
            // One canonical page: one derived placement PDF plus a thumbnail.
            let bytes =
                bytes_of(&call(doc, "pages.extract", vec![json!([descriptor.page])]).await?);
            let preview = self.render_thumbnail(doc, pon).await?;
            assets.push((asset, bytes, preview));
        }

        let library = StampLibrary::imported(
            library_id,
            library_name,
            library_kind,
            library_locale,
            library_categories,
        );
        let canonical_bytes = bytes_of(&call(doc, "download", Vec::new()).await?);
        Ok(Imported { library, canonical_bytes, assets })
    }

    /// `dropLibrary(id)`: forget a library's binaries and its state.
    fn drop_library(&self, id: &str) {
        if let Some(library) = self.state().libraries.get(id) {
            for asset_id in &library.asset_ids {
                self.0.binaries.binaries.borrow_mut().remove(asset_id);
                self.0.binaries.ghost_renders.borrow_mut().remove(asset_id);
            }
        }
        self.0.binaries.library_binaries.borrow_mut().remove(id);
        self.update_state(|s| remove_library(s, id));
    }

    /// `deleteLibrary(libraryId)`: delete a library with its assets. Fires
    /// `onLibraryDeleted`.
    pub fn delete_library(&self, library_id: &str, options: OperationOptions) -> Promise<()> {
        let this = self.clone();
        let id = library_id.to_string();
        verb(self.mutate_library(
            library_id,
            move || {
                Promise::spawn(async move {
                    let existed = this.state().libraries.contains_key(&id);
                    this.drop_library(&id);
                    if existed {
                        this.0.events.library_changed.emit(&StampLibraryChangedEvent {
                            library_id: id.clone(),
                            reason: StampLibraryChangeReason::Removed,
                        });
                        this.0
                            .events
                            .library_deleted
                            .emit(&StampLibraryEvent { library_id: id, library: None });
                    }
                    Ok(())
                })
            },
            options.signal,
        ))
    }

    /// `updateLibrary(libraryId, changes)`: rename a library or change its
    /// categories. Rejects `not-found`. Fires `onLibraryUpdated`.
    pub fn update_library(
        &self,
        library_id: &str,
        patch: StampLibraryPatch,
        options: OperationOptions,
    ) -> Promise<StampLibrary> {
        let this = self.clone();
        let id = library_id.to_string();
        verb(Promise::spawn(async move {
            let signal = options.signal.clone();
            throw_if_cancelled(signal.as_ref())?;
            if !this.state().libraries.contains_key(&id) {
                return Err(not_found("library", &id));
            }
            let queued = {
                let (this, id, signal, queued) =
                    (this.clone(), id.clone(), signal.clone(), signal.clone());
                this.clone().mutate_library(
                    &id.clone(),
                    move || {
                        Promise::spawn(
                            async move { this.rewrite_library(&id, &patch, signal).await },
                        )
                    },
                    queued,
                )
            };
            adopt(queued).await
        }))
    }

    /// `updateLibrary`'s mutation.
    async fn rewrite_library(
        &self,
        id: &str,
        patch: &StampLibraryPatch,
        signal: Option<epdf_kernel::signal::AbortSignal>,
    ) -> Result<StampLibrary, Error> {
        let Some(library) = self.state().libraries.get(id).cloned() else {
            return Err(not_found("library", id));
        };
        let canonical = self.0.binaries.library_binaries.borrow().get(id).cloned().flatten();
        let Some(canonical) = canonical else {
            return Err(stamp_error(
                code::OPERATION_FAILED,
                format!("canonical bytes are missing for library '{id}'"),
            ));
        };
        let mut next = library.clone();
        if let Some(name) = &patch.name {
            next.name = name.clone();
        }
        if let Some(categories) = &patch.categories {
            next.categories = Some(categories.clone());
            next.members.ensure("categories");
        }
        let doc =
            cancellable(CAPABILITY, signal.clone(), self.open_asset_document(canonical)).await?;
        let body = async {
            // The name is the PDF's /Title; kind, id, categories, locale ride
            // PieceInfo.
            if next.name != library.name {
                call(&doc, "metadata.update", vec![json!({ "title": next.name })]).await?;
            }
            call(
                &doc,
                "pieceInfo.update",
                vec![
                    json!(STAMP_LIBRARY_PIECEINFO_APP),
                    stamp_library_piece_info(
                        id,
                        Some(&next.kind),
                        next.categories.as_deref(),
                        next.locale.as_deref(),
                    ),
                ],
            )
            .await?;
            Ok(bytes_of(&call(&doc, "download", Vec::new()).await?))
        }
        .await;
        let rewritten = close(&doc, body).await?;
        throw_if_cancelled(signal.as_ref())?;
        self.0.binaries.library_binaries.borrow_mut().insert(id.to_string(), rewritten);
        self.update_state(|s| set_library(s, &next));
        self.0.events.library_changed.emit(&StampLibraryChangedEvent {
            library_id: id.to_string(),
            reason: StampLibraryChangeReason::Updated,
        });
        self.0
            .events
            .library_updated
            .emit(&StampLibraryEvent { library_id: id.to_string(), library: Some(next.clone()) });
        Ok(next)
    }
}

/// Bytes as the engine takes them.
pub(crate) fn bytes_json(bytes: Vec<u8>) -> Value {
    Bytes(bytes).to_json()
}
