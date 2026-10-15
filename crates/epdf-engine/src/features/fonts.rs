//! Engine-level font ownership.
//!
//! G4. Replaces `services/src/features/fonts/FontRegistrar.ts`. Operations:
//! `fonts.register`, `fonts.addFallback`, `fonts.clearFallbacks`,
//! `fonts.clear`.
//!
//! One registry per worker, selected at initialization and before any startup
//! font or document is loaded. Fonts are engine-level and not session-level:
//! PDFium's registry is thread-local, a font registered on a thread is visible
//! to everything that thread renders, and pretending otherwise by hanging a
//! registry off a document would make two sessions on one thread quietly share
//! state they did not agree to share.
//!
//! # Why a key and not the id
//!
//! PDFium's font id is a per-thread counter. It names a different font on
//! another thread and nothing at all on a thread that has registered nothing.
//! So the id never leaves: callers hold a stable key of their own, this type
//! owns the key-to-id map, and an id is resolved at the moment of use.
//! `pdfium::FontId` is a newtype for the same reason, so it cannot be
//! serialized by accident.
//!
//! # What the port order asks of this
//!
//! "G4 transfers the key-to-ID map, registration, fallback order and retained
//! handles together to one Rust `FontRegistry` per combined worker before
//! startup fonts or documents are loaded." All four are here. The TypeScript
//! `FontRegistrar` becomes a delegating facade with no second map, which is
//! web-side work this does not do.

use std::collections::BTreeMap;

use std::rc::Rc;

use pdfium::{
    FileAccess, FontDescription, FontId, FontIdentity, FontLicence, Library, PdfiumError,
};
use serde::Serialize;

use crate::error::{EngineError, EngineErrorCode};

/// `FontEmbeddingPermission`: the OS/2 `fsType` licence as the wire spells
/// it. Restricted and bitmap-only fonts are refused at registration, so
/// they never reach a key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum FontEmbeddingPermission {
    Installable,
    Editable,
    PreviewAndPrint,
}

impl FontEmbeddingPermission {
    /// `permissionFromCode` in `FontRegistrar.ts`.
    fn from_code(code: i32) -> Self {
        match code {
            1 => Self::Editable,
            2 => Self::PreviewAndPrint,
            _ => Self::Installable,
        }
    }
}

/// `FontIdentityInfo`: what the runtime resolved for a key, the identity a
/// document names the face by and the licence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FontIdentityInfo {
    pub family_name: String,
    pub weight: i32,
    pub italic: bool,
    pub embedding_permission: FontEmbeddingPermission,
    pub editing_authorized: bool,
    pub instanced: bool,
}

impl FontIdentityInfo {
    fn new(identity: FontIdentity, licence: FontLicence) -> Self {
        Self {
            family_name: identity.family_name,
            weight: identity.weight,
            italic: identity.italic,
            embedding_permission: FontEmbeddingPermission::from_code(licence.embedding_permission),
            editing_authorized: licence.editing_authorized,
            instanced: licence.instanced,
        }
    }
}

/// A font to register at startup, from a file.
///
/// By path rather than by bytes, mirroring how a file-backed base document is
/// opened: PDFium range-reads the file on demand and never holds it resident,
/// so a deployment can configure several large CJK fallbacks without paying
/// memory per font.
#[derive(Debug, Clone)]
pub struct StartupFont {
    pub key: String,
    pub path: std::path::PathBuf,
    pub description: FontDescription,
    /// Also append to the glyph-fallback chain.
    pub fallback: bool,
}

/// One thread's fonts.
///
/// Not `Send`, and deliberately so: PDFium's registry belongs to the thread
/// that filled it, and a registry that moved would name ids that mean nothing
/// where it arrived.
#[derive(Debug)]
pub struct FontRegistry {
    ids: BTreeMap<String, FontId>,
    /// Handles for path-registered fonts, held until [`FontRegistry::clear`].
    ///
    /// PDFium reads the file every time it builds a face rather than once at
    /// registration, so releasing these early would break a font that had
    /// registered successfully, at the moment something first tried to draw
    /// with it.
    retained: Vec<FileAccess>,
    /// The order fonts were added to the fallback chain, for reporting. The
    /// chain itself lives in PDFium.
    fallback_order: Vec<String>,
    /// Keeps PDFium initialized on this thread.
    ///
    /// Not optional and not the caller's job. Every `EPDFFont_*` call reads
    /// thread-local PDFium globals, and calling one on a thread that never ran
    /// `FPDF_InitLibrary` is a segmentation fault rather than an error. Holding
    /// the guard here makes a registry that exists a registry that works.
    ///
    /// Declared last so it outlives the `clear` in `Drop`, which unregisters
    /// through those same globals.
    _library: Rc<Library>,
}

thread_local! {
    /// Whether this thread already has a registry.
    ///
    /// PDFium's registry is one per thread, so two `FontRegistry` values on
    /// one thread are two owners of the same state: clearing either would pull
    /// the other's fonts out from under it, and dropping either would release
    /// file handles the other's fonts are still read through. The rule is in
    /// the port order ("one `FontRegistry` per combined worker"); this makes
    /// breaking it a loud failure at the second construction rather than a
    /// quiet one at the first clear.
    static REGISTRY_ON_THREAD: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };

    /// What each registered key resolved to, in registration order.
    ///
    /// Thread-local for the same reason the registry is one per thread: a
    /// rich text body read back on this thread names a face by family,
    /// weight and italic, and mapping that back to a key (`keyForFace` in
    /// `FontRegistrar.ts`) has to work from the readers, which are handed no
    /// registry. Order is kept because the TypeScript's `Map` breaks ties by
    /// it.
    static IDENTITIES: std::cell::RefCell<Vec<(String, FontIdentity)>> =
        const { std::cell::RefCell::new(Vec::new()) };
}

/// Record a key's identity: what the fork resolved, else what was given (a
/// runtime without the getters reports registration's own description).
fn remember_identity(key: &str, id: FontId, given: &FontDescription) {
    let identity = pdfium::font_identity(id).unwrap_or_else(|| FontIdentity {
        family_name: given.family_name.clone(),
        weight: if given.weight == 0 { 400 } else { given.weight },
        italic: given.italic == Some(true),
    });
    IDENTITIES.with(|all| {
        let mut all = all.borrow_mut();
        match all.iter_mut().find(|(k, _)| k == key) {
            Some(entry) => entry.1 = identity,
            None => all.push((key.to_owned(), identity)),
        }
    });
}

/// The key of the font registered on this thread whose identity matches a
/// face: family compared the fork's way, then the closest weight, italic
/// first. `None` when no registered family matches. Ports
/// `FontRegistrar.keyForFace`.
pub fn key_for_face(family: &str, weight: u32, italic: bool) -> Option<String> {
    let wanted = crate::features::annotation::rich_text::family_key(family);
    IDENTITIES.with(|all| {
        let mut best: Option<(&str, i64)> = None;
        let all = all.borrow();
        for (key, identity) in all.iter() {
            if crate::features::annotation::rich_text::family_key(&identity.family_name) != wanted {
                continue;
            }
            let score = (i64::from(identity.weight) - i64::from(weight)).abs()
                + if identity.italic == italic { 0 } else { 1000 };
            if best.is_none_or(|(_, s)| score < s) {
                best = Some((key, score));
            }
        }
        best.map(|(key, _)| key.to_owned())
    })
}

impl FontRegistry {
    /// The one registry for this thread.
    ///
    /// Reports `InvalidArg` when this thread already has one. See
    /// [`REGISTRY_ON_THREAD`].
    pub fn new() -> Result<Self, EngineError> {
        REGISTRY_ON_THREAD.with(|taken| {
            if taken.get() {
                return Err(EngineError::with_message(
                    EngineErrorCode::InvalidArg,
                    "this thread already has a font registry; PDFium's is thread-global \
                     and two owners would clear each other's fonts",
                ));
            }
            taken.set(true);
            Ok(())
        })?;
        Ok(Self {
            ids: BTreeMap::new(),
            retained: Vec::new(),
            fallback_order: Vec::new(),
            _library: Library::acquire(),
        })
    }

    /// Register a font from bytes.
    ///
    /// PDFium copies them, so the caller's buffer is free on return.
    pub fn register(
        &mut self,
        key: impl Into<String>,
        description: &FontDescription,
        bytes: &[u8],
    ) -> Result<FontId, EngineError> {
        let key = key.into();
        // `FontRegistrar.register` refuses empty bytes before the runtime
        // sees them, with a message of its own.
        if bytes.is_empty() {
            return Err(EngineError::with_message(
                EngineErrorCode::InvalidArg,
                format!("empty font data: {key}"),
            ));
        }
        let id = pdfium::register_memory_font(description, bytes)
            .map_err(|e| registration_failed(&key, None, e))?;
        remember_identity(&key, id, description);
        self.ids.insert(key, id);
        Ok(id)
    }

    /// Register a font PDFium reads from a file on demand.
    pub fn register_file(
        &mut self,
        key: impl Into<String>,
        description: &FontDescription,
        path: &std::path::Path,
    ) -> Result<FontId, EngineError> {
        let key = key.into();
        let access =
            FileAccess::from_file(path).map_err(|e| registration_failed(&key, Some(path), e))?;
        let id = pdfium::register_font_file(description, &access)
            .map_err(|e| registration_failed(&key, Some(path), e))?;
        // Retained only on success. A failed registration that kept its handle
        // would leak a file descriptor per attempt.
        self.retained.push(access);
        remember_identity(&key, id, description);
        self.ids.insert(key, id);
        Ok(id)
    }

    /// Register a host's startup fonts, in order.
    ///
    /// Order is load-bearing twice over: it fixes the native ids and it fixes
    /// fallback precedence, so every worker running the same list ends up
    /// identical. A list that registered in a different order would give two
    /// workers different answers for the same missing glyph.
    pub fn register_startup(&mut self, fonts: &[StartupFont]) -> Result<(), EngineError> {
        for font in fonts {
            self.register_file(font.key.clone(), &font.description, &font.path)?;
            if font.fallback {
                self.add_fallback(&font.key)?;
            }
        }
        Ok(())
    }

    /// Append a registered font to the glyph-fallback chain.
    pub fn add_fallback(&mut self, key: &str) -> Result<(), EngineError> {
        let id = self.id_for(key)?;
        pdfium::add_fallback_font(id).map_err(|_| {
            EngineError::with_message(
                EngineErrorCode::InvalidArg,
                format!("addFallback failed: {key}"),
            )
        })?;
        self.fallback_order.push(key.to_string());
        Ok(())
    }

    /// Empty the fallback chain, leaving every font registered.
    pub fn clear_fallbacks(&mut self) {
        pdfium::clear_fallback_fonts();
        self.fallback_order.clear();
    }

    /// The fallback chain, in precedence order.
    pub fn fallback_order(&self) -> &[String] {
        &self.fallback_order
    }

    /// Unregister everything and release the retained handles.
    ///
    /// The order matters: PDFium stops reading through the handles only once
    /// the fonts are gone, so releasing them first would leave the registry
    /// reading closed files.
    pub fn clear(&mut self) {
        pdfium::clear_registered_fonts();
        self.ids.clear();
        IDENTITIES.with(|all| all.borrow_mut().clear());
        self.fallback_order.clear();
        self.retained.clear();
    }

    /// Resolve a stable key to this thread's id.
    pub fn id_for(&self, key: &str) -> Result<FontId, EngineError> {
        self.ids.get(key).copied().ok_or_else(|| {
            EngineError::with_message(
                EngineErrorCode::InvalidArg,
                format!("font not registered on this thread: {key}"),
            )
        })
    }

    /// What a key resolved to. `None` for a key never registered here.
    /// Ports `FontRegistrar.describeOrUndefined`.
    pub fn describe(&self, key: &str) -> Option<FontIdentity> {
        if !self.ids.contains_key(key) {
            return None;
        }
        IDENTITIES.with(|all| {
            all.borrow().iter().find(|(k, _)| k == key).map(|(_, identity)| identity.clone())
        })
    }

    /// The identity and licence the runtime resolved for a key: what
    /// `fonts.register` answers. Ports `FontRegistrar.describe`, which
    /// refuses an unknown key with `InvalidArg`.
    pub fn identity_info(&self, key: &str) -> Result<FontIdentityInfo, EngineError> {
        let id = self.id_for(key)?;
        let identity = self.describe(key).ok_or_else(|| {
            EngineError::with_message(
                EngineErrorCode::InvalidArg,
                format!("font not registered on this thread: {key}"),
            )
        })?;
        Ok(FontIdentityInfo::new(identity, pdfium::font_licence(id)))
    }

    /// The application asserts a licence permitting editing with a font.
    /// Ports `FontRegistrar.authorizeEditing`: `InvalidArg` for an unknown
    /// key or a refusal, else the identity and licence read back after the
    /// change.
    pub fn authorize_editing(&mut self, key: &str) -> Result<FontIdentityInfo, EngineError> {
        let id = self.id_for(key)?;
        if !pdfium::authorize_font_editing(id) {
            return Err(EngineError::with_message(
                EngineErrorCode::InvalidArg,
                format!("authorizeEditing failed: {key}"),
            ));
        }
        let previous = self.describe(key).ok_or_else(|| {
            EngineError::with_message(
                EngineErrorCode::InvalidArg,
                format!("font not registered on this thread: {key}"),
            )
        })?;
        // `readIdentity` with the previous identity as what was "given".
        let given = FontDescription {
            family_name: previous.family_name,
            weight: previous.weight,
            italic: Some(previous.italic),
        };
        remember_identity(key, id, &given);
        let identity = self.describe(key).expect("just remembered");
        Ok(FontIdentityInfo::new(identity, pdfium::font_licence(id)))
    }

    pub fn contains(&self, key: &str) -> bool {
        self.ids.contains_key(key)
    }

    pub fn len(&self) -> usize {
        self.ids.len()
    }

    pub fn is_empty(&self) -> bool {
        self.ids.is_empty()
    }

    /// Every registered key, sorted.
    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.ids.keys().map(String::as_str)
    }
}

impl Drop for FontRegistry {
    /// Unregisters this thread's fonts, then releases the handles.
    ///
    /// Both, and in that order. Releasing the handles alone would leave
    /// PDFium's registry holding fonts it reads through closed files, which is
    /// a use-after-free the next glyph lookup would find. Unregistering alone
    /// would leak a file descriptor per path-registered font.
    ///
    /// This is safe to do on drop only because a thread has one registry;
    /// [`FontRegistry::new`] is what makes that true.
    fn drop(&mut self) {
        self.clear();
        REGISTRY_ON_THREAD.with(|taken| taken.set(false));
    }
}

/// `FontRegistrar`'s refusal, worded as it words it when the runtime
/// answers no id. Any other failure (a family name with a NUL, a file that
/// will not open) has no TypeScript counterpart and keeps its own detail.
fn registration_failed(
    key: &str,
    path: Option<&std::path::Path>,
    error: PdfiumError,
) -> EngineError {
    let where_ = match path {
        Some(path) => format!("{key} ({})", path.display()),
        None => key.to_string(),
    };
    let message = match error {
        PdfiumError::FontRegistration => {
            format!("font registration failed (unloadable or no glyphs): {where_}")
        }
        other => format!("font registration failed for {where_}: {other}"),
    };
    EngineError::with_message(EngineErrorCode::InvalidArg, message)
}

/// The wire's `italic`: `-1` infers from the file, `0` and `1` say. Any
/// other number infers too, because the fork's `NormalizeItalic` treats
/// everything but `0` and `1` as "infer" and the TypeScript passes the
/// number straight through.
pub fn italic_from_wire(code: i32) -> Option<bool> {
    match code {
        0 => Some(false),
        1 => Some(true),
        _ => None,
    }
}

/// `WorkerHost.handleDocumentSetFontSettings`: per-document font and
/// text-layout settings. Only the members given change. Session state of
/// the handle; nothing is written to the file.
pub fn apply_document_font_settings(
    document: &pdfium::Document,
    embedding_policy: Option<&str>,
    typographic_features: Option<bool>,
) -> Result<(), EngineError> {
    if let Some(policy) = embedding_policy {
        // `{ default: 0, subset: 1, full: 2 }`. A name outside the map is
        // refused the way the fork refuses an invalid code.
        let code = match policy {
            "default" => 0,
            "subset" => 1,
            "full" => 2,
            _ => -1,
        };
        if code < 0 || !document.set_font_embedding_policy(code) {
            return Err(EngineError::with_message(
                EngineErrorCode::InvalidArg,
                "EPDFDoc_SetFontEmbeddingPolicy failed",
            ));
        }
    }
    if let Some(enabled) = typographic_features
        && !document.set_typographic_features(enabled)
    {
        return Err(EngineError::with_message(
            EngineErrorCode::InvalidArg,
            "EPDFDoc_SetTypographicFeatures failed",
        ));
    }
    Ok(())
}
