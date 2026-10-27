//! The op table: what a change is made of (`core/src/mutation/Change.ts`).
//!
//! [`ChangeOp`] holds the op types this engine runs. Every other op type of
//! upstream's `ChangeOp` union is in [`OP_OWNERS`] with the sub-task that
//! registers it; a change naming one is refused with `NotImplemented` before
//! any op runs. An owner adds its variant to [`ChangeOp`], its arm to
//! [`super::applier`]'s `run_op` and to the page-space conversions
//! (`page_space::boundary::change_op_in_file_space` and
//! `change_item_in_page_space`), and removes its rows here. The matches
//! have no catch-all, so a new variant does not compile until it is placed.

use serde::{Deserialize, Serialize};

use crate::dto::{CustomMetadataPatch, MetadataPatch};
use crate::error::{EngineError, EngineErrorCode};
use crate::features::files::attachments::{AttachmentFileBase, AttachmentRef};
use crate::features::forms::snapshot::FormFieldRef;
use crate::features::forms::value::FormFieldValue;
use crate::features::outline::OutlinePosition;
use crate::identity::{OutlineItemRef, PageRef};

/// One change (`Change`): its ops in order, all of them or none; or the
/// reverse of an earlier change, named by its `opId`.
#[derive(Debug, Clone, PartialEq)]
pub enum Change {
    Ops(Vec<ChangeOp>),
    UndoOf(String),
}

/// One op of a change: exactly the parameters of its single verb
/// (`ChangeOp`). `expect` says what the op assumes the document holds now;
/// when it doesn't, the whole change is refused with `ChangeConflict`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ChangeOp {
    #[serde(rename = "metadata.update", rename_all = "camelCase")]
    MetadataUpdate {
        patch: Box<MetadataPatch>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expect: Option<Box<MetadataPatch>>,
        /// The patch's keys in the order the wire sent them: what the op's
        /// reverse lists, and an undo's `skipped` follows
        /// (`Object.keys(op.patch)`). Not part of the op's identity.
        #[serde(skip)]
        keys: PatchKeys,
    },
    #[serde(rename = "metadata.updateCustom", rename_all = "camelCase")]
    MetadataUpdateCustom {
        patch: CustomMetadataPatch,
        /// As above: the custom map is a `BTreeMap`, which sorts its keys.
        #[serde(skip)]
        keys: PatchKeys,
    },
    /// A file embedded in the document (`/EmbeddedFiles`), under its name:
    /// the name is its key, and a name another file has is refused.
    #[serde(rename = "attachments.create", rename_all = "camelCase")]
    AttachmentsCreate {
        file: AttachmentFileBase,
        /// Its bytes, as the `file` resource.
        #[serde(default)]
        resources: OpResources,
    },
    #[serde(rename = "attachments.delete")]
    AttachmentsDelete {
        #[serde(rename = "ref")]
        reference: AttachmentRef,
    },
    /// An annotation on `page`, from its data (`AnnotationDraft`) and the
    /// bytes beside it, at `objectNumber` when given. The data stays the
    /// JSON object the caller sent: a create checks it field by field
    /// (`features::annotation::write::prepare`).
    #[serde(rename = "annotations.create", rename_all = "camelCase")]
    AnnotationsCreate {
        #[serde(with = "crate::identity::page_ref")]
        page: crate::identity::PageObjectNumber,
        data: serde_json::Map<String, serde_json::Value>,
        /// As sent: a value that isn't a whole number is refused by the
        /// create, `InvalidArg`, as `claimObjectNumber` refuses it.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        object_number: Option<serde_json::Number>,
        #[serde(default, skip_serializing_if = "OpResources::is_empty")]
        resources: OpResources,
    },
    /// The annotation `ref` names, changed by `patch` (`AnnotationPatch`),
    /// with the bytes beside it. `expect` is the values it assumes the
    /// annotation reads now. The patch stays the JSON object the caller
    /// sent: an update checks it against the annotation
    /// (`features::annotation::write::patch`).
    #[serde(rename = "annotations.update", rename_all = "camelCase")]
    AnnotationsUpdate {
        #[serde(rename = "ref")]
        reference: crate::identity::AnnotationRef,
        patch: serde_json::Map<String, serde_json::Value>,
        #[serde(default, skip_serializing_if = "OpResources::is_empty")]
        resources: OpResources,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expect: Option<serde_json::Map<String, serde_json::Value>>,
    },
    /// The annotation `ref` names, with its thread and popups. `expect` is
    /// the values it assumes the annotation reads now.
    #[serde(rename = "annotations.delete", rename_all = "camelCase")]
    AnnotationsDelete {
        #[serde(rename = "ref")]
        reference: crate::identity::AnnotationRef,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expect: Option<serde_json::Map<String, serde_json::Value>>,
    },
    /// Stacking order: the annotations go together to `position` among the
    /// page's annotations.
    #[serde(rename = "annotations.reorder", rename_all = "camelCase")]
    AnnotationsReorder {
        #[serde(with = "crate::identity::page_ref")]
        page: crate::identity::PageObjectNumber,
        refs: Vec<crate::identity::AnnotationRef>,
        position: crate::mutation::list_position::AnnotationPosition,
    },
    /// Every page gets `rotation`, its display rotation from now on. Kept
    /// as sent: a value that isn't 0, 90, 180 or 270 is the op's
    /// `InvalidArg`, as `PagesMutator.rotate` refuses it.
    #[serde(rename = "pages.rotate")]
    PagesRotate { pages: Vec<PageRef>, rotation: serde_json::Number },
    /// The pages go together, in the order given, to `position`.
    #[serde(rename = "pages.reorder")]
    PagesReorder { pages: Vec<PageRef>, position: crate::features::pages_write::PagePosition },
    /// The pages go, with their annotations. A document keeps at least one
    /// page.
    #[serde(rename = "pages.delete")]
    PagesDelete { pages: Vec<PageRef> },
    /// `spec.count` blank pages at `position` (the end when left out), each
    /// at the number `objectNumbers` gives it, in order, when it gives one.
    /// The spec stays the object sent: the op checks it as
    /// `PagesInserter.insertBlank` does.
    #[serde(rename = "pages.insertBlank", rename_all = "camelCase")]
    PagesInsertBlank {
        spec: serde_json::Map<String, serde_json::Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        position: Option<crate::features::pages_write::PagePosition>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        object_numbers: Option<Vec<serde_json::Number>>,
    },
    /// Every page of the PDF `source`, copied in at `position` (the end
    /// when left out). The source is a payload the request carries,
    /// `{ "payload": n }`, as every other byte resource on this wire: upstream's
    /// typed array, moved beside the JSON rather than spelled in it.
    #[serde(rename = "pages.insert")]
    PagesInsert {
        source: ResourceBytes,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        position: Option<crate::features::pages_write::PagePosition>,
    },
    /// A bundle's annotations, created as one change (`AnnotationImportOp`).
    /// Only its verb, `annotations.import`, and a server's change make one:
    /// `document.apply` takes no import ([`assert_no_import`]). The bundle
    /// is page space whichever space the change runs in: the import
    /// measures each create on the page it goes to.
    #[serde(rename = "annotations.import", rename_all = "camelCase")]
    AnnotationsImport {
        bundle: WireBundle,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pages: Option<crate::features::transfer::BundleImportPages>,
        attribution: Attribution,
        /// Who `stamp` attributes each annotation to, and whose user
        /// `restore` records as `importedBy`.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        actor: Option<crate::features::annotation::write::draft::AnnotationActor>,
        #[serde(default)]
        limits: crate::features::transfer::BundleLimits,
    },
    /// A bundle's fields, copied in as one change (`FormImportOp`, the
    /// design). Only its verb and a server's change make one.
    #[serde(rename = "forms.import", rename_all = "camelCase")]
    FormsImport {
        bundle: WireBundle,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pages: Option<crate::features::transfer::BundleImportPages>,
        attribution: Attribution,
        /// Whether each field gets the bundle's value; `true` when absent.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        values: Option<bool>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        actor: Option<crate::features::annotation::write::draft::AnnotationActor>,
        #[serde(default)]
        limits: crate::features::transfer::BundleLimits,
        /// Whether the caller holds `doc.forms.script`, which the verb
        /// checked: without it, scripts, submits and links are left out.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        may_script: Option<bool>,
    },
    /// A bundle's values, filled into the fields of the same name, as one
    /// change (`FormImportOp`, the values).
    #[serde(rename = "forms.importValues", rename_all = "camelCase")]
    FormsImportValues {
        bundle: WireBundle,
        attribution: Attribution,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        actor: Option<crate::features::annotation::write::draft::AnnotationActor>,
        #[serde(default)]
        limits: crate::features::transfer::BundleLimits,
    },
    /// The field's value, as a read of it returns one (`FormFieldValue`).
    /// `expect` is the value it assumes the field holds now.
    #[serde(rename = "forms.setValue")]
    FormsSetValue {
        field: FormFieldRef,
        value: FormFieldValue,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expect: Option<FormFieldValue>,
    },
    /// A script's visibility effect on every widget of the field.
    #[serde(rename = "forms.setDisplay")]
    FormsSetDisplay {
        field: FormFieldRef,
        display: crate::features::forms::write::FormFieldDisplay,
    },
    /// A script's formatted text, drawn without changing the value.
    #[serde(rename = "forms.setAppearanceText")]
    FormsSetAppearanceText { field: FormFieldRef, text: String },
    /// The fields back to their defaults; every field without `fields`.
    #[serde(rename = "forms.reset")]
    FormsReset {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        fields: Option<Vec<FormFieldRef>>,
    },
    /// A field from its draft (`FormFieldDraft`), at `objectNumber` and its
    /// widgets at `widgetObjectNumbers`, in `draft.widgets` order, when
    /// given. The draft stays the object sent: its places convert as it
    /// runs, and the create reads it.
    #[serde(rename = "forms.create", rename_all = "camelCase")]
    FormsCreate {
        draft: serde_json::Map<String, serde_json::Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        object_number: Option<serde_json::Number>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        widget_object_numbers: Option<Vec<serde_json::Number>>,
    },
    /// The settings `patch` names (`FormFieldPatch`). `expect` is the
    /// properties it assumes the field has now.
    #[serde(rename = "forms.update")]
    FormsUpdate {
        field: FormFieldRef,
        patch: serde_json::Map<String, serde_json::Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expect: Option<serde_json::Map<String, serde_json::Value>>,
    },
    /// The field, its widgets, and the parents it leaves empty.
    #[serde(rename = "forms.delete")]
    FormsDelete {
        field: FormFieldRef,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expect: Option<serde_json::Map<String, serde_json::Value>>,
    },
    /// One more widget for the field, where `placement` says, at
    /// `objectNumber` when given; a merged field's widget half moves to
    /// `splitObjectNumber`.
    #[serde(rename = "forms.addWidget", rename_all = "camelCase")]
    FormsAddWidget {
        field: FormFieldRef,
        placement: serde_json::Map<String, serde_json::Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        object_number: Option<serde_json::Number>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        split_object_number: Option<serde_json::Number>,
    },
    /// The widget leaves the field, and stays on its page.
    #[serde(rename = "forms.removeWidget")]
    FormsRemoveWidget { field: FormFieldRef, widget: crate::identity::AnnotationRef },
    /// The widget leaves its page, and its field when it has one.
    #[serde(rename = "forms.deleteWidget")]
    FormsDeleteWidget {
        widget: crate::identity::AnnotationRef,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expect: Option<serde_json::Map<String, serde_json::Value>>,
    },
    /// The calculation order: the fields go together to `position` in it.
    #[serde(rename = "forms.reorderCalculations")]
    FormsReorderCalculations {
        fields: Vec<FormFieldRef>,
        position: crate::features::forms::field::FieldPosition,
    },
    /// Stacking order: the widgets go together to `position` among the
    /// page's widgets.
    #[serde(rename = "forms.reorderWidgets")]
    FormsReorderWidgets {
        #[serde(with = "crate::identity::page_ref")]
        page: crate::identity::PageObjectNumber,
        widgets: Vec<crate::identity::AnnotationRef>,
        position: crate::mutation::list_position::AnnotationPosition,
    },
    /// A widget's place and look: what `annotations.update` takes for a
    /// widget (`WidgetPatch`). The patch stays the object sent.
    #[serde(rename = "forms.updateWidget")]
    FormsUpdateWidget {
        widget: crate::identity::AnnotationRef,
        patch: serde_json::Map<String, serde_json::Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expect: Option<serde_json::Map<String, serde_json::Value>>,
    },
    /// A one-page PDF drawn into every widget of an unsigned signature
    /// field (`SignatureAppearanceInput`); the PDF is a request payload,
    /// `{ "pdf": { "payload": n } }`.
    #[serde(rename = "forms.setSignatureAppearance")]
    FormsSetSignatureAppearance { field: FormFieldRef, appearance: SignatureAppearance },
    /// A new outline item under `parent` (`None`: the top level), at
    /// `position` among its children (the end when left out), at
    /// `objectNumber` when given. The draft (`OutlineItemDraft`) stays the
    /// object sent: the create checks it as it runs.
    #[serde(rename = "outline.create", rename_all = "camelCase")]
    OutlineCreate {
        parent: Option<OutlineItemRef>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        position: Option<OutlinePosition>,
        data: serde_json::Map<String, serde_json::Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        object_number: Option<serde_json::Number>,
    },
    /// The fields `patch` names (`OutlineItemPatch`). `expect` is the
    /// values it assumes the item reads now.
    #[serde(rename = "outline.update")]
    OutlineUpdate {
        #[serde(rename = "ref")]
        reference: OutlineItemRef,
        patch: serde_json::Map<String, serde_json::Value>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expect: Option<serde_json::Map<String, serde_json::Value>>,
    },
    /// The items go together, with the items under them, under `parent` at
    /// `position` among its children.
    #[serde(rename = "outline.move")]
    OutlineMove {
        refs: Vec<OutlineItemRef>,
        parent: Option<OutlineItemRef>,
        position: OutlinePosition,
    },
    /// The item and the items under it.
    #[serde(rename = "outline.delete")]
    OutlineDelete {
        #[serde(rename = "ref")]
        reference: OutlineItemRef,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        expect: Option<serde_json::Map<String, serde_json::Value>>,
    },
}

/// Whose an import's rows are (`attribution`): `restore` writes the
/// authors, dates and names the bundle has, and takes an import's rights;
/// `stamp` makes them the session's, as a create does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Attribution {
    #[default]
    Restore,
    Stamp,
}

/// A bundle as a request carries it (`WireAnnotationBundle`,
/// `WireFormBundle`): its manifest as the JSON sent, and each resource a
/// payload of the request, `{ "payload": n }`, until the host puts the
/// bytes in ([`resolve_payloads`]).
#[derive(Debug, Clone, PartialEq)]
pub struct WireBundle {
    /// Every key but `resources`, in the order sent.
    pub manifest: serde_json::Map<String, serde_json::Value>,
    pub resources: Vec<(String, ResourceBytes)>,
}

impl Serialize for WireBundle {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut map = s.serialize_map(Some(self.manifest.len() + 1))?;
        for (key, value) in &self.manifest {
            map.serialize_entry(key, value)?;
        }
        let resources: Vec<(&String, &ResourceBytes)> =
            self.resources.iter().map(|(id, bytes)| (id, bytes)).collect();
        map.serialize_entry("resources", &ResourceMap(&resources))?;
        map.end()
    }
}

/// The resources of a [`WireBundle`], serialized in their order.
struct ResourceMap<'a>(&'a [(&'a String, &'a ResourceBytes)]);

impl Serialize for ResourceMap<'_> {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_map(self.0.iter().map(|(id, bytes)| (*id, *bytes)))
    }
}

impl<'de> Deserialize<'de> for WireBundle {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        use serde::de::Error;
        let serde_json::Value::Object(mut manifest) = serde_json::Value::deserialize(d)? else {
            return Err(D::Error::custom("a bundle is an object"));
        };
        let resources = match manifest.remove("resources") {
            None | Some(serde_json::Value::Null) => Vec::new(),
            Some(serde_json::Value::Object(resources)) => resources
                .into_iter()
                .map(|(id, bytes)| {
                    ResourceBytes::deserialize(bytes)
                        .map(|bytes| (id.clone(), bytes))
                        .map_err(|_| D::Error::custom(format!("resource {id} is not bytes")))
                })
                .collect::<Result<_, _>>()?,
            Some(_) => return Err(D::Error::custom("a bundle's resources are an object")),
        };
        Ok(Self { manifest, resources })
    }
}

impl WireBundle {
    /// Each resource's bytes from the request's payloads.
    pub fn resolve(&mut self, payloads: &[Vec<u8>]) -> Result<(), EngineError> {
        for (id, slot) in &mut self.resources {
            if let ResourceBytes::Payload(index) = slot {
                let bytes = payloads.get(*index).ok_or_else(|| {
                    malformed(format!(
                        "resource {id} names payload {index}: the request carries {}",
                        payloads.len()
                    ))
                })?;
                *slot = ResourceBytes::Bytes(bytes.clone());
            }
        }
        Ok(())
    }

    /// The bundle, its resources as bytes.
    pub fn bundle(&self) -> Result<crate::features::transfer::Bundle, EngineError> {
        Ok(crate::features::transfer::Bundle {
            manifest: self.manifest.clone(),
            resources: self
                .resources
                .iter()
                .map(|(id, bytes)| Ok((id.clone(), bytes.bytes()?.to_vec())))
                .collect::<Result<_, EngineError>>()?,
        })
    }
}

/// The bytes beside an op, by role (`WireAnnotationResources`).
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
pub struct OpResources {
    /// What a stamp shows: PNG, JPEG or a one-page PDF.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub appearance: Option<ResourceBytes>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<ResourceBytes>,
}

impl OpResources {
    pub fn is_empty(&self) -> bool {
        self.appearance.is_none() && self.file.is_none()
    }

    /// Each role's bytes from the request's payloads, by the payload each
    /// names.
    pub fn resolve(&mut self, payloads: &[Vec<u8>]) -> Result<(), EngineError> {
        for slot in [&mut self.appearance, &mut self.file] {
            if let Some(ResourceBytes::Payload(index)) = slot {
                let bytes = payloads.get(*index).ok_or_else(|| {
                    malformed(format!(
                        "resource payload {index} is missing: the request carries {}",
                        payloads.len()
                    ))
                })?;
                *slot = Some(ResourceBytes::Bytes(bytes.clone()));
            }
        }
        Ok(())
    }
}

/// `SignatureAppearanceInput`: the one-page PDF a visual fill draws.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SignatureAppearance {
    pub pdf: ResourceBytes,
}

/// One resource's bytes. On this wire a resource names the request payload
/// that holds it, `{ "payload": n }`, as an annotation write's resources do;
/// the host puts the bytes in before the change runs or is fingerprinted
/// ([`resolve_payloads`]), and they then fingerprint as upstream's typed
/// array does, `{ "$bytes": ... }`.
#[derive(Debug, Clone, PartialEq)]
pub enum ResourceBytes {
    Payload(usize),
    Bytes(Vec<u8>),
}

impl Serialize for ResourceBytes {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut map = s.serialize_map(Some(1))?;
        match self {
            Self::Payload(index) => map.serialize_entry("payload", index)?,
            Self::Bytes(bytes) => {
                map.serialize_entry("$bytes", &super::fingerprint::bytes_token(bytes))?;
            }
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for ResourceBytes {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct PayloadRef {
            payload: usize,
        }
        Ok(Self::Payload(PayloadRef::deserialize(d)?.payload))
    }
}

impl ResourceBytes {
    /// The bytes, once the host put them in.
    pub fn bytes(&self) -> Result<&[u8], EngineError> {
        match self {
            Self::Bytes(bytes) => Ok(bytes),
            Self::Payload(index) => {
                Err(malformed(format!("resource payload {index} was never put in")))
            }
        }
    }
}

/// Put each op's resource bytes in from the request's payloads, by the
/// payload each names.
pub fn resolve_payloads(change: &mut Change, payloads: &[Vec<u8>]) -> Result<(), EngineError> {
    let Change::Ops(ops) = change else {
        return Ok(());
    };
    for op in ops {
        match op {
            ChangeOp::AttachmentsCreate { resources, .. }
            | ChangeOp::AnnotationsCreate { resources, .. }
            | ChangeOp::AnnotationsUpdate { resources, .. } => resources.resolve(payloads)?,
            ChangeOp::PagesInsert { source, .. } => {
                if let ResourceBytes::Payload(index) = source {
                    let bytes = payloads.get(*index).ok_or_else(|| {
                        malformed(format!(
                            "source payload {index} is missing: the request carries {}",
                            payloads.len()
                        ))
                    })?;
                    *source = ResourceBytes::Bytes(bytes.clone());
                }
            }
            ChangeOp::AnnotationsImport { bundle, .. }
            | ChangeOp::FormsImport { bundle, .. }
            | ChangeOp::FormsImportValues { bundle, .. } => bundle.resolve(payloads)?,
            ChangeOp::FormsSetSignatureAppearance { appearance, .. } => {
                if let ResourceBytes::Payload(index) = appearance.pdf {
                    let bytes = payloads.get(index).ok_or_else(|| {
                        malformed(format!(
                            "appearance payload {index} is missing: the request carries {}",
                            payloads.len()
                        ))
                    })?;
                    appearance.pdf = ResourceBytes::Bytes(bytes.clone());
                }
            }
            ChangeOp::MetadataUpdate { .. }
            | ChangeOp::MetadataUpdateCustom { .. }
            | ChangeOp::AttachmentsDelete { .. }
            | ChangeOp::AnnotationsDelete { .. }
            | ChangeOp::AnnotationsReorder { .. }
            | ChangeOp::PagesRotate { .. }
            | ChangeOp::PagesReorder { .. }
            | ChangeOp::PagesDelete { .. }
            | ChangeOp::PagesInsertBlank { .. }
            | ChangeOp::FormsSetValue { .. }
            | ChangeOp::FormsSetDisplay { .. }
            | ChangeOp::FormsSetAppearanceText { .. }
            | ChangeOp::FormsReset { .. }
            | ChangeOp::FormsCreate { .. }
            | ChangeOp::FormsUpdate { .. }
            | ChangeOp::FormsDelete { .. }
            | ChangeOp::FormsAddWidget { .. }
            | ChangeOp::FormsRemoveWidget { .. }
            | ChangeOp::FormsDeleteWidget { .. }
            | ChangeOp::FormsReorderCalculations { .. }
            | ChangeOp::FormsReorderWidgets { .. }
            | ChangeOp::FormsUpdateWidget { .. }
            | ChangeOp::OutlineCreate { .. }
            | ChangeOp::OutlineUpdate { .. }
            | ChangeOp::OutlineMove { .. }
            | ChangeOp::OutlineDelete { .. } => {}
        }
    }
    Ok(())
}

/// A patch's keys in wire order. Compares equal whatever it holds, so two
/// ops with the same patch are the same op.
#[derive(Debug, Clone, Default)]
pub struct PatchKeys(pub Vec<String>);

impl PartialEq for PatchKeys {
    fn eq(&self, _: &Self) -> bool {
        true
    }
}

impl PatchKeys {
    /// The keys of `op`'s `patch` object, in the order they arrived.
    fn of(op: &serde_json::Value) -> Self {
        Self(
            op.get("patch")
                .and_then(serde_json::Value::as_object)
                .map(|patch| patch.keys().cloned().collect())
                .unwrap_or_default(),
        )
    }
}

impl ChangeOp {
    /// The op's `type`.
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::MetadataUpdate { .. } => "metadata.update",
            Self::MetadataUpdateCustom { .. } => "metadata.updateCustom",
            Self::AttachmentsCreate { .. } => "attachments.create",
            Self::AttachmentsDelete { .. } => "attachments.delete",
            Self::AnnotationsCreate { .. } => "annotations.create",
            Self::AnnotationsUpdate { .. } => "annotations.update",
            Self::AnnotationsDelete { .. } => "annotations.delete",
            Self::AnnotationsReorder { .. } => "annotations.reorder",
            Self::PagesRotate { .. } => "pages.rotate",
            Self::PagesReorder { .. } => "pages.reorder",
            Self::PagesDelete { .. } => "pages.delete",
            Self::PagesInsertBlank { .. } => "pages.insertBlank",
            Self::PagesInsert { .. } => "pages.insert",
            Self::AnnotationsImport { .. } => "annotations.import",
            Self::FormsImport { .. } => "forms.import",
            Self::FormsImportValues { .. } => "forms.importValues",
            Self::FormsSetValue { .. } => "forms.setValue",
            Self::FormsSetDisplay { .. } => "forms.setDisplay",
            Self::FormsSetAppearanceText { .. } => "forms.setAppearanceText",
            Self::FormsReset { .. } => "forms.reset",
            Self::FormsCreate { .. } => "forms.create",
            Self::FormsUpdate { .. } => "forms.update",
            Self::FormsDelete { .. } => "forms.delete",
            Self::FormsAddWidget { .. } => "forms.addWidget",
            Self::FormsRemoveWidget { .. } => "forms.removeWidget",
            Self::FormsDeleteWidget { .. } => "forms.deleteWidget",
            Self::FormsReorderCalculations { .. } => "forms.reorderCalculations",
            Self::FormsReorderWidgets { .. } => "forms.reorderWidgets",
            Self::FormsUpdateWidget { .. } => "forms.updateWidget",
            Self::FormsSetSignatureAppearance { .. } => "forms.setSignatureAppearance",
            Self::OutlineCreate { .. } => "outline.create",
            Self::OutlineUpdate { .. } => "outline.update",
            Self::OutlineMove { .. } => "outline.move",
            Self::OutlineDelete { .. } => "outline.delete",
        }
    }
}

/// A change as its typed ops spell it, what both `document.apply` and a
/// single verb fingerprint, so the two agree on what "the same change" is.
pub fn change_value(change: &Change) -> Result<serde_json::Value, EngineError> {
    match change {
        Change::UndoOf(op_id) => Ok(serde_json::json!({ "undoOf": op_id })),
        Change::Ops(ops) => {
            let ops = serde_json::to_value(ops).map_err(|e| malformed(e.to_string()))?;
            Ok(serde_json::json!({ "ops": ops }))
        }
    }
}

/// The op types of upstream's `ChangeOp` union this engine does not run yet,
/// each with the sub-task that registers it. The one list the owners look
/// in: each removes its rows when it adds its variant to [`ChangeOp`].
/// Empty since 8.26 registered `forms.setSignatureAppearance`, the last.
pub const OP_OWNERS: &[(&str, &str)] = &[];

/// The imports `document.apply` refuses, with the verb that runs each
/// (`assertNoImport`).
const IMPORT_VERBS: &[(&str, &str)] = &[
    ("annotations.import", "doc.annotations.import"),
    ("forms.import", "doc.forms.import"),
    ("forms.importValues", "doc.forms.importValues"),
];

/// `change` as the wire spells it: `{ ops }` or `{ undoOf }`. Every op is
/// read before any runs, so a change naming an op this engine does not run,
/// or an op of the wrong shape, is refused whole.
pub fn parse_change(change: &serde_json::Value) -> Result<Change, EngineError> {
    let object = change.as_object().ok_or_else(|| malformed("a change is an object"))?;
    match (object.get("ops"), object.get("undoOf")) {
        (Some(ops), None) => parse_ops(ops).map(Change::Ops),
        (None, Some(serde_json::Value::String(op_id))) => Ok(Change::UndoOf(op_id.clone())),
        (None, Some(_)) => Err(malformed("undoOf is an opId")),
        _ => Err(malformed("a change has exactly one of ops or undoOf")),
    }
}

/// A change's ops, in order.
pub fn parse_ops(ops: &serde_json::Value) -> Result<Vec<ChangeOp>, EngineError> {
    let ops = ops.as_array().ok_or_else(|| malformed("ops is a list"))?;
    ops.iter().enumerate().map(|(at, op)| parse_op(at, op)).collect()
}

/// `document.apply` takes no import (`assertNoImport`, which only
/// `handleDocumentApply` runs): an import is its own verb's one-op change,
/// which alone may write restored attribution. A server request's change
/// may hold one (`document.applyChanges`).
pub fn assert_no_import(change: &serde_json::Value) -> Result<(), EngineError> {
    let Some(ops) = change.get("ops").and_then(serde_json::Value::as_array) else {
        return Ok(());
    };
    if let Some((_, verb)) = ops.iter().find_map(|op| {
        let kind = op.get("type").and_then(serde_json::Value::as_str)?;
        IMPORT_VERBS.iter().find(|(import, _)| *import == kind)
    }) {
        return Err(EngineError::with_message(
            EngineErrorCode::InvalidArg,
            format!("doc.apply takes no import: import a bundle with {verb}"),
        ));
    }
    Ok(())
}

/// One op as the wire spells it, its patch's keys kept in wire order.
pub fn parse_op(at: usize, op: &serde_json::Value) -> Result<ChangeOp, EngineError> {
    let kind = op
        .get("type")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| malformed(format!("op {at} has no type")))?;
    if let Some((_, owner)) = OP_OWNERS.iter().find(|(name, _)| *name == kind) {
        return Err(EngineError::with_message(
            EngineErrorCode::NotImplemented,
            format!("the {kind} op is not ported yet (Task {owner})"),
        )
        .with_details(serde_json::json!({ "opIndex": at, "type": kind })));
    }
    let mut parsed: ChangeOp = serde_json::from_value(op.clone())
        .map_err(|e| malformed(format!("op {at} ({kind}) is not the shape it takes: {e}")))?;
    match &mut parsed {
        ChangeOp::MetadataUpdate { keys, .. } | ChangeOp::MetadataUpdateCustom { keys, .. } => {
            *keys = PatchKeys::of(op);
        }
        ChangeOp::AttachmentsCreate { .. }
        | ChangeOp::AttachmentsDelete { .. }
        | ChangeOp::AnnotationsCreate { .. }
        | ChangeOp::AnnotationsImport { .. }
        | ChangeOp::FormsImport { .. }
        | ChangeOp::FormsImportValues { .. }
        | ChangeOp::AnnotationsUpdate { .. }
        | ChangeOp::AnnotationsDelete { .. }
        | ChangeOp::AnnotationsReorder { .. }
        | ChangeOp::PagesRotate { .. }
        | ChangeOp::PagesReorder { .. }
        | ChangeOp::PagesDelete { .. }
        | ChangeOp::PagesInsertBlank { .. }
        | ChangeOp::PagesInsert { .. }
        | ChangeOp::FormsSetValue { .. }
        | ChangeOp::FormsSetDisplay { .. }
        | ChangeOp::FormsSetAppearanceText { .. }
        | ChangeOp::FormsReset { .. }
        | ChangeOp::FormsCreate { .. }
        | ChangeOp::FormsUpdate { .. }
        | ChangeOp::FormsDelete { .. }
        | ChangeOp::FormsAddWidget { .. }
        | ChangeOp::FormsRemoveWidget { .. }
        | ChangeOp::FormsDeleteWidget { .. }
        | ChangeOp::FormsReorderCalculations { .. }
        | ChangeOp::FormsReorderWidgets { .. }
        | ChangeOp::FormsUpdateWidget { .. }
        | ChangeOp::FormsSetSignatureAppearance { .. }
        | ChangeOp::OutlineCreate { .. }
        | ChangeOp::OutlineUpdate { .. }
        | ChangeOp::OutlineMove { .. }
        | ChangeOp::OutlineDelete { .. } => {}
    }
    Ok(parsed)
}

fn malformed(message: impl Into<String>) -> EngineError {
    EngineError::with_message(EngineErrorCode::WireFormat, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_change_is_its_ops_or_an_undo() {
        let ops = parse_change(&serde_json::json!({ "ops": [
            { "type": "metadata.update", "patch": { "title": "T" } },
            { "type": "metadata.updateCustom", "patch": { "Department": null } },
        ] }))
        .unwrap();
        let Change::Ops(ops) = ops else { panic!("ops") };
        assert_eq!(
            ops.iter().map(ChangeOp::type_name).collect::<Vec<_>>(),
            ["metadata.update", "metadata.updateCustom"]
        );
        assert_eq!(
            parse_change(&serde_json::json!({ "undoOf": "op-1" })).unwrap(),
            Change::UndoOf("op-1".into())
        );
        for bad in [
            serde_json::json!({}),
            serde_json::json!({ "ops": [], "undoOf": "op-1" }),
            serde_json::json!({ "undoOf": 3 }),
            serde_json::json!("x"),
        ] {
            assert_eq!(parse_change(&bad).unwrap_err().code, EngineErrorCode::WireFormat, "{bad}");
        }
    }

    #[test]
    fn an_op_of_the_wrong_shape_refuses_the_whole_change() {
        let refused = parse_ops(&serde_json::json!([
            { "type": "metadata.update", "patch": {} },
            { "type": "forms.setSignatureAppearance" },
        ]))
        .unwrap_err();
        assert_eq!(refused.code, EngineErrorCode::WireFormat);
        assert!(refused.message.contains("op 1"), "{refused}");
    }

    #[test]
    fn a_signature_appearance_takes_its_pdf_from_a_payload() {
        let mut change = parse_change(&serde_json::json!({ "ops": [
            { "type": "forms.setSignatureAppearance",
              "field": { "kind": "fqn", "name": "sig" },
              "appearance": { "pdf": { "payload": 1 } } },
        ] }))
        .unwrap();
        resolve_payloads(&mut change, &[b"a".to_vec(), b"%PDF".to_vec()]).unwrap();
        let Change::Ops(ops) = &change else { panic!("ops") };
        let ChangeOp::FormsSetSignatureAppearance { appearance, .. } = &ops[0] else {
            panic!("{:?}", ops[0])
        };
        assert_eq!(appearance.pdf.bytes().unwrap(), b"%PDF");
        let refused = resolve_payloads(
            &mut parse_change(&serde_json::json!({ "ops": [
                { "type": "forms.setSignatureAppearance",
                  "field": { "kind": "fqn", "name": "sig" },
                  "appearance": { "pdf": { "payload": 3 } } },
            ] }))
            .unwrap(),
            &[],
        )
        .unwrap_err();
        assert_eq!(refused.code, EngineErrorCode::WireFormat);
    }

    #[test]
    fn document_apply_refuses_an_import_naming_its_verb_and_a_server_change_reads_it() {
        let change = serde_json::json!({ "ops": [{ "type": "forms.importValues",
            "bundle": { "format": "embedpdf/form", "resources": { "sha256-x": { "payload": 0 } } },
            "attribution": "stamp" }] });
        let refused = assert_no_import(&change).unwrap_err();
        assert_eq!(refused.code, EngineErrorCode::InvalidArg);
        assert!(refused.message.contains("doc.forms.importValues"), "{refused}");
        let mut parsed = parse_change(&change).unwrap();
        resolve_payloads(&mut parsed, &[b"bytes".to_vec()]).unwrap();
        let Change::Ops(ops) = &parsed else { panic!("ops") };
        let ChangeOp::FormsImportValues { bundle, attribution, .. } = &ops[0] else {
            panic!("import")
        };
        assert_eq!(*attribution, Attribution::Stamp);
        assert_eq!(
            bundle.bundle().unwrap().resources,
            [("sha256-x".to_owned(), b"bytes".to_vec())]
        );
        // Fingerprinted with its bytes, its manifest keys in the order sent.
        let value = change_value(&parsed).unwrap();
        let keys: Vec<&String> = value["ops"][0]["bundle"].as_object().unwrap().keys().collect();
        assert_eq!(keys, ["format", "resources"]);
        assert!(value["ops"][0]["bundle"]["resources"]["sha256-x"]["$bytes"].is_string());
    }

    #[test]
    fn a_patch_keeps_its_wire_key_order() {
        let ops = parse_ops(&serde_json::json!([
            { "type": "metadata.update", "patch": { "subject": "S", "author": "A", "title": "T" } },
            { "type": "metadata.updateCustom", "patch": { "Zed": "z", "Alpha": null } },
        ]))
        .unwrap();
        let ChangeOp::MetadataUpdate { keys, .. } = &ops[0] else { panic!("update") };
        assert_eq!(keys.0, ["subject", "author", "title"]);
        let ChangeOp::MetadataUpdateCustom { keys, .. } = &ops[1] else { panic!("custom") };
        assert_eq!(keys.0, ["Zed", "Alpha"]);
    }

    #[test]
    fn an_unknown_or_malformed_op_is_a_wire_format_error() {
        for op in [
            serde_json::json!({ "type": "metadata.rename" }),
            serde_json::json!({ "type": "metadata.update" }),
            serde_json::json!({ "type": "metadata.update", "patch": { "custom": {} } }),
            serde_json::json!({ "patch": {} }),
        ] {
            let refused = parse_ops(&serde_json::json!([op])).unwrap_err();
            assert_eq!(refused.code, EngineErrorCode::WireFormat, "{op}");
        }
    }

    #[test]
    fn an_attachment_op_takes_its_bytes_from_the_payload_it_names() {
        let mut change = parse_change(&serde_json::json!({ "ops": [
            { "type": "attachments.create", "file": { "name": "a.txt" },
              "resources": { "file": { "payload": 1 } } },
            { "type": "attachments.delete", "ref": { "kind": "key", "key": "b.txt" } },
        ] }))
        .unwrap();
        let unresolved = change_value(&change).unwrap();
        assert_eq!(unresolved["ops"][0]["resources"]["file"], serde_json::json!({ "payload": 1 }));
        resolve_payloads(&mut change, &[Vec::new(), b"hello".to_vec()]).unwrap();
        let Change::Ops(ops) = &change else { panic!("ops") };
        let ChangeOp::AttachmentsCreate { resources, .. } = &ops[0] else { panic!("create") };
        assert_eq!(resources.file.as_ref().unwrap().bytes().unwrap(), b"hello");
        // Fingerprinted as upstream's typed array: `$bytes`, length and hash.
        let resolved = change_value(&change).unwrap();
        assert_eq!(
            resolved["ops"][0]["resources"]["file"],
            serde_json::json!({ "$bytes": "5-19jqrdkps12" })
        );
        assert_eq!(resolved["ops"][1]["ref"], serde_json::json!({ "kind": "key", "key": "b.txt" }));

        let mut missing = parse_change(&serde_json::json!({ "ops": [
            { "type": "attachments.create", "file": { "name": "a.txt" },
              "resources": { "file": { "payload": 2 } } },
        ] }))
        .unwrap();
        let refused = resolve_payloads(&mut missing, &[]).unwrap_err();
        assert_eq!(refused.code, EngineErrorCode::WireFormat);
    }

    #[test]
    fn every_owned_op_is_named_once() {
        let mut names: Vec<&str> = OP_OWNERS.iter().map(|(name, _)| *name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), OP_OWNERS.len());
        for ported in [
            "metadata.update",
            "metadata.updateCustom",
            "attachments.create",
            "attachments.delete",
            "annotations.create",
            "annotations.update",
            "annotations.delete",
            "annotations.reorder",
            "pages.rotate",
            "pages.reorder",
            "pages.delete",
            "pages.insertBlank",
            "pages.insert",
            "forms.setValue",
            "forms.setDisplay",
            "forms.setAppearanceText",
            "forms.reset",
            "forms.create",
            "forms.update",
            "forms.delete",
            "forms.addWidget",
            "forms.removeWidget",
            "forms.deleteWidget",
            "forms.reorderCalculations",
            "forms.reorderWidgets",
            "forms.updateWidget",
            "annotations.import",
            "forms.import",
            "forms.importValues",
            "outline.create",
            "outline.update",
            "outline.move",
            "outline.delete",
            "forms.setSignatureAppearance",
        ] {
            assert!(!names.contains(&ported), "{ported} is ported and still listed");
        }
    }
}
