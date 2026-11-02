//! Engine-level font ownership.
//!
//! G4. Replaces `services/src/features/fonts/FontRegistrar.ts`. Operations:
//! `fonts.register`, `fonts.addFallback`, `fonts.clearFallbacks`,
//! `fonts.clear`, `fonts.authorizeEditing`, `fonts.list`, `fonts.program`.
//! [`document`] ports `DocumentFontReader.ts` (the `document.fonts.*`
//! kinds), and [`list`] the parts of engine-core's `fontList.ts` it reads
//! with.
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

pub mod document;
pub mod list;

// `LAST_RESORT_HASH` and `LAST_RESORT_PROGRAM`, from
// `packages/core/fonts/src/last-resort.generated.ts` (`build.rs`).
include!(concat!(env!("OUT_DIR"), "/last_resort.rs"));

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

/// The glyph-fallback tiers, `FALLBACK_TIERS` in
/// `engine/core/src/fonts/fontList.ts`.
///
/// PDFium tries every font of a lower tier before any of a higher one, so a
/// host's own fallbacks win over the built-in ones whatever order they were
/// added in. A tier is a JavaScript number here, as the TypeScript types it;
/// see [`FontRegistry::add_fallback_in_tier`].
pub mod fallback_tier {
    /// The host's or the application's own fonts. The default tier.
    pub const OWN: f64 = 0.0;
    /// The fonts the engine ships.
    pub const BUILT_IN: f64 = 1.0;
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
    /// The fallback tier, when `fallback` is set. See [`fallback_tier`].
    pub tier: f64,
    /// Whether its bytes may go to a browser (`StartupFontSpec.serve`, the
    /// server's `serve`); false by default there.
    pub serve: bool,
}

/// `FontFormat`: what a program is, as the wire spells it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FontFormat {
    Truetype,
    Opentype,
    Cff,
    Type1,
    Type3,
}

impl FontFormat {
    /// `FORMATS` in `DocumentFontReader.ts`: `EPDF_FONT_FORMAT_*`, `None`
    /// for `EPDF_FONT_FORMAT_NONE` and any code it doesn't name.
    pub fn of_program(format: Option<pdfium::FontProgramFormat>) -> Option<Self> {
        use pdfium::FontProgramFormat as F;
        Some(match format? {
            F::TrueType => Self::Truetype,
            F::OpenTypeCff => Self::Opentype,
            F::Cff => Self::Cff,
            F::Type1 => Self::Type1,
            F::Type3 => Self::Type3,
        })
    }
}

/// `CodePointRange`: `start` and the `count` of code points that follow.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct CodePointRange {
    pub start: u32,
    pub count: u32,
}

impl From<pdfium::CodePointRange> for CodePointRange {
    fn from(range: pdfium::CodePointRange) -> Self {
        Self { start: range.start, count: range.count }
    }
}

/// `EngineFontFacts`: one of the engine's fonts as a font list takes it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EngineFontFacts {
    pub key: String,
    pub identity: FontIdentityInfo,
    pub post_script_name: Option<String>,
    pub format: FontFormat,
    /// SHA-256 (hex) of the program the engine lays text out with.
    pub sha256: String,
    /// Whether its bytes may go to a browser.
    pub served: bool,
}

/// A program's coverage and line metrics, as `programFor(..).info()` and
/// `document.fonts.program` answer them: thousandths of the font size,
/// descent negative.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ProgramInfo {
    pub covers: Vec<CodePointRange>,
    pub ascent: f64,
    pub descent: f64,
}

/// `Math.round`: halves round up, toward positive infinity.
pub(crate) fn js_round(value: f64) -> f64 {
    let floor = value.floor();
    if value - floor >= 0.5 { floor + 1.0 } else { floor }
}

/// `Math.round(ascent * 1000)` and `-Math.round(descent * 1000)` of the
/// fork's em metrics, `0` and `-0` folded to `0` as JSON writes them.
pub(crate) fn thousandths(metrics: Option<pdfium::FontLineMetrics>) -> (f64, f64) {
    let (ascent, descent) =
        metrics.map_or((0.0, 0.0), |m| (f64::from(m.ascent), f64::from(m.descent)));
    (js_round(ascent * 1000.0) + 0.0, -js_round(descent * 1000.0) + 0.0)
}

/// Lowercase hex, as `hex` in `DocumentFontReader.ts` writes a hash.
pub(crate) fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes.iter().fold(String::with_capacity(bytes.len() * 2), |mut out, byte| {
        let _ = write!(out, "{byte:02x}");
        out
    })
}

/// What `FontRegistrar.readProgram` read for a key, and whether its bytes
/// may go to a browser.
#[derive(Debug, Clone, PartialEq)]
struct RegisteredProgram {
    sha256: String,
    format: FontFormat,
    post_script_name: Option<String>,
    served: bool,
}

impl RegisteredProgram {
    /// `readProgram`: the hash, `''` when the fork has none; `opentype` for
    /// `EPDF_FONT_FORMAT_OPENTYPE` and `truetype` for anything else; the
    /// PostScript name, `null` when empty.
    fn read(id: FontId, served: bool) -> Self {
        Self {
            sha256: pdfium::font_program_hash(id).map(|hash| hex(&hash)).unwrap_or_default(),
            format: match pdfium::font_program_format(id) {
                Some(pdfium::FontProgramFormat::OpenTypeCff) => FontFormat::Opentype,
                _ => FontFormat::Truetype,
            },
            post_script_name: pdfium::font_postscript_name(id)
                .ok()
                .flatten()
                .filter(|name| !name.is_empty()),
            served,
        }
    }
}

/// `FontRegistrar.registerLastResort`: the font a character no other font
/// has is drawn in, as a box. It has no key, so no list names it.
fn register_last_resort() -> Result<(), EngineError> {
    let registered =
        pdfium::register_memory_font(&FontDescription::inferred(), LAST_RESORT_PROGRAM);
    match registered {
        Ok(id) if pdfium::set_last_resort_font(Some(id)) => Ok(()),
        _ => Err(EngineError::with_message(
            EngineErrorCode::RuntimeUnavailable,
            "the last-resort font did not register",
        )),
    }
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
    /// The keys in the fallback chain with their tiers as given, in the order
    /// added, for reporting. The chain itself lives in PDFium.
    fallbacks: Vec<(String, f64)>,
    /// Each key's program, in the order first registered (a `Map`'s order,
    /// which `programFor` searches in).
    programs: Vec<(String, RegisteredProgram)>,
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
    /// The one registry for this thread, with the last resort registered,
    /// as `FontRegistrar`'s constructor registers it.
    ///
    /// Reports `InvalidArg` when this thread already has one. See
    /// [`REGISTRY_ON_THREAD`]. `RuntimeUnavailable` when the last resort
    /// doesn't register.
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
        // Built before the last resort registers: PDFium must be up on this
        // thread first, and a refusal drops it, which frees the thread.
        let registry = Self {
            ids: BTreeMap::new(),
            retained: Vec::new(),
            fallbacks: Vec::new(),
            programs: Vec::new(),
            _library: Library::acquire(),
        };
        register_last_resort()?;
        Ok(registry)
    }

    /// Record what `readProgram` reads for a key, keeping its first place.
    fn remember_program(&mut self, key: &str, id: FontId, served: bool) {
        let program = RegisteredProgram::read(id, served);
        match self.programs.iter_mut().find(|(k, _)| k == key) {
            Some(entry) => entry.1 = program,
            None => self.programs.push((key.to_owned(), program)),
        }
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
        // An application's own font: its bytes may go to a browser.
        self.remember_program(&key, id, true);
        self.ids.insert(key, id);
        Ok(id)
    }

    /// Register a font PDFium reads from a file on demand, its bytes kept
    /// from browsers (`registerFromNodeFile`'s default).
    pub fn register_file(
        &mut self,
        key: impl Into<String>,
        description: &FontDescription,
        path: &std::path::Path,
    ) -> Result<FontId, EngineError> {
        self.register_file_served(key, description, path, false)
    }

    /// [`FontRegistry::register_file`], saying whether its bytes may go to a
    /// browser (`registerFromNodeFile`'s `served`).
    pub fn register_file_served(
        &mut self,
        key: impl Into<String>,
        description: &FontDescription,
        path: &std::path::Path,
        served: bool,
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
        self.remember_program(&key, id, served);
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
            self.register_file_served(font.key.clone(), &font.description, &font.path, font.serve)?;
            if font.fallback {
                self.add_fallback_in_tier(&font.key, font.tier)?;
            }
        }
        Ok(())
    }

    /// Add a registered font to the glyph-fallback chain in the host's own
    /// tier, [`fallback_tier::OWN`], which is the TypeScript default.
    pub fn add_fallback(&mut self, key: &str) -> Result<(), EngineError> {
        self.add_fallback_in_tier(key, fallback_tier::OWN)
    }

    /// Add a registered font to the glyph-fallback chain in `tier`: it is
    /// tried after every font of a lower tier. Adding it again changes its
    /// tier only, keeping its place among the fonts added before it.
    ///
    /// `tier` is the number `FontRegistrar.addFallback` takes, and it goes
    /// where the TypeScript's does. PDFium gets `ToInt32(tier)`, which is what
    /// the WebAssembly call does to it: 1.7 is tier 1, NaN is tier 0, 2^32 + 1
    /// is tier 1. [`FontRegistry::fallback_order`] sorts by the number itself.
    pub fn add_fallback_in_tier(&mut self, key: &str, tier: f64) -> Result<(), EngineError> {
        let id = self.id_for(key)?;
        pdfium::add_fallback_font(id, crate::js_number::to_int32(tier)).map_err(|_| {
            EngineError::with_message(
                EngineErrorCode::InvalidArg,
                format!("addFallback failed: {key}"),
            )
        })?;
        match self.fallbacks.iter_mut().find(|(existing, _)| existing == key) {
            Some(existing) => existing.1 = tier,
            None => self.fallbacks.push((key.to_string(), tier)),
        }
        Ok(())
    }

    /// Empty the fallback chain, leaving every font registered.
    pub fn clear_fallbacks(&mut self) {
        pdfium::clear_fallback_fonts();
        self.fallbacks.clear();
    }

    /// The fallback chain by tier, each tier in the order added.
    /// `FontRegistrar.fallbackKeys`, which sorts with `left.tier - right.tier`
    /// on the tiers as given, so 1 comes before 1.7 here although PDFium
    /// tries both as tier 1.
    ///
    /// A difference that is NaN (a tier that was a word, or two infinities)
    /// counts as equal, as `Array.prototype.sort` counts it. That comparator
    /// is not an order, and the TypeScript's answer then depends on the
    /// JavaScript engine's sort; this answers what a stable insertion sort
    /// does with it, and never panics, which `sort_by` may on a comparator
    /// that is not total.
    pub fn fallback_order(&self) -> Vec<String> {
        let mut ordered: Vec<&(String, f64)> = self.fallbacks.iter().collect();
        for end in 1..ordered.len() {
            let mut at = end;
            while at > 0 && ordered[at - 1].1 - ordered[at].1 > 0.0 {
                ordered.swap(at - 1, at);
                at -= 1;
            }
        }
        ordered.into_iter().map(|(key, _)| key.clone()).collect()
    }

    /// Unregister everything and release the retained handles. The last
    /// resort stays: it is registered again, as `FontRegistrar.clear` does,
    /// and in its order, so a last resort that doesn't register refuses the
    /// clear with `RuntimeUnavailable` before the keys are forgotten.
    ///
    /// The order matters: PDFium stops reading through the handles only once
    /// the fonts are gone, so releasing them first would leave the registry
    /// reading closed files.
    pub fn clear(&mut self) -> Result<(), EngineError> {
        pdfium::clear_registered_fonts();
        register_last_resort()?;
        self.forget_all();
        Ok(())
    }

    /// Forget every key, program and fallback, and release the handles.
    fn forget_all(&mut self) {
        self.ids.clear();
        IDENTITIES.with(|all| all.borrow_mut().clear());
        self.fallbacks.clear();
        self.programs.clear();
        self.retained.clear();
    }

    /// `FontRegistrar.engineFontFacts`: the registered fonts, in
    /// registration order, as a font list takes them.
    pub fn engine_font_facts(&self) -> Vec<EngineFontFacts> {
        let keys: Vec<String> =
            IDENTITIES.with(|all| all.borrow().iter().map(|(key, _)| key.clone()).collect());
        keys.into_iter()
            .filter_map(|key| {
                let program = self.programs.iter().find(|(k, _)| *k == key)?.1.clone();
                let identity = self.identity_info(&key).ok()?;
                Some(EngineFontFacts {
                    key,
                    identity,
                    post_script_name: program.post_script_name,
                    format: program.format,
                    sha256: program.sha256,
                    served: program.served,
                })
            })
            .collect()
    }

    /// `FontRegistrar.keyForId`: the key of this thread's native id.
    pub fn key_for_id(&self, id: FontId) -> Option<String> {
        self.ids.iter().find(|(_, candidate)| **candidate == id).map(|(key, _)| key.clone())
    }

    /// `FontRegistrar.programFor`: the first key (in registration order)
    /// whose program has this hash. `None` for a hash no key has.
    fn key_for_program(&self, sha256: &str) -> Option<&str> {
        self.programs.iter().find(|(_, p)| p.sha256 == sha256).map(|(key, _)| key.as_str())
    }

    /// `programFor(sha256).info()`: a registered program's coverage and
    /// metrics. `None` for a hash no registered font has.
    pub fn program_info(&self, sha256: &str) -> Result<Option<ProgramInfo>, EngineError> {
        let Some(key) = self.key_for_program(sha256) else { return Ok(None) };
        let id = self.id_for(key)?;
        let covers = pdfium::font_coverage(id).map_err(engine_error)?;
        let (ascent, descent) = thousandths(pdfium::font_metrics(id));
        Ok(Some(ProgramInfo {
            covers: covers.into_iter().map(Into::into).collect(),
            ascent,
            descent,
        }))
    }

    /// `programFor(sha256).bytes()`: a registered program's bytes, empty
    /// when the fork reports none. `None` for a hash no registered font has.
    pub fn program_bytes(&self, sha256: &str) -> Result<Option<Vec<u8>>, EngineError> {
        let Some(key) = self.key_for_program(sha256) else { return Ok(None) };
        let id = self.id_for(key)?;
        Ok(Some(pdfium::font_program_data(id).map_err(engine_error)?.unwrap_or_default()))
    }

    /// `WorkerHost.handleFontsProgram`: a served engine font's bytes, by
    /// its program's hash. `NotFound` for a font not served.
    pub fn served_program(&self, sha256: &str) -> Result<Vec<u8>, EngineError> {
        let served =
            self.programs.iter().any(|(_, program)| program.sha256 == sha256 && program.served);
        let bytes = if served { self.program_bytes(sha256)? } else { None };
        bytes.ok_or_else(|| {
            EngineError::with_message(
                EngineErrorCode::NotFound,
                format!("no served font has the program {sha256}"),
            )
        })
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
        // Not `clear`: the thread keeps no last resort past its registry.
        pdfium::set_last_resort_font(None);
        pdfium::clear_registered_fonts();
        self.forget_all();
        REGISTRY_ON_THREAD.with(|taken| taken.set(false));
    }
}

/// A fork read that failed for want of memory.
fn engine_error(error: PdfiumError) -> EngineError {
    EngineError::with_message(EngineErrorCode::Unknown, error.to_string())
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
