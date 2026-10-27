//! The scope vocabulary.
//!
//! A transcription of `packages/engine/core/src/auth/scope/types.ts`. The
//! doc comments there explain why each capability exists and which PDF bit
//! documents it; they are not repeated here. What is repeated is anything the
//! Rust shape changes.

use std::fmt;

/// Closed set of capability scopes.
///
/// An enum rather than a string newtype, so a capability that does not exist
/// cannot be constructed and a `match` over them is exhaustive. The cost is
/// that adding one means touching [`DocCapability::as_str`] and
/// [`DocCapability::parse`] together, which is the same two-place edit the
/// TypeScript needs between the union and `KNOWN_CAPABILITIES`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DocCapability {
    DocOpen,
    DocRender,
    DocTextSelect,
    DocTextCopy,
    DocTextSearch,
    DocContentCopy,
    DocDownload,
    DocDownloadFlattened,
    DocPrint,
    DocPrintHigh,
    DocPagesModify,
    DocPagesAssemble,
    DocFormsRead,
    DocFormsFill,
    DocFormsModify,
    DocFormsImport,
    DocFormsScript,
    DocFormsSubmit,
    DocAnnotateRead,
    DocAnnotateModify,
    DocAnnotateImport,
    DocMetadataModify,
    DocAttachmentsModify,
    DocOutlineModify,
    DocRedact,
    DocSign,
    DocSignCertify,
}

impl DocCapability {
    /// Every capability, in the order `types.ts` declares them.
    pub const ALL: [DocCapability; 27] = [
        Self::DocOpen,
        Self::DocRender,
        Self::DocTextSelect,
        Self::DocTextCopy,
        Self::DocTextSearch,
        Self::DocContentCopy,
        Self::DocDownload,
        Self::DocDownloadFlattened,
        Self::DocPrint,
        Self::DocPrintHigh,
        Self::DocPagesModify,
        Self::DocPagesAssemble,
        Self::DocFormsRead,
        Self::DocFormsFill,
        Self::DocFormsModify,
        Self::DocFormsImport,
        Self::DocFormsScript,
        Self::DocFormsSubmit,
        Self::DocAnnotateRead,
        Self::DocAnnotateModify,
        Self::DocAnnotateImport,
        Self::DocMetadataModify,
        Self::DocAttachmentsModify,
        Self::DocOutlineModify,
        Self::DocRedact,
        Self::DocSign,
        Self::DocSignCertify,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::DocOpen => "doc.open",
            Self::DocRender => "doc.render",
            Self::DocTextSelect => "doc.text.select",
            Self::DocTextCopy => "doc.text.copy",
            Self::DocTextSearch => "doc.text.search",
            Self::DocContentCopy => "doc.content.copy",
            Self::DocDownload => "doc.download",
            Self::DocDownloadFlattened => "doc.download.flattened",
            Self::DocPrint => "doc.print",
            Self::DocPrintHigh => "doc.print.high",
            Self::DocPagesModify => "doc.pages.modify",
            Self::DocPagesAssemble => "doc.pages.assemble",
            Self::DocFormsRead => "doc.forms.read",
            Self::DocFormsFill => "doc.forms.fill",
            Self::DocFormsModify => "doc.forms.modify",
            Self::DocFormsImport => "doc.forms.import",
            Self::DocFormsScript => "doc.forms.script",
            Self::DocFormsSubmit => "doc.forms.submit",
            Self::DocAnnotateRead => "doc.annotate.read",
            Self::DocAnnotateModify => "doc.annotate.modify",
            Self::DocAnnotateImport => "doc.annotate.import",
            Self::DocMetadataModify => "doc.metadata.modify",
            Self::DocAttachmentsModify => "doc.attachments.modify",
            Self::DocOutlineModify => "doc.outline.modify",
            Self::DocRedact => "doc.redact",
            Self::DocSign => "doc.sign",
            Self::DocSignCertify => "doc.sign.certify",
        }
    }

    /// Membership in the closed set. Shape is checked by the parser first.
    pub fn parse(raw: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|c| c.as_str() == raw)
    }
}

impl fmt::Display for DocCapability {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// What a collab scope is about. Both follow one grammar,
/// `entity:action:filter`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CollabEntity {
    Annotations,
    Fields,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CollabAction {
    Create,
    Update,
    Delete,
    SetGroup,
}

impl CollabAction {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Create => "create",
            Self::Update => "update",
            Self::Delete => "delete",
            Self::SetGroup => "set-group",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        [Self::Create, Self::Update, Self::Delete, Self::SetGroup]
            .into_iter()
            .find(|a| a.as_str() == raw)
    }
}

/// The action slot of a collab scope, which may be the wildcard.
///
/// TypeScript writes this as `CollabAction | '*'`. A separate variant here
/// rather than an `Option`, because "any action" is a value and not an
/// absence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CollabActionPattern {
    Any,
    One(CollabAction),
}

impl CollabActionPattern {
    pub fn covers(self, action: CollabAction) -> bool {
        match self {
            Self::Any => true,
            Self::One(one) => one == action,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum CollabFilter {
    All,
    SelfOnly,
    CreatedBy(String),
    Group(String),
}

impl CollabFilter {
    /// The discriminant name, spelled as `errors.ts` spells it in the
    /// `set-group` rejection message.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::All => "all",
            Self::SelfOnly => "self",
            Self::CreatedBy(_) => "createdBy",
            Self::Group(_) => "group",
        }
    }
}

/// What a field scope lets the holder do to a field (`FieldAction`): fill
/// it in, sign it, or put it in a group. A field's group is who fills it,
/// not who made it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FieldAction {
    Fill,
    Sign,
    SetGroup,
}

impl FieldAction {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Fill => "fill",
            Self::Sign => "sign",
            Self::SetGroup => "set-group",
        }
    }
}

/// The action slot of a field scope, which may be the wildcard.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FieldActionPattern {
    Any,
    One(FieldAction),
}

impl FieldActionPattern {
    pub fn covers(self, action: FieldAction) -> bool {
        match self {
            Self::Any => true,
            Self::One(one) => one == action,
        }
    }
}

/// A field filter (`FieldFilter`): every field, or the fields of one group.
/// Fields record no owner, so there is no `self` or `createdBy`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum FieldFilter {
    All,
    Group(String),
}

/// The two writes a field scope narrows (`FieldWriteAction`, `'fill' |
/// 'sign'`), each with the broad capability that decides when no field
/// scope names the action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FieldWriteAction {
    Fill,
    Sign,
}

impl FieldWriteAction {
    pub fn action(self) -> FieldAction {
        match self {
            Self::Fill => FieldAction::Fill,
            Self::Sign => FieldAction::Sign,
        }
    }

    /// `doc.forms.fill` to fill, `doc.sign` to sign.
    pub fn broad(self) -> DocCapability {
        match self {
            Self::Fill => DocCapability::DocFormsFill,
            Self::Sign => DocCapability::DocSign,
        }
    }
}

/// The form fields a session may act on, by group (`FieldGroupRight`):
/// every field (`{ all: true }`), or only the fields of the listed groups
/// (`{ all: false, groups }`, none when the list is empty). A field in no
/// group counts only under `All`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldGroupRight {
    All,
    Groups(Vec<String>),
}

impl FieldGroupRight {
    pub fn none() -> Self {
        Self::Groups(Vec::new())
    }

    /// Whether a field in `group_id` (`None`: in no group) is covered.
    pub fn covers(&self, group_id: Option<&str>) -> bool {
        match self {
            Self::All => true,
            Self::Groups(groups) => group_id.is_some_and(|g| groups.iter().any(|x| x == g)),
        }
    }
}

/// What a session may do with form fields, per action (`FieldRights`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldRights {
    /// The fields it may fill in.
    pub fill: FieldGroupRight,
    /// The fields it may sign.
    pub sign: FieldGroupRight,
    /// The groups it may put a field in: its own default group always among
    /// them.
    pub set_group: FieldGroupRight,
}

/// A parsed scope (`ParsedScope`).
///
/// TypeScript's `ParsedCollab` is a union on `entity`; here the two
/// entities are two variants, because their actions and filters differ.
/// `Collab` is always `entity: Annotations`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParsedScope {
    Capability(DocCapability),
    Collab {
        entity: CollabEntity,
        action: CollabActionPattern,
        filter: CollabFilter,
    },
    /// `fields:<fill|sign|set-group|*>:<all|group=X>`.
    FieldCollab {
        action: FieldActionPattern,
        filter: FieldFilter,
    },
    /// The one virtual scope, `pdf.permissions`.
    Virtual,
    Wildcard,
}

/// Typed view of the PDF user-access permission word.
///
/// Field names mirror ISO 32000 bit positions, as in TypeScript, rather than
/// being renamed to what each bit means. A rename would be an improvement in
/// isolation and a liability across a boundary two implementations have to
/// agree on: `bit6` is unambiguous, `annotate` is not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PdfBits {
    pub bit3: bool,
    pub bit4: bool,
    pub bit5: bool,
    pub bit6: bool,
    pub bit9: bool,
    pub bit10: bool,
    pub bit11: bool,
    pub bit12: bool,
}

/// Identity claims from a JWT, or supplied at engine open time.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IdentityClaims {
    pub user_id: Option<String>,
    pub group_id: Option<String>,
    pub groups: Vec<String>,
    pub display_name: Option<String>,
}

/// The per-record identity a collab filter is tested against.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CollabTarget {
    pub user_id: Option<String>,
    pub group_id: Option<String>,
}
