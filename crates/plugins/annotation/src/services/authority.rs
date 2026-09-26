//! `services/authority.ts`: the engine's own collab resolver, mirrored.
//! `canCreate` asks about the caller's identity; the mutation checks ask
//! about the target's stamped owner, built from the record's EMBD metadata.

use epdf_annotation_core::{AnnotationRef, PageRef, annot_deletable, annot_transformable};
use epdf_kernel::Error;
use epdf_kernel::error::{code, plugin_error};
use serde_json::{Map, Value, json};

use crate::controller::AnnotationCapability;
use crate::repository::annotation_key;
use crate::value::get;

/// `{userId?, groupId?}` of a DTO: the stamped owner the resolver checks.
pub(crate) fn owner_of(dto: Option<&Value>) -> Value {
    let mut out = Map::new();
    if let Some(d) = dto {
        for key in ["userId", "groupId"] {
            if let Some(v) = get(d, key) {
                out.insert(key.into(), v.clone());
            }
        }
    }
    Value::Object(out)
}

impl AnnotationCapability {
    /// `canRead()`: `doc.annotate.read`, allowed without a document.
    pub fn can_read(&self) -> bool {
        self.doc().is_none_or(|d| d.security().allows("doc.annotate.read"))
    }

    /// `canCreate()`: the caller's own create authority.
    pub fn can_create(&self) -> bool {
        self.doc().is_some_and(|d| d.security().allows_annotation_create())
    }

    /// `allowsMutation(action, ref)`: against the target's stamped owner.
    pub(crate) fn allows_mutation(&self, action: &str, r: &AnnotationRef) -> bool {
        let Some(doc) = self.doc() else { return false };
        let m = self.model();
        let target = owner_of(m.by_id.get(&annotation_key(r)).and_then(|a| a.data.as_ref()));
        doc.security().allows_annotation_mutation(action, &target)
    }

    /// `canEdit(ref)`: authority and flags, the fused transform predicate.
    pub fn can_edit(&self, r: &AnnotationRef) -> bool {
        self.model().by_id.get(&annotation_key(r)).is_some_and(annot_transformable)
    }

    /// `canDelete(ref)`.
    pub fn can_delete(&self, r: &AnnotationRef) -> bool {
        self.model().by_id.get(&annotation_key(r)).is_some_and(annot_deletable)
    }

    /// `assertCreate()`.
    pub(crate) fn assert_create(&self) -> Result<(), Error> {
        if self.can_create() {
            return Ok(());
        }
        Err(plugin_error(
            code::PERMISSION_DENIED,
            "annotation",
            "create requires doc.annotate.create",
            None,
            Some(json!({ "required": "doc.annotate.create" })),
        ))
    }

    /// `assertPage(page)`.
    pub(crate) fn assert_page(&self, page: PageRef) -> Result<(), Error> {
        if self.has_page(page.page_object_number) {
            return Ok(());
        }
        Err(plugin_error(
            code::NOT_FOUND,
            "annotation",
            format!("page {} is not in this document", page.page_object_number),
            None,
            None,
        ))
    }
}
