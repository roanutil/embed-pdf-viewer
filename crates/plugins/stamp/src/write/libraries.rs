//! `write/libraries.ts`: library writes. A library IS a PDF: its `/Title`
//! is the name, its `/Names /Pages` registry the assets, its pages the
//! artwork, `/PieceInfo` only what has no standard home. Every whole-PDF
//! rewrite rides the per-library mutation queue.

use std::collections::HashSet;
use std::rc::Rc;

use epdf_geometry::Size;
use epdf_kernel::engine::DocumentHandle;
use epdf_kernel::geometry::PageRef;
use epdf_kernel::rt::{self, Promise};
use epdf_kernel::{Error, undefined};
use epdf_plugin_annotation::binary::{resolve_binary_source, sniff_binary_metadata};
use serde_json::{Value, json};

use crate::blank_library::blank_library_pdf;
use crate::contract::*;
use crate::controller::StampCapability;
use crate::convention::*;
use crate::model::StampAction;
use crate::promise::await_engine;
use crate::services::asset_engine::{bytes_of, call, close, page_call, read_off, uid};
use crate::services::binaries::AssetBinary;
use crate::value::{get, num, str_of};

// ── PieceInfo entries ────────────────────────────────────────────────────────

fn typed_entry<'a>(entries: &'a Value, key: &str, kind: &str) -> Option<&'a Value> {
    let entry = get(entries, key)?;
    (str_of(entry, "type") == Some(kind)).then_some(entry)
}

/// `entryString`: a non-empty `string` entry.
pub(crate) fn entry_string(entries: &Value, key: &str) -> Option<String> {
    let value = str_of(typed_entry(entries, key, "string")?, "value")?;
    (!value.is_empty()).then(|| value.to_string())
}

/// `entryName`: a non-empty `name` entry.
pub(crate) fn entry_name(entries: &Value, key: &str) -> Option<String> {
    let value = str_of(typed_entry(entries, key, "name")?, "value")?;
    (!value.is_empty()).then(|| value.to_string())
}

/// `entryStringArray`: a `string-array` entry, copied.
pub(crate) fn entry_string_array(entries: &Value, key: &str) -> Option<Vec<String>> {
    let entry = typed_entry(entries, key, "string-array")?;
    Some(
        get(entry, "value")
            .and_then(Value::as_array)
            .map(|values| values.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
            .unwrap_or_default(),
    )
}

/// `kindFromPdfName(name)`.
fn kind_from_pdf_name(name: Option<&str>) -> Option<StampAssetKind> {
    match name?.to_lowercase().as_str() {
        "stamp" => Some(StampAssetKind::Stamp),
        "signature" => Some(StampAssetKind::Signature),
        "initials" => Some(StampAssetKind::Initials),
        _ => None,
    }
}

/// `snapshot?.entries ?? {}`.
fn entries_of(snapshot: &Value) -> Value {
    get(snapshot, "entries").filter(|e| !e.is_null()).cloned().unwrap_or_else(|| json!({}))
}

/// `allocateId(preferred, prefix, taken)`.
pub(crate) fn allocate_id(
    preferred: Option<String>,
    prefix: &str,
    taken: &mut HashSet<String>,
) -> String {
    let id = match preferred {
        Some(id) if !id.is_empty() && !taken.contains(&id) => id,
        _ => loop {
            let id = uid(prefix);
            if !taken.contains(&id) {
                break id;
            }
        },
    };
    taken.insert(id.clone());
    id
}

pub(crate) fn pon_of_ref(page: &Value) -> Option<u32> {
    num(page, "pageObjectNumber").map(|p| p as u32)
}

fn size_of(layout: &Value) -> Size {
    let size = get(layout, "size").cloned().unwrap_or(Value::Null);
    let dim = |key| num(&size, key).unwrap_or(f64::NAN);
    Size { width: dim("width"), height: dim("height") }
}

struct Descriptor {
    page: PageRef,
    index: f64,
    name: String,
    label: String,
}

struct Imported {
    library: StampLibrary,
    canonical: Option<Vec<u8>>,
    assets: Vec<(StampAsset, AssetBinary)>,
}

impl StampCapability {
    /// `createLibrary(name, {id, kind, categories})`: a blank seed with its
    /// `/Title` and catalog PieceInfo written.
    pub fn create_library(&self, name: &str, opts: CreateLibraryOptions) -> Promise<String> {
        let this = self.clone();
        let name = name.to_string();
        Promise::spawn(async move {
            let kind = opts.kind.clone().unwrap_or_else(|| DEFAULT_LIBRARY_KIND.into());
            let mut taken: HashSet<String> = this.state().libraries.keys().cloned().collect();
            let id = allocate_id(opts.id.clone(), "stamp-lib", &mut taken);
            let doc = rt::returned(this.open_asset_document(blank_library_pdf())).await?;
            let body = async {
                Self::require_canonical_services(&doc)?;
                await_engine(call(&doc, "metadata.update", vec![json!({ "title": name })])).await?;
                let patch =
                    stamp_library_piece_info(&id, Some(&kind), opts.categories.as_deref(), None);
                await_engine(call(
                    &doc,
                    "pieceInfo.update",
                    vec![json!(STAMP_LIBRARY_PIECEINFO_APP), patch],
                ))
                .await?;
                await_engine(call(&doc, "download", Vec::new())).await
            }
            .await;
            let bytes = close(&doc, body).await?;
            this.0.library_binaries.borrow_mut().insert(id.clone(), bytes_of(&bytes));
            this.dispatch(StampAction::LibraryAdded {
                library: StampLibrary {
                    id: id.clone(),
                    name,
                    kind,
                    asset_ids: Vec::new(),
                    locale: None,
                    categories: opts.categories,
                    member_order: StampLibraryMemberOrder::Created,
                },
            });
            let events = &this.0.events;
            events.library_changed.emit(&StampLibraryChange {
                library_id: id.clone(),
                reason: StampLibraryChangeReason::Created,
            });
            events.library_created.emit(&StampLibraryEvent {
                library_id: id.clone(),
                library: this.state().libraries.get(&id).cloned(),
            });
            Ok(id)
        })
    }

    /// `importLibrary(source, opts)`: a PDF becomes a library, one asset per
    /// registered page (or per page, registered now, for a plain PDF).
    pub fn import_library(&self, source: Value, opts: ImportLibraryOptions) -> Promise<String> {
        let this = self.clone();
        Promise::spawn(async move {
            let resolved = resolve_binary_source(&source).await?;
            let meta = sniff_binary_metadata(&resolved.bytes);
            if meta.map(|m| m.mime_type) != Some("application/pdf") {
                return Err(Error::engine(
                    "InvalidArg",
                    "[stamp] importLibraryPdf needs PDF bytes (use addAsset for a raster image)",
                ));
            }
            let doc = rt::returned(this.open_asset_document(resolved.bytes)).await?;
            let body = this.import_pages(&doc, &opts).await;
            let imported = close(&doc, body).await?;

            let library = imported.library;
            let id = library.id.clone();
            this.0.library_binaries.borrow_mut().insert(id.clone(), imported.canonical);
            this.dispatch(StampAction::LibraryAdded { library: library.clone() });
            for (asset, binary) in &imported.assets {
                this.0.binaries.borrow_mut().insert(asset.id.clone(), binary.clone());
                this.dispatch(StampAction::AssetAdded { asset: asset.clone() });
            }
            let events = &this.0.events;
            events.library_changed.emit(&StampLibraryChange {
                library_id: id.clone(),
                reason: StampLibraryChangeReason::Imported,
            });
            events
                .library_created
                .emit(&StampLibraryEvent { library_id: id.clone(), library: Some(library) });
            for (asset, _) in imported.assets {
                events.asset_created.emit(&StampAssetEvent {
                    asset_id: asset.id.clone(),
                    library_id: id.clone(),
                    asset: Some(asset),
                });
            }
            Ok(id)
        })
    }

    /// The `try` block of `importLibraryPdf`.
    async fn import_pages(
        &self,
        doc: &Rc<dyn DocumentHandle>,
        opts: &ImportLibraryOptions,
    ) -> Result<Imported, Error> {
        Self::require_canonical_services(doc)?;
        let layout = await_engine(call(doc, "pages.list", Vec::new())).await?;
        if num(&layout, "pageCount") == Some(0.0) {
            return Err(Error::engine(
                "InvalidArg",
                "[stamp] a canonical stamp library PDF must contain at least one page",
            ));
        }
        let pages: Vec<Value> = match get(&layout, "pages").and_then(Value::as_array) {
            Some(pages) => pages.clone(),
            None => return Err(read_off(&undefined(), "map")),
        };
        let by_pon = |pon: u32| {
            pages.iter().find(|p| get(p, "ref").and_then(pon_of_ref) == Some(pon)).cloned()
        };

        // Library identity: /Title (Acrobat), then v1 PieceInfo, then the caller.
        let catalog =
            await_engine(call(doc, "pieceInfo.read", vec![json!(STAMP_LIBRARY_PIECEINFO_APP)]))
                .await?;
        let catalog_entries = entries_of(&catalog);
        let mut taken: HashSet<String> = self.state().libraries.keys().cloned().collect();
        let library_id = allocate_id(entry_string(&catalog_entries, "Id"), "stamp-lib", &mut taken);
        let doc_meta = await_engine(call(doc, "metadata.read", Vec::new())).await?;
        let title = str_of(&doc_meta, "title").map(str::to_string);
        let library_name = title
            .clone()
            .filter(|t| !t.is_empty())
            .or_else(|| entry_string(&catalog_entries, "Name"))
            .or_else(|| opts.name.clone())
            .unwrap_or_else(|| "Stamps".into());
        if title.as_deref().is_none_or(str::is_empty) {
            await_engine(call(doc, "metadata.update", vec![json!({ "title": library_name })]))
                .await?;
        }
        let library_categories =
            opts.categories.clone().or_else(|| entry_string_array(&catalog_entries, "Categories"));
        let library_locale = entry_string(&catalog_entries, "Locale");
        let library_kind = opts.library_kind.clone().unwrap_or_else(|| {
            library_kind_from_pdf_name(entry_name(&catalog_entries, "Kind").as_deref())
        });

        // The registry: /Names /Pages keys `identifier=label` to pages.
        let named = get(&layout, "namedPages").and_then(Value::as_array).cloned();
        let target_kind =
            |e: &Value| get(e, "target").and_then(|t| str_of(t, "kind")).map(str::to_string);
        let registry: Vec<Value> = named
            .iter()
            .flatten()
            .filter(|e| target_kind(e).as_deref() == Some("page"))
            .cloned()
            .collect();
        let hidden =
            named.iter().flatten().filter(|e| target_kind(e).as_deref() != Some("page")).count();
        if hidden > 0 {
            self.warn(
                &format!(
                    "[stamp] library '{library_name}' carries {hidden} hidden-template or dangling registration(s); they are not stamps and were ignored"
                ),
                None,
            );
        }

        let mut descriptors: Vec<Descriptor> = Vec::new();
        if !registry.is_empty() {
            let mut seen: HashSet<String> = HashSet::new();
            for entry in &registry {
                let key = str_of(entry, "name").unwrap_or_default().to_string();
                let StampKey { name, label } = parse_stamp_key(&key);
                if name.is_empty() {
                    return Err(Error::engine(
                        "InvalidArg",
                        format!(
                            "[stamp] library '{library_name}': empty stamp identifier in key '{key}'"
                        ),
                    ));
                }
                if seen.contains(&name) {
                    return Err(Error::engine(
                        "InvalidArg",
                        format!(
                            "[stamp] library '{library_name}': duplicate stamp identifier '{name}'"
                        ),
                    ));
                }
                seen.insert(name.clone());
                let pon = get(entry, "target").and_then(|t| get(t, "page")).and_then(pon_of_ref);
                let Some(page) = pon.and_then(by_pon) else {
                    return Err(read_off(&undefined(), "ref"));
                };
                descriptors.push(Descriptor {
                    page: get(&page, "ref").and_then(PageRef::from_json).unwrap_or(PageRef::new(0)),
                    index: num(&page, "index").unwrap_or(f64::NAN),
                    name,
                    label,
                });
            }
            // Display order is page order, never the tree's key order.
            descriptors.sort_by(|a, b| a.index.total_cmp(&b.index));
        } else if named.is_none() {
            return Err(Error::engine(
                "NotImplemented",
                "[stamp] the asset engine reports no named-page registry (an older engine); upgrade it to import libraries",
            ));
        } else {
            // A plain PDF: every page is a stamp, registered now so the
            // canonical copy is Acrobat-readable. v1 PieceInfo keys are
            // honoured as a fallback.
            for page in &pages {
                let page_ref =
                    get(page, "ref").and_then(PageRef::from_json).unwrap_or(PageRef::new(0));
                let pon = page_ref.page_object_number;
                let index = num(page, "index").unwrap_or(f64::NAN);
                doc.page_index(pon)?;
                let v1 = if doc.has("page.pieceInfo") {
                    let read = page_call(
                        doc,
                        "page.pieceInfo.read",
                        pon,
                        vec![json!(STAMP_PIECEINFO_APP)],
                    );
                    let snapshot = await_engine(read).await?;
                    get(&snapshot, "entries").filter(|e| !e.is_null()).cloned()
                } else {
                    // `await undefined`.
                    rt::tick().await;
                    None
                };
                let name = v1.as_ref().and_then(|v| entry_string(v, "Name")).unwrap_or_else(|| {
                    format!("Stamp{}", epdf_geometry::js::number_to_string(index + 1.0))
                });
                let label = v1
                    .as_ref()
                    .and_then(|v| entry_string(v, "Subject"))
                    .or_else(|| opts.asset_name.as_ref().and_then(|f| f(index)))
                    .unwrap_or_else(|| name.clone());
                descriptors.push(Descriptor { page: page_ref, index, name, label });
            }
            for d in &descriptors {
                let arg = json!({ "name": stamp_key(&d.name, &d.label), "page": d.page.to_json() });
                await_engine(call(doc, "pages.setName", vec![arg])).await?;
            }
        }

        let patch = stamp_library_piece_info(
            &library_id,
            Some(&library_kind),
            library_categories.as_deref(),
            library_locale.as_deref(),
        );
        await_engine(call(
            doc,
            "pieceInfo.update",
            vec![json!(STAMP_LIBRARY_PIECEINFO_APP), patch],
        ))
        .await?;

        let mut assets = Vec::new();
        for d in &descriptors {
            let pon = d.page.page_object_number;
            doc.page_index(pon)?;
            if !doc.has("page.pieceInfo") {
                return Err(Error::engine(
                    "NotImplemented",
                    "[stamp] canonical PDF libraries need page pieceInfo support",
                ));
            }
            let read = page_call(doc, "page.pieceInfo.read", pon, vec![json!(STAMP_PIECEINFO_APP)]);
            let entries = entries_of(&await_engine(read).await?);
            let kind = opts
                .kind
                .or_else(|| kind_from_pdf_name(entry_name(&entries, "Kind").as_deref()))
                .unwrap_or(StampAssetKind::Stamp);
            let subject = entry_string(&entries, "SubjectOverride");
            let categories = entry_string_array(&entries, "Categories");
            let layout = by_pon(pon).unwrap_or(Value::Null);
            let asset = StampAsset {
                id: asset_id_for(&library_id, &d.name),
                library_id: library_id.clone(),
                kind,
                name: d.name.clone(),
                label: d.label.clone(),
                size: size_of(&layout),
                page: d.page,
                subject: subject.clone(),
                categories: categories.clone(),
            };
            let update = page_call(
                doc,
                "page.pieceInfo.update",
                pon,
                vec![
                    json!(STAMP_PIECEINFO_APP),
                    stamp_piece_info(kind, subject.as_deref(), categories.as_deref()),
                ],
            );
            await_engine(update).await?;
            // One canonical page: one derived placement PDF plus a thumbnail.
            let bytes =
                await_engine(call(doc, "pages.extract", vec![json!([d.page.to_json()])])).await?;
            let preview = rt::returned(self.render_thumbnail(doc, pon)).await?;
            assets.push((asset, AssetBinary { bytes: bytes_of(&bytes), preview: Some(preview) }));
        }

        let library = StampLibrary {
            id: library_id,
            name: library_name,
            kind: library_kind,
            locale: library_locale.filter(|l| !l.is_empty()),
            categories: library_categories,
            asset_ids: Vec::new(),
            member_order: StampLibraryMemberOrder::Imported,
        };
        let canonical = await_engine(call(doc, "download", Vec::new())).await?;
        Ok(Imported { library, canonical: bytes_of(&canonical), assets })
    }

    /// `dropLibrary(id)`: the library, its assets' binaries and renders, and
    /// its canonical bytes.
    fn drop_library(&self, id: &str) {
        if let Some(library) = self.state().libraries.get(id) {
            for asset_id in &library.asset_ids {
                self.0.binaries.borrow_mut().remove(asset_id);
                self.0.ghost_renders.borrow_mut().remove(asset_id);
            }
        }
        self.0.library_binaries.borrow_mut().remove(id);
        self.dispatch(StampAction::LibraryRemoved { library_id: id.to_string() });
    }

    /// `deleteLibrary(id)`: a library with its assets, ordered with in-flight
    /// library mutations.
    pub fn delete_library(&self, id: &str) -> Promise<()> {
        let this = self.clone();
        let key = id.to_string();
        self.mutate_library(id, move || {
            Promise::spawn(async move {
                let existed = this.state().libraries.contains_key(&key);
                this.drop_library(&key);
                if existed {
                    let events = &this.0.events;
                    events.library_changed.emit(&StampLibraryChange {
                        library_id: key.clone(),
                        reason: StampLibraryChangeReason::Removed,
                    });
                    events
                        .library_deleted
                        .emit(&StampLibraryEvent { library_id: key, library: None });
                }
                Ok(())
            })
        })
    }

    /// `updateLibrary(id, {name, categories})`: the name is the PDF's
    /// `/Title`; the rest rides PieceInfo.
    pub fn update_library(&self, id: &str, patch: LibraryPatch) -> Promise<()> {
        let this = self.clone();
        let id = id.to_string();
        Promise::spawn(async move {
            if !this.state().libraries.contains_key(&id) {
                return Err(Error::engine("NotFound", format!("[stamp] unknown library '{id}'")));
            }
            let mutating = this.clone();
            let key = id.clone();
            let queued = this.mutate_library(&id, move || {
                Promise::spawn(async move { mutating.update_library_now(&key, patch).await })
            });
            crate::promise::adopt(queued).await
        })
    }

    async fn update_library_now(&self, id: &str, patch: LibraryPatch) -> Result<(), Error> {
        let Some(library) = self.state().libraries.get(id).cloned() else {
            return Err(Error::engine("NotFound", format!("[stamp] unknown library '{id}'")));
        };
        let Some(canonical) = self.0.library_binaries.borrow().get(id).cloned().flatten() else {
            return Err(Error::engine(
                "Unknown",
                format!("[stamp] canonical bytes are missing for library '{id}'"),
            ));
        };
        let mut next = library.clone();
        if let Some(name) = patch.name {
            next.name = name;
        }
        if let Some(categories) = patch.categories {
            next.categories = Some(categories);
        }
        let doc = rt::returned(self.open_asset_document(canonical)).await?;
        let body = async {
            Self::require_canonical_services(&doc)?;
            if next.name != library.name {
                await_engine(call(&doc, "metadata.update", vec![json!({ "title": next.name })]))
                    .await?;
            }
            let info = stamp_library_piece_info(
                id,
                Some(&next.kind),
                next.categories.as_deref(),
                next.locale.as_deref(),
            );
            await_engine(call(
                &doc,
                "pieceInfo.update",
                vec![json!(STAMP_LIBRARY_PIECEINFO_APP), info],
            ))
            .await?;
            await_engine(call(&doc, "download", Vec::new())).await
        }
        .await;
        let rewritten = close(&doc, body).await?;
        self.0.library_binaries.borrow_mut().insert(id.to_string(), bytes_of(&rewritten));
        self.dispatch(StampAction::LibraryUpdated { library: next.clone() });
        let events = &self.0.events;
        events.library_changed.emit(&StampLibraryChange {
            library_id: id.to_string(),
            reason: StampLibraryChangeReason::Updated,
        });
        events
            .library_updated
            .emit(&StampLibraryEvent { library_id: id.to_string(), library: Some(next) });
        Ok(())
    }
}
