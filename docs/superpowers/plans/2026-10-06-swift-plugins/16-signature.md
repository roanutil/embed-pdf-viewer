# Signature and Document Security: Swift Wrapper Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make digital signatures work end to end on iOS: a native engine that reads, analyzes and seals signatures, a `NativeSecurity` that enforces document permissions and lets a locked document unlock, `epdf-plugin-signature` exported as `SignatureHandle`, and a Swift `SignatureCapability` that signs with a Keychain key and validates against anchors the app supplies.

**Architecture:** Three layers, built bottom up. `epdf-engine` gains the `signatures.*` wire kinds and `forms.setSignatureAppearance`, ported from `packages/engine/services/src/features/signature/` (2,555 lines of TypeScript) over the fork's `epdf_signature.h`, `epdf_checkpoint.h` and `epdf_digest.h`. `epdf-viewer-host` replaces its allow-everything `NativeSecurity` (`crates/epdf-viewer-host/src/native_engine.rs:271-320`) with one built on `epdf_engine::session::SessionAuthority`, keeps a password-locked document open as a locked tab, and maps the new paths. The signature plugin then gets the usual FFI slice (`2026-10-01-swift-package.md` Appendix A §A.0). Its state is raw JSON in Rust (`crates/plugins/signature/src/model.rs:20-35`), so every record is a typed mirror decoded from the engine's DTOs. The two host ports become foreign traits: `SignatureSigner` (Swift: a `SecKey` and its certificate chain) and `SignatureTrust` (Swift: DER anchors).

**Tech Stack:** Rust 2024, `uniffi =0.32.0`, `rsa` (already a dev-dependency of `epdf-signature-core`), Swift 6.2 tools, Swift Testing, `swift-custom-dump` 1.3, Security.framework, Xcode 27.0, iPhone 17 simulator.

**Spec:** `docs/superpowers/plans/2026-10-06-swift-plugins/00-umbrella.md`, `docs/superpowers/plans/2026-10-01-swift-package.md` (Global Constraints, Appendix A §A.5, Appendix B row B4, Appendix C row `signatures.*`), the TypeScript wire at `packages/engine/core/src/wire/worker-protocol.ts:160-262` and `:513-522`, the DTOs at `packages/engine/core/src/signature/types.ts`, the verdict shape at `packages/core/signature/src/verdict.ts:12-47`, and `packages/engine/main/src/document/LocalDocumentSecurityService.ts` for security.

## Global Constraints

Everything under "Global Constraints" in `00-umbrella.md` applies. The ones this plan is most likely to break:

- `uniffi` is pinned at `=0.32.0`.
- UniFFI names are unique across the workspace. Records here start with `Signature` or `Document`.
- No `serde_json::Value` across the boundary. Every engine DTO decodes into a mirror; a DTO that doesn't decode is `invalid-state`.
- PDFium never crosses threads. A Swift signer is called from the kernel thread and must not block on the main actor.
- An ECDSA `SignatureSigner` answers `r || s`, as `epdf-signature-core` expects (`crates/plugins/signature-core/src/cms.rs:124-125`), not X9.62 DER. The Swift signer converts.
- `signature` requires `form` (`crates/plugins/signature/src/lib.rs:31`). This plan starts after plan 06 merges (Wave D).
- The fork headers this plan binds (`epdf_signature.h`, `epdf_checkpoint.h`, `epdf_digest.h`) aren't in the 2026-08-30 PDFium slices.
- Test key material is `packages/engine/main/test/fixtures/signature-compat/keys/TEST-ONLY-*.pem`. Nothing else signs in tests, and no real key ever enters the repo.

This plan is large: eleven tasks, five of them engine ports. Tasks 2 to 6 are independent of the Swift work and can be reviewed as a block; if a reviewer wants it split, Tasks 1 to 6 are a coherent "native signatures and security" plan on their own, and Tasks 7 to 11 follow.

## Review Focus

1. A password-protected PDF opened without a password. People expect a locked tab (`DocumentStatus.locked`) they can unlock with `PDFViewer.unlock`, not a failed open. Pinned in Task 2 (`a_locked_document_opens_locked_and_unlocks`) and Task 10 (`aLockedDocumentUnlocks`).
2. A viewer configured with a narrow scope (`["doc.open", "doc.render"]`). People expect edit affordances to report `false` (`canEdit`, `canSign`, `canFill`) and edits to be refused with `permission-denied`, not silently allowed as today. Pinned in Task 2 (`a_narrow_scope_denies_edits`) and Task 10 (`aNarrowScopeCannotSign`).
3. A signature whose bytes were changed after signing (a later incremental update that rewrites a signed field). People expect `integrity` or `modifications` to report it, and `summary` to be `invalid`, never `valid`. Pinned in Task 4 (`an_edit_after_an_approval_is_judged_forbidden`) and Task 8 (`a_changed_document_validates_invalid`).
4. A Swift signer that throws (the user cancels a biometric prompt). People expect `sign` to fail with the signer's error and the document to have no parked signing (`pending` is `nil`), not a document stuck read-only. Pinned in Task 8 (`a_throwing_signer_leaves_nothing_pending`) and Task 10 (`aCancelledSignerLeavesNothingPending`).
5. Validation with no trust anchors configured. People expect every intact signature to top out at `validUntrusted`, never `valid`. Pinned in Task 8 (`without_anchors_a_good_signature_is_valid_untrusted`) and Task 10 (`withoutAnchorsASignatureIsValidUntrusted`).

## Cross-plugin tests

| Integration | Kind | Owner |
|---|---|---|
| form (field records, `forms.setSignatureAppearance`) | required | this plan; plan 06 has merged |
| stamp (signature-kind libraries, `placeMark` with an asset mark) | optional | this plan, Task 10 (`aStampMarkFillsAField`), when plan 09 has merged; skip the test with `.disabled(if:)` naming plan 09 if it hasn't |
| interaction (the armed-mark handler, `lib.rs:38-53`) | optional | this plan, Task 9's host test registers both and checks `ViewerHost::new` succeeds; the gesture path is plan 01's to test |
| annotation (free placement, `PlaceTarget::Free`) | optional | this plan, Task 10 (`aFreeMarkIsAStamp`) |
| metadata, page-edit, every editing plugin | consumers of `NativeSecurity` | each already has a `can*` member; Task 2's scope test covers `metadata.canEdit`, and the others inherit the same `DocumentSecurity` |

## File Structure

```
crates/pdfium-sys/wrapper.h                               + epdf_signature.h, epdf_checkpoint.h, epdf_digest.h
crates/pdfium/src/signature.rs                            new: signature model, contents, byte range, revisions
crates/pdfium/src/signature_write.rs                      new: prepare / patch / abort over the fork's candidate API
crates/pdfium/tests/signature.rs                          new
crates/epdf-engine/src/auth/scope/{types.rs,parser.rs,resolver.rs}   + doc.sign, doc.sign.certify
crates/epdf-engine/src/host.rs                            open.fatMem scope; signatures.*; forms.setSignatureAppearance; document.version
crates/epdf-engine/src/features/signature/mod.rs          new
crates/epdf-engine/src/features/signature/reader.rs       new (SignatureReader.ts, readSignatureModel.ts)
crates/epdf-engine/src/features/signature/protection.rs   new (core/src/signature/protection.ts)
crates/epdf-engine/src/features/signature/analyzer.rs     new (SignatureAnalyzer.ts)
crates/epdf-engine/src/features/signature/mutator.rs      new (SignatureMutator.ts, internal/candidateStore.ts, internal/appearance.ts)
crates/epdf-engine/tests/signature_read.rs, signature_analyze.rs, signature_sign.rs, security_scope.rs
crates/epdf-viewer-host/src/native_engine.rs              NativeSecurity, locked documents, unlock
crates/epdf-viewer-host/src/translate.rs                  signatures.*, forms.setSignatureAppearance, document.version
crates/epdf-viewer-host/src/api.rs                        ViewerConfig.scope, ViewerConfig.signature
crates/epdf-viewer-host/src/{session.rs,lib.rs}           signature wiring, signer and trust slots
crates/epdf-viewer-host/tests/{translate.rs,viewer_host.rs,security.rs}
crates/plugins/signature/{Cargo.toml,src/lib.rs,src/controller.rs}
crates/plugins/signature/src/ffi/mod.rs                   new: handle
crates/plugins/signature/src/ffi/records.rs               new: DTO mirrors
crates/plugins/signature/src/ffi/ports.rs                 new: SignatureSigner, SignatureTrust and adapters
crates/plugins/signature/tests/ffi.rs                     new
platforms/apple/EmbedPDF/Sources/EmbedPDF/PDFViewer.swift unlock doc comment; setSigner, setTrust
platforms/apple/EmbedPDF/Sources/EmbedPDF/Plugins/Signature.swift
platforms/apple/EmbedPDF/Sources/EmbedPDF/Plugins/KeychainSigner.swift
platforms/apple/EmbedPDF/Tests/EmbedPDFTests/{SignatureTests,SecurityTests,KeychainSignerTests}.swift
platforms/apple/build-xcframework.sh                      + signature fixtures and TEST-ONLY keys
.github/workflows/plugin-rust.yml, platforms/apple/README.md
```

---

### Task 1: Worktree and prerequisites

**Files:**
- Modify: `crates/pdfium-sys/wrapper.h`

**Interfaces:**
- Consumes: `research/swift-plugins` with plans 01, 05 and 06 merged (and 09 if present).
- Produces: `pdfium_sys::EPDFSig_*`, `EPDFDoc_GetRevision*`, `EPDFDoc_OpenRevision`, `EPDFDoc_ReadLoadedBytes`, `EPDFObjectDiff_*`, `EPDFAnnot_SetAppearanceFromPage` bindings.

- [x] **Step 1: Create the worktree**

```bash
git worktree add .claude/worktrees/swift-signature -b research/swift-signature research/swift-plugins
cd .claude/worktrees/swift-signature
```

- [x] **Step 2: Check the PDFium slices**

```bash
for t in darwin-arm64 ios-arm64 ios-sim-arm64; do
  for h in epdf_signature.h epdf_checkpoint.h epdf_digest.h; do
    ls packages/engine/runtime/build/libpdfium/$t/include/$h >/dev/null || echo "MISSING $t $h"
  done
done
```

Expected: no output. Otherwise this worktree fetched the old slices: run `bash packages/engine/runtime/scripts/fetch-libpdfium.sh <slice>` for each, and if a header is still missing, stop and finish umbrella Task 0b, which rebuilds the slices from fork commit `6dc31d14e` or later.

- [x] **Step 3: Include the headers**

In `crates/pdfium-sys/wrapper.h`, after the last `epdf_*.h` include:

```c
#include "epdf_checkpoint.h"
#include "epdf_digest.h"
#include "epdf_signature.h"
```

- [x] **Step 4: Confirm the bindings**

Run: `cargo build -p pdfium-sys && grep -c 'pub fn EPDFSig_GetByteRange\|pub fn EPDFDoc_GetRevisionCount\|pub fn EPDFObjectDiff_GetCount\|pub fn EPDFSig_DigestByteRange' target/debug/build/pdfium-sys-*/out/bindings.rs`
Expected: `4`.

- [x] **Step 5: Commit**

```bash
git add crates/pdfium-sys/wrapper.h
git commit -m "Bind the fork's signature, checkpoint and digest headers"
```

### Task 2: `NativeSecurity`, scopes and locked documents

**Files:**
- Modify: `crates/epdf-engine/src/auth/scope/types.rs`, `parser.rs`, `resolver.rs`
- Modify: `crates/epdf-engine/src/host.rs` (`open_fat_memory`)
- Create: `crates/epdf-engine/tests/security_scope.rs`
- Modify: `crates/epdf-viewer-host/src/native_engine.rs`, `src/api.rs`, `src/session.rs`, `src/lib.rs`
- Create: `crates/epdf-viewer-host/tests/security.rs`
- Modify: `platforms/apple/EmbedPDF/Sources/EmbedPDF/PDFViewer.swift`

**Interfaces:**
- Produces: `DocCapability::{DocSign, DocSignCertify}` (`"doc.sign"`, `"doc.sign.certify"`); `open.fatMem` reads an optional `scope: string[]` (default `["*"]`) and `identity` object; `ViewerConfig.scope: Option<Vec<String>>`; a locked open resolves with `DocumentStatus::Locked`; `ViewerHost::unlock_document` works.

`SessionAuthority` and `check_capability` already exist (`crates/epdf-engine/src/session/authority.rs:47-123`, `auth/scope/resolver.rs:43-104`). What's missing is the wire: `open.fatMem` always passes `SessionAuthority::unrestricted()` (`host.rs:1169`), and the native document ignores the answer. The kernel reads permissions only through `DocumentSecurity`, so the native side must answer from the same authority the engine enforces with, or a control would show enabled and then fail.

- [x] **Step 1: Write the failing engine test**

```rust
// crates/epdf-engine/tests/security_scope.rs
use epdf_engine::auth::scope::{DocCapability, expand_raw_scope, decode_pdf_bits};
use epdf_engine::host::Host;
use serde_json::json;

mod common;
use common::open_fixture_with;

#[test]
fn doc_sign_expands_from_form_fill_bits() {
    let bits = decode_pdf_bits(Some(1 << 8)); // bit 9: fill in forms
    let caps = expand_raw_scope(&["pdf.permissions".into()], bits).unwrap();
    assert!(caps.contains(&DocCapability::DocSign));
    assert!(!caps.contains(&DocCapability::DocSignCertify));
    assert_eq!(DocCapability::parse("doc.sign.certify"), Some(DocCapability::DocSignCertify));
}

#[test]
fn an_open_with_a_narrow_scope_refuses_a_metadata_write() {
    let mut host = Host::new();
    let doc = open_fixture_with(&mut host, "pdfium-multiplatform-prototype/fixtures/report.pdf", json!({ "scope": ["doc.open", "doc.render"] }));
    let response = host.call(
        &json!({ "kind": "metadata.update", "jobId": 9, "docId": doc, "patch": { "title": "x" } }).to_string(),
        Vec::new(),
    );
    let envelope: serde_json::Value = serde_json::from_str(&response.json).unwrap();
    assert_eq!(envelope["error"]["code"], json!("Forbidden"), "{envelope}");
}
```

`open_fixture_with` sends `open.fatMem` with the extra fields merged into the request; add it to `tests/common/mod.rs` beside `open_fixture`. The bit position for "fill in forms" is bit 9 (`1 << 8`), per `resolver.ts:306`.

- [x] **Step 2: Run it to verify it fails**

Run: `cargo test -p epdf-engine --test security_scope`
Expected: FAIL to compile, no `DocSign`.

- [x] **Step 3: Add the capabilities and read the scope**

In `types.rs`, add `DocSign` and `DocSignCertify` to `DocCapability`, to `ALL` (its length becomes 23), and to `as_str` (`"doc.sign"`, `"doc.sign.certify"`), in the order `packages/engine/core/src/auth/scope/types.ts:52-53` declares them. Add both strings to the known list in `parser.rs` (`parser.ts:36-37`). In `resolver.rs`, where bit 6 or bit 9 grants `DocFormsFill`, grant `DocSign` too:

```rust
    if b.bit6 || b.bit9 {
        out.insert(DocCapability::DocFormsFill);
        out.insert(DocCapability::DocSign);
    }
```

`DocSignCertify` is grant-minted only and never expanded from bits. In `host.rs` `open_fat_memory`, replace `SessionAuthority::unrestricted()`:

```rust
        let scope: Vec<String> = optional(body, "scope")?.unwrap_or_else(|| vec!["*".to_string()]);
        let identity: IdentityClaims = optional(body, "identity")?.unwrap_or_default();
        let authority = SessionAuthority::new(&scope, identity, PdfBits::default()).map_err(|e| {
            EngineError::with_message(EngineErrorCode::InvalidArg, format!("invalid scope: {e}"))
        })?;
```

and pass `authority`. The registry already calls `set_pdf_bits` once it has probed the document (`authority.rs:83-92`); confirm `open_memory` does, and add the call after the probe if it doesn't. Add `"doc.sign"` and `"doc.sign.certify"` to any capability list `tests/wire_contract.rs` or the scope vectors check, and regenerate nothing: `tooling/engine-conformance/vectors/scope/` comes from the TypeScript, which already has both.

- [x] **Step 4: Run the engine tests**

Run: `cargo test -p epdf-engine`
Expected: PASS, including `security_scope` and the scope vectors.

- [x] **Step 5: Write the failing host test**

```rust
// crates/epdf-viewer-host/tests/security.rs
use std::sync::Arc;

use epdf_ffi::CancelToken;
use epdf_viewer_host::{DocumentStatus, OpenDocumentOptions, ViewerConfig, ViewerHost};
use futures::executor::block_on;

mod common;
use common::{NullDelegate, fixture};

#[test]
fn a_locked_document_opens_locked_and_unlocks() {
    let host = ViewerHost::new(ViewerConfig::default(), Arc::new(NullDelegate)).unwrap();
    block_on(host.start()).unwrap();
    let doc = block_on(host.open_document(fixture("encrypted-hello"), OpenDocumentOptions::default())).unwrap();
    assert_eq!(doc.status, DocumentStatus::Locked);
    let wrong = block_on(host.unlock_document(doc.id.clone(), "nope".into()));
    assert!(wrong.is_err());
    block_on(host.unlock_document(doc.id.clone(), "secret".into())).unwrap();
    let info = block_on(host.documents()).unwrap().into_iter().find(|d| d.id == doc.id).unwrap();
    assert_eq!(info.status, DocumentStatus::Ready);
    assert!(info.page_count > 0);
}

#[test]
fn a_narrow_scope_denies_edits() {
    let host = ViewerHost::new(
        ViewerConfig { metadata: true, scope: Some(vec!["doc.open".into(), "doc.render".into()]), ..Default::default() },
        Arc::new(NullDelegate),
    )
    .unwrap();
    block_on(host.start()).unwrap();
    let doc = block_on(host.open_document(fixture("report"), OpenDocumentOptions::default())).unwrap();
    let metadata = block_on(host.metadata(doc.id)).unwrap();
    assert!(!block_on(metadata.can_edit()).unwrap());
}

#[test]
fn the_default_scope_allows_everything() {
    let host = ViewerHost::new(ViewerConfig { metadata: true, ..Default::default() }, Arc::new(NullDelegate)).unwrap();
    block_on(host.start()).unwrap();
    let doc = block_on(host.open_document(fixture("report"), OpenDocumentOptions::default())).unwrap();
    assert!(block_on(block_on(host.metadata(doc.id)).unwrap().can_edit()).unwrap());
}
```

`encrypted-hello.pdf`'s password is the one `pdfium-multiplatform-prototype/fixtures/README.md` gives; use that string. Move `NullDelegate` and `fixture` into `tests/common/mod.rs` if they live inside `viewer_host.rs` today.

- [x] **Step 6: Run it to verify it fails**

Run: `cargo test -p epdf-viewer-host --test security`
Expected: FAIL, `no field scope`; once that compiles, `a_locked_document_opens_locked_and_unlocks` fails because the open rejects with `DocPasswordRequired` (`lib.rs:128-130`).

- [x] **Step 7: Implement `NativeSecurity` and the locked open**

`api.rs`, in `ViewerConfig`, before the plugin fields:

```rust
    /// The capability scope every document opens with, as the engine
    /// parses it (`doc.open`, `doc.annotate.modify`, `pdf.permissions`,
    /// ...). `None` is `["*"]`: everything, today's behaviour.
    #[uniffi(default = None)]
    pub scope: Option<Vec<String>>,
```

`session.rs` passes `config.scope.clone()` to `NativeEngine::new`, which keeps it in `Inner` and sends it on every `open.fatMem` as `"scope"`.

In `native_engine.rs`, the open no longer fails a locked document. After `open.fatMem` answers its `security` probe, read the access state the engine reports (`security.access.reason == "password"`, from `features/security/state.rs:57-70`); for a locked document, skip `pages.list` and resolve a `NativeDocument` with an empty page list and `locked: Cell<bool>` set:

```rust
            let locked = security
                .pointer("/access/reason")
                .and_then(Value::as_str)
                == Some("password");
            if locked {
                return settled.resolve(Rc::new(NativeDocument::locked(doc, inner, security)));
            }
```

`NativeDocument` gains `pages: RefCell<Value>`, `security: RefCell<Value>` and `locked: Cell<bool>`. `security()` returns a `NativeSecurity` over a shared `Rc<NativeDocumentState>` so an unlock that lands is visible to every holder:

```rust
/// What the kernel asks before an action, answered from the same scope
/// and permission bits the engine enforces with.
struct NativeSecurity {
    state: Rc<NativeDocumentState>,
}

struct NativeDocumentState {
    id: String,
    inner: Rc<Inner>,
    authority: RefCell<epdf_engine::session::SessionAuthority>,
    security: RefCell<Value>,
    pages: RefCell<Value>,
    locked: Cell<bool>,
}

impl DocumentSecurity for NativeSecurity {
    fn allows(&self, capability: &str) -> bool {
        !self.state.locked.get()
            && epdf_engine::auth::scope::DocCapability::parse(capability)
                .is_some_and(|c| self.state.authority.borrow().allows(c))
    }

    fn allows_annotation_create(&self) -> bool {
        self.allows("doc.annotate.modify")
    }

    fn allows_annotation_mutation(&self, action: &str, target: &Value) -> bool {
        let authority = self.state.authority.borrow();
        let Some(action) = epdf_engine::auth::scope::CollabAction::parse(action) else { return false };
        let Ok(target) = serde_json::from_value(target.clone()) else { return false };
        !self.state.locked.get() && authority.allows_collab(action, &target)
    }

    fn allows_annotation_group_assignment(&self, group_id: &str) -> bool {
        !self.state.locked.get() && self.state.authority.borrow().allows_set_group(group_id)
    }

    fn effective_scope(&self) -> Vec<String> {
        self.state.authority.borrow().expanded().iter().map(|c| c.as_str().to_string()).collect()
    }

    fn identity(&self) -> Value {
        serde_json::to_value(self.state.authority.borrow().identity()).unwrap_or(Value::Null)
    }

    fn current(&self) -> Value {
        self.state.security.borrow().clone()
    }

    fn password_prompt(&self) -> Value {
        if self.state.locked.get() {
            json!({ "state": "required" })
        } else {
            json!({ "state": "none" })
        }
    }

    /// `document.checkPasswordPermissions`, then the page list the open
    /// skipped. Resolves `{security}` once the tab can be read.
    fn unlock(&self, input: Value) -> EngineCall<Value> {
        let state = self.state.clone();
        let password = input.get("password").cloned().unwrap_or(Value::Null);
        let mode = input.get("mode").cloned().unwrap_or_else(|| json!("any"));
        let checked = state.inner.send(&state.id, |job| {
            Ok(WireCall {
                request: json!({ "kind": "document.checkPasswordPermissions", "jobId": job,
                                 "docId": state.id, "password": password, "mode": mode }),
                payloads: Vec::new(),
                decode: Decode::Result,
            })
        });
        Promise::spawn(async move {
            let probe = checked.await?;
            let security = probe.get("security").cloned().unwrap_or(Value::Null);
            if security.pointer("/access/reason").and_then(Value::as_str) == Some("password") {
                return Err(Error::engine("DocPasswordRequired", "DocPasswordRequired: wrong password"));
            }
            let pages = state
                .inner
                .send(&state.id, |job| translate::translate(&state.id, "pages.list", None, &[], job))
                .await?;
            *state.pages.borrow_mut() = pages;
            *state.security.borrow_mut() = security.clone();
            state.locked.set(false);
            Ok(json!({ "security": security }))
        })
    }
}
```

`allows_collab`, `allows_set_group` and `CollabAction::parse` wrap `check_collab` and `check_set_group` (`resolver.rs:73`, `:157`) on `SessionAuthority`; add them there if they're missing, as one-line delegations beside `allows`. The authority mirrors the engine's: built from the same scope and identity at open, with `set_pdf_bits` from the probe's permission word (`security.permissions.bits` in the probe JSON). Check the probe key name in `features/security/state.rs` `SecurityState` before reading it.

Delete the module doc's "Document security is not enforced" paragraph and the `lib.rs:128-130` note, and replace them with one line each saying what's enforced and where.

In `PDFViewer.swift`, replace `unlock`'s doc comment:

```swift
  /// Unlock a document that opened `.locked` (password-protected and opened
  /// without one). Throws `DocPasswordRequired` for a wrong password; the
  /// tab stays locked.
```

- [x] **Step 8: Run the host tests**

Run: `cargo test -p epdf-ffi -p epdf-viewer-host`
Expected: PASS. Earlier tests that expected a locked open to throw (`a_locked_document_reports_that_it_needs_a_password`, from the spec's Task 3) now fail; change them to assert `DocumentStatus::Locked` and say why in the commit message. The Swift `ViewerTests` equivalent changes the same way in Task 10.

- [x] **Step 9: Commit**

```bash
git add crates/epdf-engine crates/epdf-viewer-host platforms/apple/EmbedPDF/Sources/EmbedPDF/PDFViewer.swift
git commit -m "Enforce document scopes natively and keep a locked document open until unlocked

A locked open now resolves as a Locked tab instead of rejecting with
DocPasswordRequired; the tests that pinned the rejection now pin the tab."
```

### Task 3: Reading signatures in `pdfium` and `epdf-engine`

**Files:**
- Create: `crates/pdfium/src/signature.rs`, `crates/pdfium/tests/signature.rs`
- Create: `crates/epdf-engine/src/features/signature/{mod.rs,reader.rs,protection.rs}`
- Create: `crates/epdf-engine/tests/signature_read.rs`
- Modify: `crates/epdf-engine/src/features/mod.rs`, `crates/epdf-engine/src/host.rs`

**Interfaces:**
- Produces: wire kinds `signatures.list` (`{workingCopy?}` → `SignatureSnapshot`), `signatures.contents` (`{ref: FormFieldRef}` → payload bytes, result `{size}`), `signatures.revisionBytes` (`{revisionIndex}` → payload bytes), `document.version` (→ `BaseVersionInfo`), shapes from `types.ts:70-197`.

The TypeScript to port, function for function:

| Rust | TypeScript |
|---|---|
| `pdfium::signature::SignatureModel` (open, count, per-field facts, close) | the `EPDFSig_*` calls in `internal/readSignatureModel.ts` (319 lines) |
| `pdfium::Document::{revision_count, revision, read_loaded_bytes, loaded_bytes_size}` | `EPDFDoc_GetRevisionCount`, `EPDFDoc_GetRevision`, `EPDFDoc_ReadLoadedBytes`, `EPDFDoc_GetLoadedBytesSize` in `SignatureReader.ts:146-230` |
| `reader::read_snapshot`, `read_contents`, `revision_bytes`, `version` | `SignatureReader.ts` `readSnapshot`, `readContents`, `revisionBytes`, `version` (lines 53-216) |
| `reader::read_model` | `readSignatureModel.ts` `readSignatureModel` |
| `protection::derive_protection` | `packages/engine/core/src/signature/protection.ts` (150 lines, pure) |

`protection.rs` is pure and is pinned by a vector: generate `tooling/engine-conformance/vectors/signature/protection.json` from the TypeScript with a generator script beside the existing ones (copy `tooling/engine-conformance/scripts/` `security` generator's shape), add its `:check` script to `package.json` and `.github/workflows/engine-rust.yml`, and replay it in `crates/epdf-engine/tests/signature_protection.rs`. Every other function here touches PDFium and is pinned by the fixture tests below.

- [x] **Step 1: Write the failing tests**

```rust
// crates/epdf-engine/tests/signature_read.rs
use epdf_engine::host::Host;
use serde_json::{Value, json};

mod common;
use common::{call, call_bytes, open_fixture};

const FIX: &str = "packages/engine/main/test/fixtures";

#[test]
fn two_approvals_list_in_field_order_with_whole_revision_coverage() {
    let mut host = Host::new();
    let (doc, _) = open_fixture(&mut host, &format!("{FIX}/signed_two_approvals.pdf"));
    let snapshot = call(&mut host, json!({ "kind": "signatures.list", "docId": doc }));
    assert_eq!(snapshot["chainValid"], json!(true));
    let signatures = snapshot["signatures"].as_array().unwrap();
    assert_eq!(signatures.len(), 2);
    for (i, s) in signatures.iter().enumerate() {
        assert_eq!(s["index"], json!(i));
        assert_eq!(s["signed"], json!(true));
        assert_eq!(s["coverage"], json!("whole-revision"));
        assert_eq!(s["field"]["kind"], json!("objectNumber"));
    }
    assert_eq!(snapshot["revisions"].as_array().unwrap().len(), 3);
    assert_eq!(snapshot["protection"]["enforced"], Value::Null);
    assert_eq!(snapshot["protection"]["judged"], json!("fill"));
}

#[test]
fn a_certification_is_named_in_protection() {
    let mut host = Host::new();
    let (doc, _) = open_fixture(&mut host, &format!("{FIX}/signed_certified.pdf"));
    let snapshot = call(&mut host, json!({ "kind": "signatures.list", "docId": doc }));
    assert_eq!(snapshot["signatures"][0]["catalogCertification"], json!(true));
    assert!(snapshot["protection"]["certification"].is_object());
}

#[test]
fn an_unsigned_field_lists_unsigned() {
    let mut host = Host::new();
    let (doc, _) = open_fixture(&mut host, &format!("{FIX}/unsigned_sigfield.pdf"));
    let snapshot = call(&mut host, json!({ "kind": "signatures.list", "docId": doc }));
    let s = &snapshot["signatures"][0];
    assert_eq!(s["signed"], json!(false));
    assert_eq!(s["coverage"], Value::Null);
    assert_eq!(snapshot["protection"]["judged"], Value::Null);
}

#[test]
fn contents_and_revision_bytes_are_payloads() {
    let mut host = Host::new();
    let (doc, _) = open_fixture(&mut host, &format!("{FIX}/signed_two_approvals.pdf"));
    let snapshot = call(&mut host, json!({ "kind": "signatures.list", "docId": doc }));
    let field = snapshot["signatures"][0]["field"].clone();
    let cms = call_bytes(&mut host, json!({ "kind": "signatures.contents", "docId": doc, "ref": field }));
    assert_eq!(cms[0], 0x30, "a DER SEQUENCE");
    let revision = call_bytes(&mut host, json!({ "kind": "signatures.revisionBytes", "docId": doc, "revisionIndex": 0 }));
    assert!(revision.starts_with(b"%PDF-"));
    let version = call(&mut host, json!({ "kind": "document.version", "docId": doc }));
    assert_eq!(version["sha256"].as_str().unwrap().len(), 64);
}
```

`call_bytes` is `call` that returns the first payload; add it to `tests/common/mod.rs`. If the fixture facts differ (three revisions, `judged == "fill"`), take the true values from `packages/engine/main/test/signatures*.test.ts`, which runs the same fixtures through the TypeScript, and use those: the TypeScript is the reference, and this test pins the port to it.

- [x] **Step 2: Run them to verify they fail**

Run: `cargo test -p epdf-engine --test signature_read`
Expected: FAIL, unknown request kind `signatures.list`.

- [x] **Step 3: Write the `pdfium` layer**

`crates/pdfium/src/signature.rs` opens with the model handle and the per-signature facts the reader needs:

```rust
//! The signature model: every signature field, its dictionary, its byte
//! range and contents, and the revisions they seal.
//!
//! Wraps `EPDFSig_*` and `EPDFDoc_*Revision*` from `epdf_signature.h` and
//! `epdf_checkpoint.h`. A model is opened over the loaded bytes and closed
//! on drop; `EPDFSig_CloseModel` must run on the thread that opened it,
//! which `!Send` guarantees.

use crate::{Document, PdfiumError};

pub struct SignatureModel<'a> {
    doc: &'a Document,
    raw: pdfium_sys::EPDF_SIGMODEL,
    _not_send: std::marker::PhantomData<*const ()>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SignatureFacts {
    pub field_object_number: u32,
    pub field_name: String,
    pub widget_object_number: u32,
    pub widget_page_object_number: u32,
    pub kind: i32,
    pub filter: Option<String>,
    pub sub_filter: Option<String>,
    pub byte_range: Option<[u64; 4]>,
    pub contents_size: u64,
    pub coverage: i32,
    pub revision_index: Option<u32>,
    pub name: Option<String>,
    pub reason: Option<String>,
    pub location: Option<String>,
    pub contact_info: Option<String>,
    pub claimed_time: Option<String>,
    pub doc_mdp: Option<u8>,
    pub catalog_certification: bool,
}

impl Document {
    pub fn signature_model(&self) -> Result<SignatureModel<'_>, PdfiumError> {
        let raw = unsafe { pdfium_sys::EPDFSig_LoadModel(self.as_raw()) };
        if raw.is_null() {
            return Err(PdfiumError::Unknown("EPDFSig_LoadModel"));
        }
        Ok(SignatureModel { doc: self, raw, _not_send: std::marker::PhantomData })
    }

    pub fn revision_count(&self) -> u32 {
        unsafe { pdfium_sys::EPDFDoc_GetRevisionCount(self.as_raw()) }.max(0) as u32
    }

    pub fn loaded_bytes_size(&self) -> u64 {
        unsafe { pdfium_sys::EPDFDoc_GetLoadedBytesSize(self.as_raw()) } as u64
    }

    /// `[offset, offset + len)` of the bytes the document was loaded from.
    pub fn read_loaded_bytes(&self, offset: u64, len: u64) -> Result<Vec<u8>, PdfiumError> {
        let mut out = vec![0u8; len as usize];
        let read = unsafe {
            pdfium_sys::EPDFDoc_ReadLoadedBytes(self.as_raw(), offset as _, out.as_mut_ptr().cast(), len as _)
        };
        if read as u64 != len {
            return Err(PdfiumError::Unknown("EPDFDoc_ReadLoadedBytes"));
        }
        Ok(out)
    }
}

impl Drop for SignatureModel<'_> {
    fn drop(&mut self) {
        unsafe { pdfium_sys::EPDFSig_CloseModel(self.raw) };
    }
}
```

Then `SignatureModel::count`, `facts(index) -> SignatureFacts`, `contents(index) -> Vec<u8>`, `field_mdp(index)`, `lock(index)` and `seed_value(index)`, each the transcription of the matching block of `readSignatureModel.ts` and calling the `EPDFSig_Get*` function that block calls (the list is in the Task 1 grep, and every one of them is in `epdf_signature.h`). Check each function's exact signature in the generated `bindings.rs` as you write its call; the names above (`EPDF_SIGMODEL`, `EPDFSig_LoadModel`) are the header's, but the TypeScript's `EPDFForm_LoadModel` / `EPDFSig_CloseModel` pairing says the model may be opened through the form model, in which case `signature_model` takes the `FormModel` the crate already has (`crates/pdfium/src/form.rs`).

- [x] **Step 4: Write the engine reader**

`crates/epdf-engine/src/features/signature/reader.rs`:

```rust
//! `SignatureReader.ts` and `internal/readSignatureModel.ts`: the
//! signature snapshot, a signature's `/Contents`, a revision's bytes and
//! the loaded bytes' version.

use serde_json::{Value, json};

use crate::cancel::CancelToken;
use crate::error::{EngineError, EngineErrorCode};
use crate::session::DocumentSession;

use super::protection::derive_protection;

pub fn read_snapshot(session: &DocumentSession, cancel: &CancelToken) -> Result<Value, EngineError> {
    cancel.check()?;
    let doc = session.document()?.document();
    let revisions = read_revisions(doc)?;
    let chain_valid = !revisions.is_empty();
    let model = doc.signature_model().map_err(EngineError::from)?;
    let mut signatures = Vec::new();
    for index in 0..model.count() {
        cancel.check()?;
        signatures.push(signature_json(&model, index, &revisions)?);
    }
    let protection = derive_protection(&signatures, chain_valid);
    Ok(json!({
        "chainValid": chain_valid,
        "revisions": if chain_valid { Value::Array(revisions.iter().map(|r| r.to_json()).collect()) } else { json!([]) },
        "signatures": signatures,
        "protection": protection,
    }))
}

pub fn read_contents(session: &DocumentSession, field: &Value, cancel: &CancelToken) -> Result<Vec<u8>, EngineError> {
    cancel.check()?;
    let doc = session.document()?.document();
    let model = doc.signature_model().map_err(EngineError::from)?;
    let index = resolve_field(&model, field)?;
    model.contents(index).map_err(EngineError::from)
}

pub fn revision_bytes(session: &DocumentSession, revision_index: u32, cancel: &CancelToken) -> Result<Vec<u8>, EngineError> {
    cancel.check()?;
    let doc = session.document()?.document();
    let revisions = read_revisions(doc)?;
    let revision = revisions.get(revision_index as usize).ok_or_else(|| {
        EngineError::with_message(EngineErrorCode::NotFound, format!("no revision {revision_index}"))
    })?;
    doc.read_loaded_bytes(0, revision.end).map_err(EngineError::from)
}

pub fn version(session: &DocumentSession, cancel: &CancelToken) -> Result<Value, EngineError> {
    cancel.check()?;
    let doc = session.document()?.document();
    let size = doc.loaded_bytes_size();
    let bytes = doc.read_loaded_bytes(0, size).map_err(EngineError::from)?;
    Ok(json!({ "sha256": crate::features::binary::sha256_hex(&bytes), "byteLength": size }))
}
```

with `read_revisions`, `signature_json` and `resolve_field` transcribed from `SignatureReader.ts:101-145` and `readSignatureModel.ts`. If `features::binary` has no `sha256_hex`, use the SHA-256 `epdf-signature-core` already depends on through its `crypto.rs`, or add `sha2` to `epdf-engine` if that's the only way, and say so in the commit. Route the four kinds in `host.rs` with the document reads; `signatures.contents` and `signatures.revisionBytes` answer through `bytes_response` (the helper `pages.extract` uses, `host.rs:542`). `workingCopy: true` on `signatures.list` goes to Task 4's `read_working_copy_snapshot`; until then, answer it from the loaded bytes, which the TypeScript does too when there are no unsaved edits.

- [x] **Step 5: Run the tests**

Run: `cargo test -p pdfium -p epdf-engine`
Expected: PASS, including `signature_read` and `signature_protection`.

- [x] **Step 6: Commit**

```bash
git add crates/pdfium crates/epdf-engine tooling package.json .github/workflows/engine-rust.yml
git commit -m "Read signatures, revisions and document protection in the Rust engine"
```

### Task 4: `signatures.analyze`

**Files:**
- Create: `crates/epdf-engine/src/features/signature/analyzer.rs`, `crates/epdf-engine/tests/signature_analyze.rs`
- Modify: `crates/pdfium/src/signature.rs` (object diff), `crates/epdf-engine/src/host.rs`

**Interfaces:**
- Produces: wire kind `signatures.analyze` (`{input: {since: {signatureIndex}, until: "persisted" | "working-copy"}}` → `ChangeAnalysis`, the shape `SignatureAnalyzer.ts:116` returns and `epdf-signature-core`'s `verdict.rs:178-200` reads: `{current: {primary: {verdict, objectNumber, ...} | null, ...}, laterRevisions, undone}`); `read_working_copy_snapshot` for `signatures.list {workingCopy: true}`.

`SignatureAnalyzer.ts` is 665 lines over `EPDFDoc_CompareRevisions` and `EPDFObjectDiff_*`. Port it as one module, keeping the TypeScript's function boundaries and names (snake-cased), and keep its classification table (`forbidden` / `permitted` per object kind and modification level) as a `const` table, not a `match` spread across functions, so the two can be diffed by eye.

- [x] **Step 1: Write the failing tests**

```rust
// crates/epdf-engine/tests/signature_analyze.rs
use epdf_engine::host::Host;
use serde_json::{Value, json};

mod common;
use common::{call, open_fixture};

const FIX: &str = "packages/engine/main/test/fixtures";

fn analyze(host: &mut Host, doc: &str, index: u32) -> Value {
    call(host, json!({ "kind": "signatures.analyze", "docId": doc,
                       "input": { "since": { "signatureIndex": index }, "until": "persisted" } }))
}

#[test]
fn the_second_approval_is_a_permitted_change_after_the_first() {
    let mut host = Host::new();
    let (doc, _) = open_fixture(&mut host, &format!("{FIX}/signed_two_approvals.pdf"));
    let analysis = analyze(&mut host, &doc, 0);
    assert_eq!(analysis["laterRevisions"], json!(1));
    let primary = &analysis["current"]["primary"];
    assert!(primary.is_null() || primary["verdict"] != json!("forbidden"), "{analysis}");
}

#[test]
fn an_edit_after_an_approval_is_judged_forbidden() {
    let mut host = Host::new();
    let (doc, pon) = open_fixture(&mut host, &format!("{FIX}/signed_two_approvals.pdf"));
    // An unsaved annotation, then judge the working copy.
    call(&mut host, json!({ "kind": "annotations.create", "docId": doc, "pageObjectNumber": pon,
        "draft": { "subtype": "square", "rect": { "left": 10, "bottom": 10, "right": 50, "top": 50 } } }));
    let analysis = call(&mut host, json!({ "kind": "signatures.analyze", "docId": doc,
        "input": { "since": { "signatureIndex": 1 }, "until": "working-copy" } }));
    assert_eq!(analysis["current"]["primary"]["verdict"], json!("forbidden"), "{analysis}");
}

#[test]
fn a_field_mdp_lock_forbids_a_locked_field_change() {
    let mut host = Host::new();
    let (doc, _) = open_fixture(&mut host, &format!("{FIX}/signed_fieldmdp.pdf"));
    let snapshot = call(&mut host, json!({ "kind": "signatures.list", "docId": doc }));
    assert!(!snapshot["protection"]["fieldLocks"].as_array().unwrap().is_empty());
}
```

The `annotations.create` draft shape follows whatever plan 05's translate tests send; copy one from `crates/epdf-viewer-host/tests/translate.rs`. As in Task 3, take any fixture fact that disagrees from `packages/engine/main/test/` and pin that.

- [x] **Step 2: Run them to verify they fail**

Run: `cargo test -p epdf-engine --test signature_analyze`
Expected: FAIL, unknown request kind `signatures.analyze`.

- [x] **Step 3: Port the analyzer**

Add `ObjectDiff` to `crates/pdfium/src/signature.rs` over `EPDFDoc_CompareRevisions`, `EPDFObjectDiff_GetCount`, `GetEntry`, `GetValue`, `GetReferrer`, `GetReferrerCount`, `GetReadStatus`, `GetRevisionHealth` and `Close` (closed on drop), plus `Document::open_revision(index)` (`EPDFDoc_OpenRevision`) and `open_base_overlay` (`EPDFDoc_OpenBaseOverlay`) for the working copy. Then `analyzer.rs`:

```rust
//! `SignatureAnalyzer.ts`: what changed after a signature, judged against
//! the protection the signatures declare. Never refuses: it reports.

use serde_json::Value;

use crate::cancel::CancelToken;
use crate::error::EngineError;
use crate::session::DocumentSession;

pub fn analyze(session: &DocumentSession, input: &Value, cancel: &CancelToken) -> Result<Value, EngineError> {
    // SignatureAnalyzer.analyze, lines 116-end
}

pub fn read_working_copy_snapshot(session: &DocumentSession, cancel: &CancelToken) -> Result<Value, EngineError> {
    // SignatureAnalyzer.readWorkingCopySnapshot, lines 97-115
}
```

Fill both bodies by transcribing the named TypeScript ranges, with the helpers it calls in the same file. Route `signatures.analyze` in `host.rs` with the document reads, and send `signatures.list {workingCopy: true}` to `read_working_copy_snapshot`.

- [x] **Step 4: Run the tests**

Run: `cargo test -p pdfium -p epdf-engine`
Expected: PASS.

- [x] **Step 5: Commit**

```bash
git add crates/pdfium crates/epdf-engine
git commit -m "Analyze changes after a signature in the Rust engine"
```

### Task 5: Sealing: `signatures.prepare`, `complete`, `abort` and the visual fill

**Files:**
- Create: `crates/pdfium/src/signature_write.rs`
- Create: `crates/epdf-engine/src/features/signature/mutator.rs`, `crates/epdf-engine/tests/signature_sign.rs`
- Modify: `crates/epdf-engine/src/host.rs`, `crates/epdf-engine/Cargo.toml` (dev-dependency on `epdf-signature-core`)

**Interfaces:**
- Produces: `signatures.prepare` (`{input: SignaturePrepareInput}` → `SignaturePrepared`, digest as a payload), `signatures.complete` (`{input: {signingId, expectedVersion}}` with the CMS as payload 0 → `SignatureCompleteResult` in a mutation envelope), `signatures.abort` (`{signingId}` → `{status}`), `forms.setSignatureAppearance` (`{ref, pageIndex}` with the PDF as payload 0 → mutation envelope).

Port `SignatureMutator.ts` (538 lines) with `internal/candidateStore.ts` (451), `internal/appearance.ts` (67) and `internal/sealCheck.ts` (72). The candidate store keeps a prepared signing's bytes in memory keyed by `signingId`, with the `expectedVersion` fence; `complete` refuses a version that moved (`Conflict`), replays idempotently for the same CMS (`already-completed`), and reloads the session onto the sealed bytes, which re-pins every page (the result's `meta`). `CandidateFinalizer.ts` is the cloud's session-less path and isn't ported.

- [x] **Step 1: Write the failing test**

The test signs with `TEST-ONLY-private-key.pem` through `epdf-signature-core`'s own CMS builder, so the engine is tested against the CMS the plugin will hand it.

```rust
// crates/epdf-engine/tests/signature_sign.rs
use epdf_engine::host::Host;
use serde_json::{Value, json};

mod common;
use common::{call, call_bytes_with, call_with_payload, open_fixture};

const FIX: &str = "packages/engine/main/test/fixtures";

fn test_cms(digest: &[u8]) -> Vec<u8> {
    epdf_signature_core::harness::test_cms_over_digest(
        digest,
        &std::fs::read_to_string(format!("{}/../../{FIX}/signature-compat/keys/TEST-ONLY-private-key.pem", env!("CARGO_MANIFEST_DIR"))).unwrap(),
        &std::fs::read_to_string(format!("{}/../../{FIX}/signature-compat/keys/TEST-ONLY-signer.pem", env!("CARGO_MANIFEST_DIR"))).unwrap(),
    )
}

#[test]
fn prepare_then_complete_seals_the_field() {
    let mut host = Host::new();
    let (doc, _) = open_fixture(&mut host, &format!("{FIX}/unsigned_sigfield.pdf"));
    let field = call(&mut host, json!({ "kind": "signatures.list", "docId": doc }))["signatures"][0]["field"].clone();
    let (prepared, digest) = call_bytes_with(&mut host, json!({ "kind": "signatures.prepare", "docId": doc,
        "input": { "field": field, "attribution": { "name": "Test Signer" } } }));
    let cms = test_cms(&digest);
    let completed = call_with_payload(&mut host, json!({ "kind": "signatures.complete", "docId": doc,
        "input": { "signingId": prepared["signingId"], "expectedVersion": prepared["expectedVersion"] } }), cms.clone());
    assert_eq!(completed["result"]["status"], json!("completed"));
    let snapshot = call(&mut host, json!({ "kind": "signatures.list", "docId": doc }));
    assert_eq!(snapshot["signatures"][0]["signed"], json!(true));
    assert_eq!(snapshot["signatures"][0]["coverage"], json!("whole-revision"));
    // Replaying the same CMS is idempotent.
    let replay = call_with_payload(&mut host, json!({ "kind": "signatures.complete", "docId": doc,
        "input": { "signingId": prepared["signingId"], "expectedVersion": prepared["expectedVersion"] } }), cms);
    assert_eq!(replay["result"]["status"], json!("already-completed"));
}

#[test]
fn abort_discards_a_prepared_signing() {
    let mut host = Host::new();
    let (doc, _) = open_fixture(&mut host, &format!("{FIX}/unsigned_sigfield.pdf"));
    let field = call(&mut host, json!({ "kind": "signatures.list", "docId": doc }))["signatures"][0]["field"].clone();
    let (prepared, _) = call_bytes_with(&mut host, json!({ "kind": "signatures.prepare", "docId": doc, "input": { "field": field } }));
    let aborted = call(&mut host, json!({ "kind": "signatures.abort", "docId": doc, "signingId": prepared["signingId"] }));
    assert_eq!(aborted["status"], json!("aborted"));
    let again = call(&mut host, json!({ "kind": "signatures.abort", "docId": doc, "signingId": prepared["signingId"] }));
    assert_eq!(again["status"], json!("unknown"));
}

#[test]
fn a_visual_fill_draws_into_an_unsigned_field() {
    let mut host = Host::new();
    let (doc, _) = open_fixture(&mut host, &format!("{FIX}/unsigned_sigfield.pdf"));
    let field = call(&mut host, json!({ "kind": "signatures.list", "docId": doc }))["signatures"][0]["field"].clone();
    let artwork = std::fs::read(format!("{}/../../{FIX}/signature_artwork.pdf", env!("CARGO_MANIFEST_DIR"))).unwrap();
    let filled = call_with_payload(&mut host, json!({ "kind": "forms.setSignatureAppearance", "docId": doc, "ref": field, "pageIndex": 0 }), artwork);
    assert!(filled.get("event").is_some(), "{filled}");
}

#[test]
fn completing_after_the_document_moved_is_a_conflict() {
    let mut host = Host::new();
    let (doc, _) = open_fixture(&mut host, &format!("{FIX}/unsigned_sigfield.pdf"));
    let field = call(&mut host, json!({ "kind": "signatures.list", "docId": doc }))["signatures"][0]["field"].clone();
    let (prepared, digest) = call_bytes_with(&mut host, json!({ "kind": "signatures.prepare", "docId": doc, "input": { "field": field } }));
    let mut stale = prepared["expectedVersion"].clone();
    stale["editsVersion"] = json!(stale["editsVersion"].as_u64().unwrap() + 1);
    let response = host.call(
        &json!({ "kind": "signatures.complete", "jobId": 7, "docId": doc,
                 "input": { "signingId": prepared["signingId"], "expectedVersion": stale } }).to_string(),
        vec![test_cms(&digest)],
    );
    let envelope: Value = serde_json::from_str(&response.json).unwrap();
    assert_eq!(envelope["error"]["code"], json!("Conflict"), "{envelope}");
}
```

`call_bytes_with` returns `(result, first payload)`; `call_with_payload` sends one payload and returns the whole envelope's `result` object (so `["result"]["status"]` reads the mutation's inner result). `epdf_signature_core::harness::test_cms_over_digest` doesn't exist yet: add it to `crates/plugins/signature-core/src/harness.rs` as a thin wrapper that loads the PKCS#8 key (as `harness.rs:175` does), builds a `RawSigner` with the certificate, and calls `cms::build_detached_cms` with `Profile::CadesB` and `Digest::Sha256`, blocking on the promise with `epdf_kernel::rt::block_on`. Gate it behind the crate's existing `harness` feature.

- [x] **Step 2: Run them to verify they fail**

Run: `cargo test -p epdf-engine --test signature_sign`
Expected: FAIL, unknown request kind `signatures.prepare`.

- [x] **Step 3: Port the mutator**

`crates/pdfium/src/signature_write.rs` wraps the fork's candidate calls `SignatureMutator.ts` makes (`EPDFSig_DigestByteRange`, the `/Contents` hole reservation, `EPDFAnnot_SetAppearanceFromPage`, `EPDFAnnot_UpdateAppearanceToRect`, the incremental save the candidate is built from). `mutator.rs` holds `CandidateStore` (a `HashMap<String, Candidate>` on the session, with each candidate's bytes, byte range, contents size, digest, field object number and `expectedVersion`) and:

```rust
pub fn prepare(session: &mut DocumentSession, input: &Value, cancel: &CancelToken) -> Result<(Value, Vec<u8>), EngineError>;
pub fn complete(session: &mut DocumentSession, input: &Value, cms: Vec<u8>, cancel: &CancelToken) -> Result<Value, EngineError>;
pub fn abort(session: &mut DocumentSession, signing_id: &str) -> Result<Value, EngineError>;
pub fn set_signature_appearance(session: &mut DocumentSession, field: &Value, pdf: Vec<u8>, page_index: u32, cancel: &CancelToken) -> Result<Value, EngineError>;
```

each transcribing its TypeScript namesake (`prepare` 127-247, `complete` 248-327, `abort` 328-end, and `appearance.ts` for the fill). `prepare` validates `contentsSize` in 256..=4 MiB (default 8192) and refuses `certify` unless the document has no signature yet and the caller holds `doc.sign.certify`; `complete` runs `sealCheck.ts`'s checks before swapping the session onto the sealed bytes. Route the four kinds in `host.rs`: `prepare` answers its digest as a payload; `complete` and `forms.setSignatureAppearance` go through the mutation envelope helper with event kinds `signatures.completed` and `forms.changed`; `abort` is a plain read-shaped answer. The `doc.sign` / `doc.forms.fill` checks use `session.authority().require(...)` like every other mutation.

- [x] **Step 4: Run the tests**

Run: `cargo test -p pdfium -p epdf-engine -p epdf-signature-core`
Expected: PASS.

- [x] **Step 5: Commit**

```bash
git add crates/pdfium crates/epdf-engine crates/plugins/signature-core
git commit -m "Prepare, complete and abort signatures, and draw visual fills, in the Rust engine"
```

### Task 6: Map the signature paths

**Files:**
- Modify: `crates/epdf-viewer-host/src/translate.rs`, `crates/epdf-viewer-host/tests/translate.rs`

**Interfaces:**
- Produces: `translate::maps` answers `signatures`, `document.versioned`, `forms.setSignatureAppearance`; routes for `signatures.list`, `signatures.contents`, `signatures.revisionBytes`, `signatures.analyze`, `signatures.prepare`, `signatures.complete`, `signatures.abort`, `document.version`, `forms.setSignatureAppearance`; `Decode::InlineBytes` (the first payload as `{"$bytes": ...}`; plan 10's `Decode::HostBytes` parks bytes in the `HostTable` instead, and plan 06's `Decode::ExportBytes` adds them to the result) and `Decode::ResultWithBytes(&'static str)` (the result with the first payload added under that key).

The plugin passes bytes as `Bytes` JSON (`{"$bytes": base64}`, `crates/plugins/signature/src/write/fill.rs:52`) and reads them back the same way. Translation moves them into and out of payloads, so a CMS or a revision never crosses the worker as base64.

- [x] **Step 1: Write the failing tests**

```rust
#[test]
fn signature_paths_are_mapped() {
    for path in ["signatures", "document.versioned", "forms.setSignatureAppearance", "signatures.list",
                 "signatures.contents", "signatures.revisionBytes", "signatures.analyze",
                 "signatures.prepare", "signatures.complete", "signatures.abort"] {
        assert!(translate::maps(path), "{path}");
    }
}

#[test]
fn complete_moves_the_cms_into_a_payload() {
    let input = json!({ "signingId": "s1", "cms": { "$bytes": "AQID" }, "expectedVersion": { "baseSha256": "x", "editsVersion": 0 } });
    let call = translate::translate("doc-1", "signatures.complete", None, &[input], 1).unwrap();
    assert_eq!(call.request["kind"], json!("signatures.complete"));
    assert!(call.request["input"].get("cms").is_none());
    assert_eq!(call.payloads, vec![vec![1, 2, 3]]);
}

#[test]
fn a_visual_fill_moves_the_pdf_into_a_payload() {
    let field = json!({ "kind": "objectNumber", "fieldObjectNumber": 12 });
    let call = translate::translate("doc-1", "forms.setSignatureAppearance", None,
        &[field.clone(), json!({ "pdf": { "$bytes": "JVBERg==" }, "pageIndex": 0 })], 1).unwrap();
    assert_eq!(call.request["ref"], field);
    assert_eq!(call.request["pageIndex"], json!(0));
    assert_eq!(call.payloads, vec![b"%PDF".to_vec()]);
}

#[test]
fn contents_come_back_as_bytes() {
    let response = HostResponse { json: r#"{"result":{"size":3}}"#.into(), payloads: vec![vec![1, 2, 3]] };
    let decoded = translate::decode(&translate::Decode::InlineBytes, response).unwrap();
    assert_eq!(decoded.value, json!({ "$bytes": "AQID" }));
}

#[test]
fn prepare_answers_its_digest_inside_the_result() {
    let response = HostResponse { json: r#"{"result":{"signingId":"s1"}}"#.into(), payloads: vec![vec![9]] };
    let decoded = translate::decode(&translate::Decode::ResultWithBytes("digest"), response).unwrap();
    assert_eq!(decoded.value["digest"], json!({ "$bytes": "CQ==" }));
    assert_eq!(decoded.value["signingId"], json!("s1"));
}
```

Check the plugin's actual argument shapes before running: `signatures.complete` at `crates/plugins/signature/src/write/sign.rs:184-190`, `signatures.prepare` at `:150-154`, `signatures.revisionBytes` at `sync/hydration.rs:175` (a bare index), and the signature-core calls `grep -rn 'call("signatures' crates/plugins/signature-core/src` finds. Adjust the tests' inputs to what the plugin sends, never the reverse.

- [x] **Step 2: Run them to verify they fail**

Run: `cargo test -p epdf-viewer-host --test translate`
Expected: FAIL.

- [x] **Step 3: Implement**

Routes:

```rust
        "signatures.list" => ("signatures.list", Decode::Result),
        "signatures.contents" => ("signatures.contents", Decode::InlineBytes),
        "signatures.revisionBytes" => ("signatures.revisionBytes", Decode::InlineBytes),
        "signatures.analyze" => ("signatures.analyze", Decode::Result),
        "signatures.prepare" => ("signatures.prepare", Decode::ResultWithBytes("digest")),
        "signatures.complete" => ("signatures.complete", Decode::Mutation),
        "signatures.abort" => ("signatures.abort", Decode::Result),
        "document.version" => ("document.version", Decode::Result),
        "forms.setSignatureAppearance" => ("forms.setSignatureAppearance", Decode::Mutation),
```

Add the routed paths to `PATHS` (umbrella Task 0e), one line each in plan order: `"signatures.list"`, `"signatures.contents"`, `"signatures.revisionBytes"`, `"signatures.analyze"`, `"signatures.prepare"`, `"signatures.complete"`, `"signatures.abort"`, `"document.version"`, `"forms.setSignatureAppearance"`. `maps("signatures")` then answers through the namespace rule. `document.versioned` is a feature flag, not a path or a namespace of one, so add it to `FLAGS` (Task 0e):

```rust
    "document.versioned",
```

In `translate`, add arms to the `extra` match (plain `kind` patterns, like every other arm; `payloads` is the `let mut payloads` plan 10 added):

```rust
        "signatures.list" => vec![("workingCopy", arg(0).get("workingCopy").cloned().unwrap_or(Value::Null))],
        "signatures.contents" => vec![("ref", arg(0))],
        "signatures.revisionBytes" => vec![("revisionIndex", arg(0))],
        "signatures.analyze" | "signatures.prepare" => vec![("input", arg(0))],
        "signatures.abort" => vec![("signingId", arg(0))],
        "signatures.complete" => {
            payloads.push(bytes_at(&arg(0), "cms", path)?);
            vec![("input", without(arg(0), "cms"))]
        }
        "forms.setSignatureAppearance" => {
            payloads.push(bytes_at(&arg(1), "pdf", path)?);
            vec![
                ("ref", arg(0)),
                ("pageIndex", arg(1).get("pageIndex").cloned().unwrap_or(json!(0))),
            ]
        }
```

with

```rust
fn without(mut value: Value, key: &str) -> Value {
    if let Value::Object(map) = &mut value {
        map.remove(key);
    }
    value
}

fn bytes_at(value: &Value, key: &str, path: &str) -> Result<Vec<u8>, Error> {
    value
        .get(key)
        .and_then(epdf_kernel::bytes::Bytes::from_json)
        .map(|b| b.0)
        .ok_or_else(|| coded("invalid-input", format!("[epdf-viewer-host] {path} needs {key} bytes")))
}
```

`decode` gains the two arms, each taking the first payload and wrapping it with `Bytes(..).to_json()`. Update Appendix C's `signatures.*` row in `2026-10-01-swift-package.md` to `mapped (16-signature.md)`.

- [x] **Step 4: Run the tests**

Run: `cargo test -p epdf-viewer-host`
Expected: PASS.

- [x] **Step 5: Commit**

```bash
git add crates/epdf-viewer-host docs/superpowers/plans/2026-10-01-swift-package.md
git commit -m "Map the signature paths in the native engine"
```

### Task 7: Signature FFI records and read members

**Files:**
- Modify: `crates/plugins/signature/Cargo.toml`, `src/lib.rs`, `src/controller.rs`
- Create: `crates/plugins/signature/src/ffi/mod.rs`, `src/ffi/records.rs`, `crates/plugins/signature/tests/ffi.rs`

**Interfaces:**
- Consumes: `epdf_ffi::{AnnotationRef, CancelToken, KernelBound, KernelPost, PageRef, Subscription, ViewerError}`; plan 06's field reference record in `crates/epdf-ffi/src/form.rs`. The umbrella calls it `FieldId`; this plan needs it to be the engine's `FormFieldRef` (`{ObjectNumber {field_object_number: u32}, Fqn {name: String}}`). If plan 06 shipped another shape, write the two conversion functions at the top of `records.rs` against what it shipped and keep the rest of this task unchanged.
- Produces (`epdf_plugin_signature::ffi`): `SignatureHandle::new(cap: SignatureCapability, post: KernelPost) -> Arc<Self>`; records `SignatureSnapshot`, `SignatureRevision`, `SignatureInfo`, `SignatureWidget`, `SignatureSignerClaims`, `SignatureSeedValue`, `SignatureFieldLock`, `SignatureFieldLockAction`, `DocumentFieldLock`, `DocumentFieldLockSource`, `DocumentProtection`, `DocumentCertification`, `ModificationLevel`, `SignatureKind`, `SignatureCoverage`, `SignatureStatus`, `SignatureMode`, `SignaturePending`, `SignatureVerdict`, `SignatureIntegrity`, `SignatureCryptography`, `SignatureTrustStatus`, `SignatureModifications`, `SignatureModificationsVerdict`, `SignatureModificationsBasis`, `SignatureSummary`, `SignatureValidateRequest`, `SignatureValidationTime`, `SignatureEvent`; trait `SignatureEventListener`.

The members, from `crates/plugins/signature/src/harness.rs` `MEMBERS` (lines 52-91), under FFI names: `snapshot`, `status`, `signatures`, `unsigned_fields`, `signature`, `verdicts`, `verdict`, `protection`, `pending`, `is_busy`, `mode`, `target`, `refresh`, `validate`, `validate_field`, `analyze_changes`, `read_revision`, `set_target`, `request_inspection`, `sign`, `prepare_signature`, `complete_signature`, `abort_pending`, `fill_field`, `clear_field`, `place_mark`, `can_sign`, `can_fill`, `can_certify`, and `subscribe` for the nine `on*` hooks. This task does the reads, `refresh`, `validate`, `validate_field`, `analyze_changes`, `read_revision`, `set_target`, `request_inspection`, the `can_*` members and `subscribe`; Task 8 does the writes.

- [x] **Step 1: Feature and scaffolding**

`Cargo.toml` gets the `uniffi` feature, optional `epdf-ffi` and `uniffi`, dev-dependencies (`futures`, `plugin-conformance`, `rsa`, `epdf-signature-core` with `harness`), and `[[test]] name = "ffi" required-features = ["uniffi"]`, as plan 08 Task 3 Step 1 shows. `lib.rs` gets `#[cfg(feature = "uniffi")] pub mod ffi;` and `uniffi::setup_scaffolding!();`. `controller.rs` gets:

```rust
    /// The instance lifetime, aborted when the document closes.
    pub(crate) fn lifetime(&self) -> &epdf_kernel::signal::AbortSignal {
        self.0.cx.lifetime()
    }
```

- [x] **Step 2: Write the failing tests**

```rust
// crates/plugins/signature/tests/ffi.rs
#![cfg(feature = "uniffi")]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use epdf_ffi::{CancelToken, KernelThread, ViewerError};
use epdf_plugin_form::{FormConfig, form_plugin};
use epdf_plugin_interaction::{InteractionConfig, interaction_plugin};
use epdf_plugin_signature::ffi::{
    SignatureCoverage, SignatureEvent, SignatureEventListener, SignatureHandle, SignatureMode,
    SignatureStatus, SignatureSummary, SignatureValidateRequest,
};
use epdf_plugin_signature::{SIGNATURE, SignatureConfig, signature_plugin};
use futures::executor::block_on;
use plugin_conformance::test_kernel::{Reply, TestKernel, TestSpec};
use serde_json::{Value, json};

#[derive(Default)]
struct Events(Mutex<Vec<SignatureEvent>>, AtomicUsize);

impl SignatureEventListener for Events {
    fn on_event(&self, event: SignatureEvent) {
        self.0.lock().unwrap().push(event);
    }
    fn on_end(&self) {
        self.1.fetch_add(1, Ordering::SeqCst);
    }
}

fn field(n: u32) -> Value {
    json!({ "kind": "objectNumber", "fieldObjectNumber": n })
}

/// One signed field (12) over revision 0 of two, one unsigned (14).
fn snapshot() -> Value {
    let signer = json!({ "name": "Ana", "reason": null, "location": null, "contactInfo": null, "claimedTime": "D:20260901120000Z" });
    let base = |n: u32, signed: bool| json!({
        "index": if n == 12 { 0 } else { 1 }, "field": field(n), "fieldName": format!("Sig{n}"),
        "widget": { "ref": null, "annotObjectNumber": n + 1, "page": { "kind": "objectNumber", "pageObjectNumber": 1 } },
        "signed": signed, "kind": "signature", "filter": if signed { json!("Adobe.PPKLite") } else { Value::Null },
        "subFilter": if signed { json!("ETSI.CAdES.detached") } else { Value::Null },
        "byteRange": if signed { json!([0, 100, 200, 50]) } else { Value::Null },
        "contentsSize": if signed { 8192 } else { 0 },
        "coverage": if signed { json!("whole-revision") } else { Value::Null },
        "revisionIndex": if signed { json!(0) } else { Value::Null },
        "signer": signer, "docMdp": null, "catalogCertification": false,
        "fieldMdp": null, "lock": null, "seedValue": null
    });
    json!({
        "chainValid": true,
        "revisions": [
            { "index": 0, "end": 250, "xrefOffset": 180, "signatureIndex": 0 },
            { "index": 1, "end": 400, "xrefOffset": 330, "signatureIndex": null }
        ],
        "signatures": [base(12, true), base(14, false)],
        "protection": { "enforced": null, "judged": "fill", "certification": null, "fieldLocks": [], "policyVersion": 1 }
    })
}

fn spec() -> TestSpec {
    TestSpec::new(json!({
        "documents": { "a": {
            "pages": [{ "index": 0, "ref": { "kind": "objectNumber", "pageObjectNumber": 1 }, "label": null,
                        "size": { "width": 600, "height": 800 }, "rotation": 0, "userUnit": 1,
                        "boxes": { "media": { "left": 0, "bottom": 0, "right": 600, "top": 800 },
                                   "crop": { "left": 0, "bottom": 0, "right": 600, "top": 800 } } }],
            "security": { "allows": "*" },
            "services": ["signatures", "forms.setSignatureAppearance"]
        } }
    }))
    .respond("signatures.list", |_| Ok(Reply::value(snapshot())))
    .respond("signatures.contents", |_| Ok(Reply::value(json!({ "$bytes": "MAA=" }))))
    .respond("signatures.analyze", |_| Ok(Reply::value(json!({ "current": { "primary": null }, "laterRevisions": 1 }))))
    .respond("signatures.revisionBytes", |_| Ok(Reply::value(json!({ "$bytes": "JVBERi0=" }))))
    .respond("forms.list", |_| Ok(Reply::value(json!({ "fields": [] }))))
}

fn handle_with(config: SignatureConfig) -> (KernelThread<TestKernel>, Arc<SignatureHandle>) {
    let thread = KernelThread::spawn(move |_| {
        TestKernel::open(
            vec![
                interaction_plugin(InteractionConfig::default()),
                form_plugin(FormConfig::default()),
                signature_plugin(config),
            ],
            spec(),
        )
    })
    .unwrap();
    let post = thread.post();
    let handle = block_on(thread.call(move |k| SignatureHandle::new(k.capability(&SIGNATURE).unwrap(), post))).unwrap();
    (thread, handle)
}

fn handle() -> (KernelThread<TestKernel>, Arc<SignatureHandle>) {
    handle_with(SignatureConfig::default())
}

fn code(error: &ViewerError) -> Option<&str> {
    match error {
        ViewerError::Kernel { code, .. } => code.as_deref(),
        ViewerError::Closed => None,
    }
}

#[test]
fn refresh_decodes_the_snapshot() {
    let (_thread, s) = handle();
    let snapshot = block_on(s.refresh(CancelToken::new())).unwrap().unwrap();
    assert!(snapshot.chain_valid);
    assert_eq!(snapshot.revisions.len(), 2);
    assert_eq!(block_on(s.status()).unwrap(), SignatureStatus::Ready);
    let signed = block_on(s.signatures()).unwrap();
    assert_eq!(signed.len(), 1);
    assert_eq!(signed[0].coverage, Some(SignatureCoverage::WholeRevision));
    assert_eq!(signed[0].signer.name.as_deref(), Some("Ana"));
    assert_eq!(block_on(s.unsigned_fields()).unwrap().len(), 1);
}

#[test]
fn a_snapshot_the_record_cannot_hold_is_invalid_state() {
    let thread = KernelThread::spawn(|_| {
        TestKernel::open(
            vec![interaction_plugin(InteractionConfig::default()), form_plugin(FormConfig::default()), signature_plugin(SignatureConfig::default())],
            spec().respond("signatures.list", |_| Ok(Reply::value(json!({ "chainValid": "yes" })))),
        )
    })
    .unwrap();
    let post = thread.post();
    let s = block_on(thread.call(move |k| SignatureHandle::new(k.capability(&SIGNATURE).unwrap(), post))).unwrap();
    let error = block_on(s.refresh(CancelToken::new())).unwrap_err();
    assert_eq!(code(&error), Some("invalid-state"), "{error:?}");
}

#[test]
fn without_anchors_a_good_signature_is_at_best_valid_untrusted() {
    let (_thread, s) = handle();
    block_on(s.refresh(CancelToken::new())).unwrap();
    let verdicts = block_on(s.validate(SignatureValidateRequest::default(), CancelToken::new())).unwrap();
    assert_eq!(verdicts.len(), 1);
    // The fake CMS is not a real signature, so this one is indeterminate or
    // invalid; it is never `Valid` without anchors.
    assert_ne!(verdicts[0].summary, SignatureSummary::Valid);
}

#[test]
fn mode_defaults_to_visual_without_a_signer() {
    let (_thread, s) = handle();
    assert_eq!(block_on(s.mode()).unwrap(), SignatureMode::Visual);
    assert!(!block_on(s.can_sign()).unwrap());
    assert!(block_on(s.can_fill()).unwrap());
    assert!(!block_on(s.can_certify()).unwrap());
}

#[test]
fn set_target_is_observed() {
    let (_thread, s) = handle();
    let events = Arc::new(Events::default());
    let _sub = s.subscribe(events.clone());
    block_on(s.refresh(CancelToken::new())).unwrap();
    let unsigned = block_on(s.unsigned_fields()).unwrap();
    block_on(s.set_target(Some(unsigned[0].field.clone()))).unwrap();
    assert_eq!(block_on(s.target()).unwrap(), Some(unsigned[0].field.clone()));
    assert!(events.0.lock().unwrap().iter().any(|e| matches!(e, SignatureEvent::TargetChanged { .. })));
}

#[test]
fn read_revision_answers_bytes() {
    let (_thread, s) = handle();
    block_on(s.refresh(CancelToken::new())).unwrap();
    let field = block_on(s.signatures()).unwrap()[0].field.clone();
    let bytes = block_on(s.read_revision(field, CancelToken::new())).unwrap();
    assert!(bytes.starts_with(b"%PDF-"));
}

#[test]
fn a_closed_documents_capability_refuses_with_instance_closed() {
    let (thread, s) = handle();
    block_on(thread.call(|k| k.close())).unwrap().unwrap();
    for error in [
        block_on(s.snapshot()).map(|_| ()).unwrap_err(),
        block_on(s.status()).map(|_| ()).unwrap_err(),
        block_on(s.refresh(CancelToken::new())).map(|_| ()).unwrap_err(),
    ] {
        assert_eq!(code(&error), Some("instance-closed"), "{error:?}");
    }
}

#[test]
fn closing_the_document_ends_its_subscriptions() {
    let (thread, s) = handle();
    let events = Arc::new(Events::default());
    let _sub = s.subscribe(events.clone());
    block_on(thread.call(|k| k.close())).unwrap().unwrap();
    assert_eq!(events.1.load(Ordering::SeqCst), 1);
}
```

- [x] **Step 3: Run them to verify they fail**

Run: `cargo test -p epdf-plugin-signature --features uniffi --test ffi`
Expected: FAIL to compile, no `ffi` module.

- [x] **Step 4: Write the records**

`crates/plugins/signature/src/ffi/records.rs` mirrors `types.ts:56-197` and `verdict.ts:12-47` field for field. Each mirror has a private serde twin it decodes through, so the JSON shape is written once:

```rust
//! The engine's signature DTOs (`types.ts`) and `epdf-signature-core`'s
//! verdicts, as typed records. The plugin keeps them as JSON; each record
//! here decodes from that JSON and a value that doesn't is `invalid-state`.

use epdf_ffi::{AnnotationRef, FormFieldRef, PageRef};
use epdf_kernel::Error;
use serde::Deserialize;
use serde_json::Value;

pub(crate) fn invalid_state(what: &str, e: impl std::fmt::Display) -> Error {
    Error::named("InvalidState", format!("[signature] {what} does not decode: {e}"))
        .with("code", "invalid-state")
}

pub(crate) fn decode<T: serde::de::DeserializeOwned>(what: &str, value: &Value) -> Result<T, Error> {
    serde_json::from_value(value.clone()).map_err(|e| invalid_state(what, e))
}

pub(crate) fn field_json(field: &FormFieldRef) -> Value {
    match field {
        FormFieldRef::ObjectNumber { field_object_number } => {
            serde_json::json!({ "kind": "objectNumber", "fieldObjectNumber": field_object_number })
        }
        FormFieldRef::Fqn { name } => serde_json::json!({ "kind": "fqn", "name": name }),
    }
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum FieldRefJson {
    #[serde(rename_all = "camelCase")]
    ObjectNumber { field_object_number: u32 },
    Fqn { name: String },
}

impl From<FieldRefJson> for FormFieldRef {
    fn from(f: FieldRefJson) -> Self {
        match f {
            FieldRefJson::ObjectNumber { field_object_number } => FormFieldRef::ObjectNumber { field_object_number },
            FieldRefJson::Fqn { name } => FormFieldRef::Fqn { name },
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, uniffi::Enum)]
#[serde(rename_all = "lowercase")]
pub enum SignatureKind { Signature, Timestamp }

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, uniffi::Enum)]
#[serde(rename_all = "kebab-case")]
pub enum SignatureCoverage { WholeRevision, Partial, Malformed }

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, uniffi::Enum)]
#[serde(rename_all = "lowercase")]
pub enum ModificationLevel { None, Lta, Fill, Annotate }

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, uniffi::Enum)]
#[serde(rename_all = "lowercase")]
pub enum SignatureFieldLockAction { All, Include, Exclude }

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, uniffi::Record)]
pub struct SignatureFieldLock {
    pub action: SignatureFieldLockAction,
    pub fields: Vec<String>,
    #[serde(default)]
    pub permission: Option<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct SignatureRevision {
    pub index: u32,
    pub end: u64,
    pub xref_offset: u64,
    pub signature_index: Option<u32>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct SignatureSignerClaims {
    pub name: Option<String>,
    pub reason: Option<String>,
    pub location: Option<String>,
    pub contact_info: Option<String>,
    /// `/M` as written, a PDF date string. A claim, not verified.
    pub claimed_time: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, uniffi::Record)]
#[serde(rename_all = "camelCase")]
pub struct SignatureSeedValue {
    pub required_flags: u32,
    pub present_flags: u32,
    pub version: Option<u32>,
    pub mdp: Option<u8>,
    pub filter: Option<String>,
    pub sub_filters: Vec<String>,
    pub digest_methods: Vec<String>,
    pub reasons: Vec<String>,
    pub unsupported_required: bool,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct SignatureWidget {
    pub annotation: Option<AnnotationRef>,
    pub annot_object_number: u32,
    pub page: Option<PageRef>,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct SignatureInfo {
    pub index: u32,
    pub field: FormFieldRef,
    pub field_name: String,
    pub widget: Option<SignatureWidget>,
    pub signed: bool,
    pub kind: SignatureKind,
    pub filter: Option<String>,
    pub sub_filter: Option<String>,
    pub byte_range: Option<Vec<u64>>,
    pub contents_size: u64,
    pub coverage: Option<SignatureCoverage>,
    pub revision_index: Option<u32>,
    pub signer: SignatureSignerClaims,
    pub doc_mdp: Option<u8>,
    pub catalog_certification: bool,
    pub field_mdp: Option<SignatureFieldLock>,
    pub lock: Option<SignatureFieldLock>,
    pub seed_value: Option<SignatureSeedValue>,
}
```

and the rest in the same pattern: `SignatureInfo` decodes through a `SignatureInfoJson` twin (because `field` and `widget` need the conversions above), `DocumentProtection { enforced: Option<ModificationLevel>, judged: Option<ModificationLevel>, certification: Option<DocumentCertification { signature_index: u32, permission: u8 }>, field_locks: Vec<DocumentFieldLock { signature_index: u32, source: DocumentFieldLockSource /* Fieldmdp | Lock */, spec: SignatureFieldLock }>, policy_version: u32 }`, `SignatureSnapshot { chain_valid, revisions, signatures: Vec<SignatureInfo>, protection }`, `SignaturePending { signing_id: String, field: FormFieldRef }`, `SignatureStatus` (the plugin's `ResourceStatus`), `SignatureMode { Sign, Visual, Ask }`, and the verdict:

```rust
#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct SignatureVerdict {
    pub signature: SignatureInfo,
    pub integrity: SignatureIntegrity,          // Valid | Invalid | Indeterminate
    pub cryptography: SignatureCryptography,    // the CryptographyVerdict strings, kebab-case
    pub trust: SignatureTrustStatus,            // Trusted | Untrusted | Unknown
    pub trust_reason: Option<String>,
    pub validated_at: SignatureValidationTime,  // Now | Timestamp { gen_time_ms: f64 }
    pub modifications: SignatureModifications,
    pub summary: SignatureSummary,              // Valid | ValidUntrusted | Invalid | Indeterminate
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct SignatureModifications {
    pub verdict: SignatureModificationsVerdict, // Unchanged | Permitted | Forbidden | Indeterminate
    pub detail: Option<String>,
    pub basis: SignatureModificationsBasis,     // Persisted | WorkingCopy
    pub later_revisions: Option<u32>,
    pub undone: Option<bool>,
}
```

`cms: ParsedCms` isn't exported: it's a parse tree a UI doesn't need, and `signature` plus `trust_reason` carry what one shows. The `CryptographyVerdict` strings are whatever `crates/plugins/signature-core/src/verdict.rs` writes into `"cryptography"`; enumerate them from that file. `validatedAt.genTime` is `{"$date": ms}` in the Rust JSON (`trust.rs:45`).

Events mirror `contract.rs:183-245`: `SignatureEvent { Signed { field, result: SignatureCompletion }, Filled { field }, Cleared { field }, Validated { verdicts: Vec<SignatureVerdict> }, ProtectionChanged { protection: DocumentProtection }, Invalidating { field, detail: String }, TargetChanged { field: Option<FormFieldRef> }, SignRequested { field, mark: SignatureMark }, InspectionRequested { field } }`, with `origin` dropped as metadata drops `ChangeOrigin`. `SignatureCompletion` mirrors `SignatureCompleteResult` (`status: SignatureCompletionStatus { Completed, AlreadyCompleted }`, `signature: SignatureInfo`, `version: SignatureBaseVersion { sha256, byte_length }`, `previous: SignatureVersionRef { base_sha256, edits_version }`, `protection`), dropping `meta`. `SignatureMark { Asset { asset_id: String }, Source { bytes: Vec<u8> } }` lands in Task 8 but is declared here because the event uses it.

- [x] **Step 5: Write the handle's read members**

`crates/plugins/signature/src/ffi/mod.rs`:

```rust
//! The signature plugin through uniffi. Records in [`records`], the host
//! ports in [`ports`].

pub mod ports;
pub mod records;

use std::sync::Arc;

use epdf_ffi::{CancelToken, FormFieldRef, KernelBound, KernelPost, Subscription, ViewerError};
use epdf_kernel::Error;
use epdf_kernel::error::{code, plugin_error};
use serde_json::Value;

pub use records::*;

use crate::SignatureCapability;
use crate::sync::hydration::ValidateRequest;

fn open(cap: &SignatureCapability) -> Result<&SignatureCapability, Error> {
    if cap.lifetime().aborted() {
        return Err(plugin_error(code::INSTANCE_CLOSED, "signature", "the document this capability belongs to is closed", None, None));
    }
    Ok(cap)
}

fn infos(list: Vec<Value>) -> Result<Vec<SignatureInfo>, Error> {
    list.iter().map(SignatureInfo::from_json).collect()
}

#[derive(uniffi::Object)]
pub struct SignatureHandle {
    pub(crate) cap: KernelBound<SignatureCapability>,
    pub(crate) post: KernelPost,
}

impl SignatureHandle {
    /// On the kernel thread.
    pub fn new(cap: SignatureCapability, post: KernelPost) -> Arc<Self> {
        Arc::new(SignatureHandle { cap: KernelBound::new(cap, post.clone()), post })
    }
}

#[uniffi::export]
impl SignatureHandle {
    pub async fn snapshot(&self) -> Result<Option<SignatureSnapshot>, ViewerError> {
        self.cap.call(|c| open(c)?.get_snapshot().as_ref().map(SignatureSnapshot::from_json).transpose()).await
    }

    pub async fn status(&self) -> Result<SignatureStatus, ViewerError> {
        self.cap.call(|c| Ok(open(c)?.get_status().into())).await
    }

    /// The signed fields.
    pub async fn signatures(&self) -> Result<Vec<SignatureInfo>, ViewerError> {
        self.cap.call(|c| infos(open(c)?.list_signatures())).await
    }

    pub async fn unsigned_fields(&self) -> Result<Vec<SignatureInfo>, ViewerError> {
        self.cap.call(|c| infos(open(c)?.list_unsigned_fields())).await
    }

    pub async fn signature(&self, field: FormFieldRef) -> Result<Option<SignatureInfo>, ViewerError> {
        self.cap
            .call(move |c| open(c)?.get_signature(&field_json(&field)).as_ref().map(SignatureInfo::from_json).transpose())
            .await
    }

    pub async fn verdicts(&self) -> Result<Option<Vec<SignatureVerdict>>, ViewerError> {
        self.cap.call(|c| open(c)?.list_verdicts().as_ref().map(SignatureVerdict::list_from_json).transpose()).await
    }

    pub async fn verdict(&self, field: FormFieldRef) -> Result<Option<SignatureVerdict>, ViewerError> {
        self.cap
            .call(move |c| open(c)?.get_verdict(&field_json(&field)).as_ref().map(SignatureVerdict::from_json).transpose())
            .await
    }

    pub async fn protection(&self) -> Result<Option<DocumentProtection>, ViewerError> {
        self.cap.call(|c| open(c)?.get_protection().as_ref().map(DocumentProtection::from_json).transpose()).await
    }

    /// A parked two-phase signing: the document is read-only until it
    /// completes or is aborted.
    pub async fn pending(&self) -> Result<Option<SignaturePending>, ViewerError> {
        self.cap.call(|c| open(c)?.get_pending().map(SignaturePending::try_from).transpose()).await
    }

    pub async fn is_busy(&self) -> Result<bool, ViewerError> {
        self.cap.call(|c| Ok(open(c)?.is_busy())).await
    }

    pub async fn mode(&self) -> Result<SignatureMode, ViewerError> {
        self.cap.call(|c| Ok(open(c)?.get_mode().into())).await
    }

    pub async fn target(&self) -> Result<Option<FormFieldRef>, ViewerError> {
        self.cap.call(|c| open(c)?.get_target().as_ref().map(field_from_json).transpose()).await
    }

    /// Re-read the snapshot. `None` when the engine has no signatures.
    pub async fn refresh(&self, cancel: Arc<CancelToken>) -> Result<Option<SignatureSnapshot>, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                |c| Ok(open(c)?.refresh()),
                |s: Option<Value>| s.as_ref().map(SignatureSnapshot::from_json).transpose().map_err(ViewerError::from),
            )
            .await?
    }

    /// Judge every signature. Without trust anchors a good one tops out at
    /// `validUntrusted`.
    pub async fn validate(
        &self,
        request: SignatureValidateRequest,
        cancel: Arc<CancelToken>,
    ) -> Result<Vec<SignatureVerdict>, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(open(c)?.validate(ValidateRequest::from(request))),
                |v: Value| SignatureVerdict::list_from_json(&v).map_err(ViewerError::from),
            )
            .await?
    }

    pub async fn validate_field(
        &self,
        field: FormFieldRef,
        request: SignatureValidateRequest,
        cancel: Arc<CancelToken>,
    ) -> Result<SignatureVerdict, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(open(c)?.validate_field(field_json(&field), ValidateRequest::from(request))),
                |v: Value| SignatureVerdict::from_json(&v).map_err(ViewerError::from),
            )
            .await?
    }

    /// What changed after `since` (a signature index), up to the saved
    /// bytes or the working copy.
    pub async fn analyze_changes(
        &self,
        since: u32,
        until: SignatureModificationsBasis,
        cancel: Arc<CancelToken>,
    ) -> Result<SignatureChangeAnalysis, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| {
                    let input = serde_json::json!({ "since": { "signatureIndex": since }, "until": until.as_str() });
                    open(c)?.analyze_changes(input)
                },
                |v: Value| SignatureChangeAnalysis::from_json(&v).map_err(ViewerError::from),
            )
            .await?
    }

    /// The bytes the signature on `field` seals: its revision, a complete
    /// PDF of its own.
    pub async fn read_revision(&self, field: FormFieldRef, cancel: Arc<CancelToken>) -> Result<Vec<u8>, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| open(c)?.read_revision(&serde_json::json!({ "field": field_json(&field) })),
                |v: Value| {
                    epdf_kernel::bytes::Bytes::from_json(&v)
                        .map(|b| b.0)
                        .ok_or_else(|| ViewerError::from(records::invalid_state("revision bytes", &v)))
                },
            )
            .await?
    }

    pub async fn set_target(&self, field: Option<FormFieldRef>) -> Result<(), ViewerError> {
        self.cap.call(move |c| {
            open(c)?.set_target(field.as_ref().map(field_json));
            Ok(())
        })
        .await
    }

    pub async fn request_inspection(&self, field: FormFieldRef) -> Result<(), ViewerError> {
        self.cap.call(move |c| {
            open(c)?.request_inspection(field_json(&field));
            Ok(())
        })
        .await
    }

    /// `doc.sign` is granted and a signer is configured.
    pub async fn can_sign(&self) -> Result<bool, ViewerError> {
        self.cap.call(|c| Ok(open(c)?.can_sign())).await
    }

    pub async fn can_fill(&self) -> Result<bool, ViewerError> {
        self.cap.call(|c| Ok(open(c)?.can_fill())).await
    }

    pub async fn can_certify(&self) -> Result<bool, ViewerError> {
        self.cap.call(|c| Ok(open(c)?.can_certify())).await
    }

    pub fn subscribe(&self, listener: Arc<dyn SignatureEventListener>) -> Arc<Subscription> {
        let (subscription, slot) = Subscription::pending(self.post.clone());
        self.cap.fire(move |c| {
            let ending = listener.clone();
            slot.fill_until_end(c.lifetime(), || records::register_hooks(c, listener), move || ending.on_end());
        });
        subscription
    }
}
```

`SignatureChangeAnalysis` is a small record in `records.rs` over what `verdict.rs:185-200` reads (`primary: Option<SignatureChangePrimary { verdict, object_number, detail }>`, `later_revisions`, `undone`). `records::register_hooks` registers the nine `on_*` hooks with one decoding closure each and returns one `Unsubscribe`, the same shape as plan 08's `subscribe`. `SignatureValidateRequest { at: Option<SignatureValidationTime>, until: Option<SignatureModificationsBasis> }` converts into the plugin's `ValidateRequest`. Write `from_json` on each record as `decode` into its serde twin, then convert.

- [x] **Step 6: Run the tests**

Run: `cargo test -p epdf-plugin-signature --features uniffi --test ffi && cargo test -p epdf-plugin-signature`
Expected: PASS, 8 tests, and the conformance vectors.

- [x] **Step 7: Commit**

```bash
git add crates/plugins/signature
git commit -m "Export the signature plugin's reads and validation through uniffi"
```

### Task 8: Signing members and the host ports

**Files:**
- Create: `crates/plugins/signature/src/ffi/ports.rs`
- Modify: `crates/plugins/signature/src/ffi/mod.rs`, `crates/plugins/signature/tests/ffi.rs`

**Interfaces:**
- Produces: foreign traits `SignatureSigner` and `SignatureTrust`; enums `SignatureKeyAlgorithm { RsaPkcs1, RsaPss, Ecdsa }`, `SignatureDigest { Sha256, Sha384, Sha512 }`; records `SignatureMark`, `SignatureSignInput`, `SignaturePrepareInput`, `SignatureAttribution`, `SignaturePrepared`, `SignaturePlaceTarget`, `SignaturePlaceResult`, `SignatureCompletion`; adapters `ports::signer_port(Arc<dyn SignatureSigner>) -> SignerPort` and `ports::trust_port(Arc<dyn SignatureTrust>) -> Rc<dyn TrustPort>`, used by Task 9.

- [x] **Step 1: Write the failing tests**

Append to `crates/plugins/signature/tests/ffi.rs`:

```rust
use epdf_plugin_signature::ffi::{
    SignatureDigest, SignatureKeyAlgorithm, SignatureMark, SignatureSignInput, SignatureSigner,
};
use rsa::pkcs1v15::SigningKey;
use rsa::pkcs8::DecodePrivateKey;
use rsa::signature::{SignatureEncoding, Signer};

const KEYS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../../packages/engine/main/test/fixtures/signature-compat/keys");

struct TestSigner { key: rsa::RsaPrivateKey, chain: Vec<Vec<u8>>, fail: bool }

impl TestSigner {
    fn load(fail: bool) -> Self {
        let pem = std::fs::read_to_string(format!("{KEYS}/TEST-ONLY-private-key.pem")).unwrap();
        let cert = std::fs::read_to_string(format!("{KEYS}/TEST-ONLY-signer.pem")).unwrap();
        let der = pem_body(&cert);
        TestSigner { key: rsa::RsaPrivateKey::from_pkcs8_pem(&pem).unwrap(), chain: vec![der], fail }
    }
}

fn pem_body(pem: &str) -> Vec<u8> {
    use base64::Engine as _;
    let body: String = pem.lines().filter(|l| !l.starts_with("-----")).collect();
    base64::engine::general_purpose::STANDARD.decode(body).unwrap()
}

impl SignatureSigner for TestSigner {
    fn certificate_chain(&self) -> Result<Vec<Vec<u8>>, ViewerError> { Ok(self.chain.clone()) }
    fn algorithm(&self) -> SignatureKeyAlgorithm { SignatureKeyAlgorithm::RsaPkcs1 }
    fn digest(&self) -> SignatureDigest { SignatureDigest::Sha256 }
    fn sign(&self, message: Vec<u8>) -> Result<Vec<u8>, ViewerError> {
        if self.fail {
            return Err(ViewerError::Kernel { name: "UserCancelled".into(), code: Some("operation-cancelled".into()), detail: "the user cancelled".into() });
        }
        Ok(SigningKey::<sha2::Sha256>::new(self.key.clone()).sign(&message).to_vec())
    }
}

/// `spec()` plus prepare and complete responders that record the CMS.
fn signing_spec(cms: Arc<Mutex<Option<Value>>>) -> TestSpec {
    spec()
        .respond("signatures.prepare", |_| Ok(Reply::value(json!({
            "signingId": "s1", "digest": { "$bytes": "AAECAwQFBgcICQoLDA0ODxAREhMUFRYXGBkaGxwdHh8=" },
            "algorithm": "sha256", "byteRange": [0, 100, 200, 50], "contentsSize": 8192,
            "subFilter": "ETSI.CAdES.detached", "expectedVersion": { "baseSha256": "x", "editsVersion": 0 }, "expiresAt": null
        }))))
        .respond("signatures.complete", move |args| {
            *cms.lock().unwrap() = args.first().and_then(|a| a.get("cms")).cloned();
            Ok(Reply::value(json!({
                "status": "completed", "signature": snapshot()["signatures"][1].clone(),
                "version": { "sha256": "y", "byteLength": 500 }, "previous": { "baseSha256": "x", "editsVersion": 0 },
                "protection": snapshot()["protection"].clone()
            })))
        })
}

#[test]
fn signing_hands_the_engine_a_cms_from_the_foreign_signer() {
    let cms = Arc::new(Mutex::new(None));
    let seen = cms.clone();
    let thread = KernelThread::spawn(move |_| {
        TestKernel::open(
            vec![interaction_plugin(InteractionConfig::default()), form_plugin(FormConfig::default()), signature_plugin(SignatureConfig::default())],
            signing_spec(seen),
        )
    })
    .unwrap();
    let post = thread.post();
    let s = block_on(thread.call(move |k| SignatureHandle::new(k.capability(&SIGNATURE).unwrap(), post))).unwrap();
    block_on(s.refresh(CancelToken::new())).unwrap();
    let field = block_on(s.unsigned_fields()).unwrap()[0].field.clone();
    let input = SignatureSignInput { field, mark: None, attribution: None, certify: None, lock: None };
    let done = block_on(s.sign(input, Some(Arc::new(TestSigner::load(false))), CancelToken::new())).unwrap();
    assert!(done.signature.signed);
    let cms = cms.lock().unwrap().clone().unwrap();
    let der = epdf_kernel::bytes::Bytes::from_json(&cms).unwrap().0;
    assert_eq!(der[0], 0x30);
    assert_eq!(block_on(s.pending()).unwrap(), None);
}

#[test]
fn a_throwing_signer_leaves_nothing_pending() {
    let thread = KernelThread::spawn(|_| {
        TestKernel::open(
            vec![interaction_plugin(InteractionConfig::default()), form_plugin(FormConfig::default()), signature_plugin(SignatureConfig::default())],
            signing_spec(Arc::default()).respond("signatures.abort", |_| Ok(Reply::value(json!({ "status": "aborted" })))),
        )
    })
    .unwrap();
    let post = thread.post();
    let s = block_on(thread.call(move |k| SignatureHandle::new(k.capability(&SIGNATURE).unwrap(), post))).unwrap();
    block_on(s.refresh(CancelToken::new())).unwrap();
    let field = block_on(s.unsigned_fields()).unwrap()[0].field.clone();
    let input = SignatureSignInput { field, mark: None, attribution: None, certify: None, lock: None };
    let error = block_on(s.sign(input, Some(Arc::new(TestSigner::load(true))), CancelToken::new())).unwrap_err();
    assert_eq!(code(&error), Some("operation-cancelled"), "{error:?}");
    assert_eq!(block_on(s.pending()).unwrap(), None);
    assert!(!block_on(s.is_busy()).unwrap());
}

#[test]
fn signing_without_a_signer_is_invalid_input() {
    let (_thread, s) = handle();
    block_on(s.refresh(CancelToken::new())).unwrap();
    let field = block_on(s.unsigned_fields()).unwrap()[0].field.clone();
    let input = SignatureSignInput { field, mark: None, attribution: None, certify: None, lock: None };
    let error = block_on(s.sign(input, None, CancelToken::new())).unwrap_err();
    assert_eq!(code(&error), Some("invalid-input"), "{error:?}");
}

#[test]
fn a_visual_fill_reaches_the_engine() {
    let filled = Arc::new(Mutex::new(false));
    let saw = filled.clone();
    let thread = KernelThread::spawn(move |_| {
        TestKernel::open(
            vec![interaction_plugin(InteractionConfig::default()), form_plugin(FormConfig::default()), signature_plugin(SignatureConfig::default())],
            spec().respond("forms.setSignatureAppearance", move |_| {
                *saw.lock().unwrap() = true;
                Ok(Reply::value(json!({})))
            }),
        )
    })
    .unwrap();
    let post = thread.post();
    let s = block_on(thread.call(move |k| SignatureHandle::new(k.capability(&SIGNATURE).unwrap(), post))).unwrap();
    block_on(s.refresh(CancelToken::new())).unwrap();
    let field = block_on(s.unsigned_fields()).unwrap()[0].field.clone();
    let artwork = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/../../../packages/engine/main/test/fixtures/signature_artwork.pdf")).unwrap();
    block_on(s.fill_field(field, SignatureMark::Source { bytes: artwork }, CancelToken::new())).unwrap();
    assert!(*filled.lock().unwrap());
}
```

Review Focus item 3 (`a_changed_document_validates_invalid`) and item 5 (`without_anchors_a_good_signature_is_valid_untrusted`) need a real CMS over real bytes, so they're host tests over PDFium in Task 9, not here.

- [x] **Step 2: Run them to verify they fail**

Run: `cargo test -p epdf-plugin-signature --features uniffi --test ffi`
Expected: FAIL to compile, no `SignatureSigner`.

- [x] **Step 3: Write `ports.rs`**

```rust
//! The host ports, implemented in Swift: a key that signs, and the anchors
//! validation trusts. Both are called on the kernel thread, synchronously.

use std::rc::Rc;
use std::sync::Arc;

use epdf_ffi::ViewerError;
use epdf_kernel::Error;
use epdf_kernel::rt::Promise;
use epdf_signature_core::cms::{RawAlgorithm, RawSigner};
use epdf_signature_core::oids::Digest;
use epdf_signature_core::{SignerPort, TrustPort};

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum SignatureKeyAlgorithm { RsaPkcs1, RsaPss, Ecdsa }

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum SignatureDigest { Sha256, Sha384, Sha512 }

/// A private key that never leaves the host, and its certificate chain
/// (leaf first, DER). `sign` gets the DER of the CMS signed attributes and
/// hashes it itself with `digest`. An ECDSA key answers `r || s`, each
/// padded to the curve's size, not X9.62 DER.
#[uniffi::export(with_foreign)]
pub trait SignatureSigner: Send + Sync {
    fn certificate_chain(&self) -> Result<Vec<Vec<u8>>, ViewerError>;
    fn algorithm(&self) -> SignatureKeyAlgorithm;
    fn digest(&self) -> SignatureDigest;
    fn sign(&self, message: Vec<u8>) -> Result<Vec<u8>, ViewerError>;
}

/// Trust anchors, DER. An empty list means every verdict tops out at
/// `validUntrusted`.
#[uniffi::export(with_foreign)]
pub trait SignatureTrust: Send + Sync {
    fn anchors(&self) -> Result<Vec<Vec<u8>>, ViewerError>;
}

fn kernel_error(error: ViewerError) -> Error {
    match error {
        ViewerError::Kernel { name, code, detail } => {
            let error = Error::named(name, detail);
            match code {
                Some(code) => error.with("code", code),
                None => error,
            }
        }
        ViewerError::Closed => Error::new("[signature] the host port is closed"),
    }
}

/// The plugin's `SignerPort` over a Swift signer. The chain is read once
/// per signing, when the plugin resolves the port.
pub fn signer_port(signer: Arc<dyn SignatureSigner>) -> Result<SignerPort, Error> {
    let certificate_chain = signer.certificate_chain().map_err(kernel_error)?;
    let algorithm = match signer.algorithm() {
        SignatureKeyAlgorithm::RsaPkcs1 => RawAlgorithm::RsaPkcs1,
        SignatureKeyAlgorithm::RsaPss => RawAlgorithm::RsaPss,
        SignatureKeyAlgorithm::Ecdsa => RawAlgorithm::Ecdsa,
    };
    let hash = match signer.digest() {
        SignatureDigest::Sha256 => Digest::Sha256,
        SignatureDigest::Sha384 => Digest::Sha384,
        SignatureDigest::Sha512 => Digest::Sha512,
    };
    Ok(SignerPort::Raw(RawSigner {
        certificate_chain,
        algorithm,
        hash,
        sign: Rc::new(move |message| match signer.sign(message) {
            Ok(signature) => Promise::resolved(signature),
            Err(error) => Promise::rejected(kernel_error(error)),
        }),
    }))
}

struct ForeignTrust(Arc<dyn SignatureTrust>);

impl TrustPort for ForeignTrust {
    fn anchors(&self) -> Promise<Vec<Vec<u8>>> {
        match self.0.anchors() {
            Ok(anchors) => Promise::resolved(anchors),
            Err(error) => Promise::rejected(kernel_error(error)),
        }
    }
}

pub fn trust_port(trust: Arc<dyn SignatureTrust>) -> Rc<dyn TrustPort> {
    Rc::new(ForeignTrust(trust))
}
```

A fallible foreign trait needs `impl From<uniffi::UnexpectedUniFFICallbackError> for ViewerError` in `epdf-ffi`. Plan 01 is the first plan with a foreign trait and should have added it; if it's missing, add it in `crates/epdf-ffi/src/error.rs`, mapping to `ViewerError::Kernel { name: "CallbackError", code: Some("internal"), detail: reason }`.

- [x] **Step 4: Write the signing members**

In `ffi/mod.rs`, the inputs (in `records.rs`):

```rust
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum SignatureMark {
    /// A stamp-library asset (needs the stamp plugin).
    Asset { asset_id: String },
    /// Artwork bytes: a PDF, PNG or JPEG.
    Source { bytes: Vec<u8> },
}

#[derive(Clone, Debug, Default, PartialEq, uniffi::Record)]
pub struct SignatureAttribution {
    #[uniffi(default = None)] pub name: Option<String>,
    #[uniffi(default = None)] pub reason: Option<String>,
    #[uniffi(default = None)] pub location: Option<String>,
    #[uniffi(default = None)] pub contact_info: Option<String>,
}

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
pub struct SignatureSignInput {
    pub field: FormFieldRef,
    #[uniffi(default = None)] pub mark: Option<SignatureMark>,
    #[uniffi(default = None)] pub attribution: Option<SignatureAttribution>,
    /// The DocMDP permission (1, 2 or 3) to certify with. Needs
    /// `allowCertify` and `doc.sign.certify`, and no earlier signature.
    #[uniffi(default = None)] pub certify: Option<u8>,
    #[uniffi(default = None)] pub lock: Option<SignatureFieldLock>,
}
```

`SignaturePrepareInput` adds `sub_filter: Option<String>` and `digest: Option<SignatureDigest>` to the same fields; `SignaturePrepared` mirrors `types.ts:236-246` with `digest: Vec<u8>`; `SignaturePlaceTarget { Field { field: FormFieldRef }, Free { page: PageRef, at: Point, target_width: Option<f64>, rotation: Option<f64> } }`; `SignaturePlaceResult { Signed { field, result: SignatureCompletion }, Filled { field }, Requested { field }, Placed { annotation: AnnotationRef } }` from `contract.rs:146-168`. `SignatureLock` for `SignatureFieldLock` needs a `to_json` writing the `types.ts:59-65` shape.

The members:

```rust
    /// Seal `input.field` with `signer`, or the signer the viewer has
    /// (`PDFViewer.setSigner`). Fails with the signer's own error if it
    /// throws, and leaves nothing pending.
    pub async fn sign(
        &self,
        input: SignatureSignInput,
        signer: Option<Arc<dyn ports::SignatureSigner>>,
        cancel: Arc<CancelToken>,
    ) -> Result<SignatureCompletion, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| {
                    let signer = signer.map(ports::signer_port).transpose()?.map(crate::SignerSource::Port);
                    Ok(open(c)?.sign(input.into_core(signer)))
                },
                |v: Value| SignatureCompletion::from_json(&v).map_err(ViewerError::from),
            )
            .await?
    }

    /// Phase one of an external signing: the digest to sign elsewhere. The
    /// document is read-only until `complete_signature` or `abort_pending`.
    pub async fn prepare_signature(&self, input: SignaturePrepareInput, cancel: Arc<CancelToken>) -> Result<SignaturePrepared, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(open(c)?.prepare_signature(input.into_core())),
                |v: Value| SignaturePrepared::from_json(&v).map_err(ViewerError::from),
            )
            .await?
    }

    pub async fn complete_signature(&self, signing_id: String, cms: Vec<u8>, cancel: Arc<CancelToken>) -> Result<SignatureCompletion, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(open(c)?.complete_signature(signing_id, cms)),
                |v: Value| SignatureCompletion::from_json(&v).map_err(ViewerError::from),
            )
            .await?
    }

    pub async fn abort_pending(&self, cancel: Arc<CancelToken>) -> Result<(), ViewerError> {
        self.cap.call_promise_cancellable(cancel, |c| Ok(open(c)?.abort_pending()), |()| ()).await
    }

    /// Draw `mark` into an unsigned field without sealing it.
    pub async fn fill_field(&self, field: FormFieldRef, mark: SignatureMark, cancel: Arc<CancelToken>) -> Result<(), ViewerError> {
        self.cap
            .call_promise_cancellable(cancel, move |c| Ok(open(c)?.fill_field(field_json(&field), mark.into())), |()| ())
            .await
    }

    pub async fn clear_field(&self, field: FormFieldRef, cancel: Arc<CancelToken>) -> Result<(), ViewerError> {
        self.cap.call_promise_cancellable(cancel, move |c| Ok(open(c)?.clear_field(field_json(&field))), |()| ()).await
    }

    /// What placing `mark` does follows `mode`: sign, fill, or ask.
    pub async fn place_mark(&self, mark: SignatureMark, target: SignaturePlaceTarget, cancel: Arc<CancelToken>) -> Result<SignaturePlaceResult, ViewerError> {
        self.cap
            .call_promise_cancellable(
                cancel,
                move |c| Ok(open(c)?.place_mark(mark.into(), target.into())),
                |r: crate::PlaceMarkResult| SignaturePlaceResult::try_from(r).map_err(ViewerError::from),
            )
            .await?
    }
```

`From<SignatureMark> for crate::Mark` wraps `Source` bytes as `Bytes(bytes).to_json()`. `SignatureSignInput::into_core(signer)` builds `crate::SignFieldInput` with `field_json`, the attribution as `{name, reason, location, contactInfo}` JSON, `certify` as `{"permission": n}`, and `lock` through `to_json`.

- [x] **Step 5: Run the tests**

Run: `cargo test -p epdf-plugin-signature --features uniffi --test ffi`
Expected: PASS, 12 tests.

- [x] **Step 6: Commit**

```bash
git add crates/plugins/signature crates/epdf-ffi
git commit -m "Export signing through uniffi with Swift signer and trust ports"
```

### Task 9: Host wiring

**Files:**
- Modify: `crates/epdf-viewer-host/Cargo.toml`, `src/api.rs`, `src/session.rs`, `src/lib.rs`, `tests/viewer_host.rs`

**Interfaces:**
- Consumes: Task 8's `ports::{signer_port, trust_port}`, `SignatureSigner`, `SignatureTrust`.
- Produces: `ViewerConfig.signature: Option<SignatureConfig>` (`{mode: Option<SignatureMode>, allow_certify: Option<bool>}`); `ViewerHost::signature(document_id) -> Arc<SignatureHandle>`; `ViewerHost::set_signature_signer(signer: Option<Arc<dyn SignatureSigner>>)`; `ViewerHost::set_signature_trust(trust: Option<Arc<dyn SignatureTrust>>)`.

A UniFFI record can't carry a foreign trait object, so the signer and trust can't sit in `ViewerConfig`. The plugin's config takes a signer thunk (`SignerSource::Thunk`, `contract.rs:39-43`) resolved per signing and a `TrustPort` called per validation, so the host passes closures that read two slots, and `set_signature_signer` / `set_signature_trust` fill them. Changing the signer then takes effect at the next signing, for every open document.

- [x] **Step 1: Write the failing host tests**

```rust
#[test]
fn a_document_signs_and_validates_over_pdfium() {
    let host = signature_host();
    host.set_signature_signer(Some(Arc::new(TestSigner::load(false))));
    let doc = block_on(host.open_document(read_repo_file("packages/engine/main/test/fixtures/unsigned_sigfield.pdf"), OpenDocumentOptions::default())).unwrap();
    let s = block_on(host.signature(doc.id)).unwrap();
    block_on(s.refresh(CancelToken::new())).unwrap();
    let field = block_on(s.unsigned_fields()).unwrap()[0].field.clone();
    let done = block_on(s.sign(SignatureSignInput { field: field.clone(), mark: None, attribution: None, certify: None, lock: None }, None, CancelToken::new())).unwrap();
    assert!(done.signature.signed);
    let verdict = block_on(s.validate_field(field, Default::default(), CancelToken::new())).unwrap();
    assert_eq!(verdict.integrity, SignatureIntegrity::Valid);
    assert_eq!(verdict.summary, SignatureSummary::ValidUntrusted);
}

#[test]
fn without_anchors_a_good_signature_is_valid_untrusted() {
    let host = signature_host();
    let doc = block_on(host.open_document(read_repo_file("packages/engine/main/test/fixtures/signed_two_approvals.pdf"), OpenDocumentOptions::default())).unwrap();
    let s = block_on(host.signature(doc.id)).unwrap();
    block_on(s.refresh(CancelToken::new())).unwrap();
    let verdicts = block_on(s.validate(Default::default(), CancelToken::new())).unwrap();
    assert!(verdicts.iter().all(|v| v.summary != SignatureSummary::Valid), "{verdicts:?}");
}

#[test]
fn a_changed_document_validates_invalid() {
    let host = signature_host();
    let doc = block_on(host.open_document(read_repo_file("packages/engine/main/test/fixtures/signed_two_approvals.pdf"), OpenDocumentOptions::default())).unwrap();
    let annotations = block_on(host.annotation(doc.id.clone())).unwrap();
    let page = block_on(host.pages(doc.id.clone())).unwrap()[0].page;
    create_square(&annotations, page);
    let s = block_on(host.signature(doc.id)).unwrap();
    block_on(s.refresh(CancelToken::new())).unwrap();
    let request = SignatureValidateRequest { at: None, until: Some(SignatureModificationsBasis::WorkingCopy) };
    let verdicts = block_on(s.validate(request, CancelToken::new())).unwrap();
    assert!(verdicts.iter().any(|v| v.modifications.verdict == SignatureModificationsVerdict::Forbidden));
    assert!(verdicts.iter().all(|v| v.summary != SignatureSummary::Valid));
}
```

`signature_host()` builds a host with `interaction: true`, `annotation`, `form`, `signature: Some(Default::default())` and starts it. `create_square` uses plan 05's `AnnotationHandle::create`:

```rust
use epdf_ffi::{PageRef, Rect};
use epdf_plugin_annotation::ffi::{AnnotationHandle, CreateAnnotationInput};

fn create_square(annotations: &AnnotationHandle, page: PageRef) {
    block_on(annotations.refresh(CancelToken::new())).unwrap();
    let input = CreateAnnotationInput {
        page,
        subtype: "square".into(),
        tool: None,
        props: None,
        flags: None,
        select: None,
        bounds: Some(Rect { x: 50.0, y: 50.0, width: 40.0, height: 30.0 }),
        rotation: None,
        from: None,
        to: None,
        vertices: None,
        strokes: None,
        quads: None,
        callout: None,
    };
    block_on(annotations.create(input, CancelToken::new())).unwrap();
}
```
 `TestSigner` is the one from Task 8; move it into `tests/common/mod.rs` so both crates' tests can't drift (the plugin test keeps its own copy, since a crate's tests can't share a module with another crate's).

- [x] **Step 2: Run them to verify they fail**

Run: `cargo test -p epdf-viewer-host --test viewer_host signature`
Expected: FAIL to compile.

- [x] **Step 3: Wire the host**

`Cargo.toml`:

```toml
epdf-plugin-signature = { path = "../plugins/signature", default-features = false, features = ["uniffi"] }
```

`api.rs`:

```rust
    /// Install `signaturePlugin(config)`: signature fields, validation and
    /// signing. Needs `form`. The signer and trust anchors are set with
    /// `ViewerHost::set_signature_signer` and `set_signature_trust`.
    #[uniffi(default = None)]
    pub signature: Option<SignatureConfig>,
```

```rust
#[derive(Debug, Default, uniffi::Record)]
pub struct SignatureConfig {
    /// `None` is `Sign` when a signer is set, else `Visual`.
    #[uniffi(default = None)]
    pub mode: Option<epdf_plugin_signature::ffi::SignatureMode>,
    /// Let the UI offer a certification. `None` is false.
    #[uniffi(default = None)]
    pub allow_certify: Option<bool>,
}
```

`lib.rs`, on `ViewerHost`, two slots shared with the session:

```rust
    signer: Arc<Mutex<Option<Arc<dyn SignatureSigner>>>>,
    trust: Arc<Mutex<Option<Arc<dyn SignatureTrust>>>>,
```

created in `new` and cloned into `Session::build`, and:

```rust
    /// The signer every document's `sign` uses when the call names none.
    /// Takes effect at the next signing.
    pub fn set_signature_signer(&self, signer: Option<Arc<dyn SignatureSigner>>) {
        *self.signer.lock().unwrap() = signer;
    }

    /// The anchors validation trusts. `None` makes every verdict top out at
    /// `validUntrusted`.
    pub fn set_signature_trust(&self, trust: Option<Arc<dyn SignatureTrust>>) {
        *self.trust.lock().unwrap() = trust;
    }

    pub async fn signature(&self, document_id: String) -> Result<Arc<SignatureHandle>, ViewerError> {
        let post = self.post.clone();
        self.call(move |s| {
            let cap = s.kernel.capability(&epdf_plugin_signature::SIGNATURE, Some(&document_id))?;
            Ok(SignatureHandle::new(cap, post))
        })
        .await
    }
```

`session.rs`:

```rust
        if let Some(signature) = config.signature {
            let signer_slot = signer.clone();
            let thunk: Rc<dyn Fn() -> Promise<SignerPort>> = Rc::new(move || {
                match signer_slot.lock().unwrap().clone() {
                    Some(signer) => match epdf_plugin_signature::ffi::ports::signer_port(signer) {
                        Ok(port) => Promise::resolved(port),
                        Err(error) => Promise::rejected(error),
                    },
                    None => Promise::rejected(
                        plugin_error(code::INVALID_INPUT, "signature", "no signer: call setSigner first", None, None),
                    ),
                }
            });
            let trust = trust.clone();
            plugins.push(epdf_plugin_signature::signature_plugin(epdf_plugin_signature::SignatureConfig {
                mode: signature.mode.map(Into::into),
                signer: Some(epdf_plugin_signature::SignerSource::Thunk(thunk)),
                trust: Some(Rc::new(SlotTrust(trust))),
                allow_certify: signature.allow_certify,
                host: Default::default(),
            }));
        }
```

with `SlotTrust` a `TrustPort` that reads the slot and answers no anchors when it's empty. Passing `signer: Some(Thunk)` always would make `get_mode()` answer `Sign` and `can_sign()` answer true with no signer set (`authority.rs:20-50`). So `can_sign` and `mode` on `SignatureHandle` check the slot too: give the handle an `Arc<Mutex<Option<..>>>` it reads (pass it to `SignatureHandle::new` from the host accessor), answering `mode = config.mode ?? (slot set ? Sign : Visual)` and `can_sign = cap.can_sign() && slot set`. Add a host test for that: `can_sign` is false until `set_signature_signer`, then true.

- [x] **Step 4: Run the host tests**

Run: `cargo test -p epdf-ffi -p epdf-viewer-host`
Expected: PASS.

- [x] **Step 5: Commit**

```bash
git add crates/epdf-viewer-host crates/plugins/signature
git commit -m "Add signature to ViewerHost with host-level signer and trust slots"
```

### Task 10: Swift: `SignatureCapability`, `KeychainSigner`, anchors

**Files:**
- Create: `platforms/apple/EmbedPDF/Sources/EmbedPDF/Plugins/Signature.swift`, `Plugins/KeychainSigner.swift`
- Create: `platforms/apple/EmbedPDF/Tests/EmbedPDFTests/SignatureTests.swift`, `SecurityTests.swift`, `KeychainSignerTests.swift`
- Modify: `platforms/apple/EmbedPDF/Sources/EmbedPDF/PDFViewer.swift`, `platforms/apple/EmbedPDF/Tests/EmbedPDFTests/ViewerTests.swift`, `platforms/apple/build-xcframework.sh`

**Interfaces:**
- Produces: `SignatureCapability`, `PDFViewer.signatures(for:)`, `PDFViewer.setSigner(_:)`, `PDFViewer.setTrust(_:)`, `KeychainSigner`, `TrustAnchors`.

On iOS there's no API that lists the system's trust anchors (`SecTrustCopyAnchorCertificates` is macOS-only), and `epdf-signature-core` validates paths itself against the anchors it's given (`trust.rs:26-28`). So the Swift trust port is `TrustAnchors`, a list of DER certificates the app bundles (its organisation's CA, say). Validating against the system store through `SecTrustEvaluateWithError` would need a different port shape in `epdf-signature-core` (evaluate a chain rather than list anchors); this plan doesn't change the core, and says so in the README row.

- [x] **Step 1: Add the fixtures**

In `platforms/apple/build-xcframework.sh`, add to the `FIXTURES` list (umbrella Task 0f), one line each, after the last plan's lines:

```bash
  packages/engine/main/test/fixtures/signed_two_approvals.pdf
  packages/engine/main/test/fixtures/signed_certified.pdf
  packages/engine/main/test/fixtures/unsigned_sigfield.pdf
  packages/engine/main/test/fixtures/signature_artwork.pdf
  packages/engine/main/test/fixtures/signature-compat/keys/TEST-ONLY-*.pem
```

The copy loop leaves entries unquoted, so the glob expands.

Add `static func text(_ name: String, ext: String) throws -> String` to `Fixture` in `BaseSuite.swift` for the PEMs.

- [x] **Step 2: Write the failing Swift tests**

```swift
// platforms/apple/EmbedPDF/Tests/EmbedPDFTests/KeychainSignerTests.swift
import CustomDump
@testable import EmbedPDF
import Security
import Testing

extension BaseSuite {
  @Suite struct KeychainSignerTests {
    static func testSigner() throws -> KeychainSigner {
      let key = try KeychainSigner.privateKey(pkcs8PEM: Fixture.text("TEST-ONLY-private-key", ext: "pem"))
      let cert = try KeychainSigner.certificate(pem: Fixture.text("TEST-ONLY-signer", ext: "pem"))
      return try KeychainSigner(privateKey: key, certificates: [cert])
    }

    @Test func anRSAKeySignsPKCS1() throws {
      let signer = try Self.testSigner()
      expectNoDifference(signer.algorithm(), .rsaPkcs1)
      expectNoDifference(signer.digest(), .sha256)
      let signature = try signer.sign(message: Data("hello".utf8))
      #expect(signature.count == 256)
      expectNoDifference(try signer.certificateChain().count, 1)
    }

    @Test func anECDSASignatureIsRawRAndS() throws {
      let attributes: [CFString: Any] = [kSecAttrKeyType: kSecAttrKeyTypeECSECPrimeRandom, kSecAttrKeySizeInBits: 256]
      var error: Unmanaged<CFError>?
      let key = try #require(SecKeyCreateRandomKey(attributes as CFDictionary, &error))
      let der = try #require(SecKeyCreateSignature(key, .ecdsaSignatureMessageX962SHA256, Data("x".utf8) as CFData, &error)) as Data
      let raw = try KeychainSigner.rawECDSA(der: der, fieldSize: 32)
      expectNoDifference(raw.count, 64)
    }
  }
}
```

```swift
// platforms/apple/EmbedPDF/Tests/EmbedPDFTests/SecurityTests.swift
import CustomDump
import EmbedPDF
import Testing

extension BaseSuite {
  @Suite struct SecurityTests {
    @Test func aLockedDocumentUnlocks() async throws {
      let viewer = try PDFViewer(ViewerConfig())
      try await viewer.start()
      let document = try await viewer.open(Fixture.data("encrypted-hello"))
      expectNoDifference(document.status, .locked)
      await #expect(throws: PDFError.self) { try await viewer.unlock(document, password: "nope") }
      try await viewer.unlock(document, password: "secret")
      let info = try #require(try await viewer.documents().first { $0.id == document.id })
      expectNoDifference(info.status, .ready)
    }

    @Test func aNarrowScopeCannotSign() async throws {
      let viewer = try PDFViewer(
        ViewerConfig(scope: ["doc.open", "doc.render"], interaction: true, form: FormConfig(), signature: SignatureConfig()))
      try await viewer.start()
      viewer.setSigner(try KeychainSignerTests.testSigner())
      let document = try await viewer.open(Fixture.data("unsigned_sigfield"))
      let signatures = try await viewer.signatures(for: document)
      expectNoDifference(try await signatures.canSign(), false)
      expectNoDifference(try await signatures.canFill(), false)
    }
  }
}
```

```swift
// platforms/apple/EmbedPDF/Tests/EmbedPDFTests/SignatureTests.swift
import CustomDump
import EmbedPDF
import Testing

extension BaseSuite {
  @Suite struct SignatureTests {
    func opened(_ fixture: String, signer: Bool = true) async throws
      -> (PDFViewer, PDFDocument, SignatureCapability)
    {
      let viewer = try PDFViewer(
        ViewerConfig(interaction: true, annotation: AnnotationConfig(), form: FormConfig(), signature: SignatureConfig()))
      try await viewer.start()
      if signer { viewer.setSigner(try KeychainSignerTests.testSigner()) }
      let document = try await viewer.open(Fixture.data(fixture))
      let signatures = try await viewer.signatures(for: document)
      _ = try await signatures.refresh()
      return (viewer, document, signatures)
    }

    @Test func twoApprovalsAreListed() async throws {
      let (_, _, signatures) = try await opened("signed_two_approvals")
      expectNoDifference(try await signatures.signatures().count, 2)
      expectNoDifference(try await signatures.protection()?.judged, .fill)
    }

    @Test func aFieldSignsAndValidatesUntrusted() async throws {
      let (_, _, signatures) = try await opened("unsigned_sigfield")
      let field = try #require(try await signatures.unsignedFields().first).field
      let done = try await signatures.sign(SignatureSignInput(field: field))
      expectNoDifference(done.signature.signed, true)
      let verdict = try await signatures.validate(field)
      expectNoDifference(verdict.integrity, .valid)
      expectNoDifference(verdict.summary, .validUntrusted)
    }

    @Test func withoutAnchorsASignatureIsValidUntrusted() async throws {
      let (_, _, signatures) = try await opened("signed_two_approvals")
      let verdicts = try await signatures.validate()
      #expect(verdicts.allSatisfy { $0.summary != .valid })
    }

    @Test func theTestCertificateAsAnAnchorIsTrusted() async throws {
      let (viewer, _, signatures) = try await opened("unsigned_sigfield")
      viewer.setTrust(try TrustAnchors(pem: [Fixture.text("TEST-ONLY-signer", ext: "pem")]))
      let field = try #require(try await signatures.unsignedFields().first).field
      _ = try await signatures.sign(SignatureSignInput(field: field))
      expectNoDifference(try await signatures.validate(field).trust, .trusted)
    }

    @Test func aCancelledSignerLeavesNothingPending() async throws {
      let (viewer, _, signatures) = try await opened("unsigned_sigfield", signer: false)
      viewer.setSigner(CancellingSigner())
      let field = try #require(try await signatures.unsignedFields().first).field
      await #expect(throws: PDFError.self) { _ = try await signatures.sign(SignatureSignInput(field: field)) }
      expectNoDifference(try await signatures.pending(), nil)
    }

    @Test func aVisualFillNeedsNoSigner() async throws {
      let (_, _, signatures) = try await opened("unsigned_sigfield", signer: false)
      expectNoDifference(try await signatures.mode(), .visual)
      let field = try #require(try await signatures.unsignedFields().first).field
      try await signatures.fill(field, with: .source(bytes: Fixture.data("signature_artwork")))
    }

    @Test(.disabled(if: !PDFViewer.hasStampPlugin, "needs plan 09 (stamp)"))
    func aStampMarkFillsAField() async throws {
      let viewer = try PDFViewer(
        ViewerConfig(interaction: true, annotation: AnnotationConfig(), form: FormConfig(), stamp: StampConfig(), signature: SignatureConfig()))
      try await viewer.start()
      let stamps = try await viewer.stamps()
      let library = try await stamps.importLibrary(Fixture.data("signature_artwork"), options: StampImportOptions(kind: .signature, libraryKind: "signatures"))
      let asset = try #require(try await stamps.assets(StampAssetFilter(libraryId: library)).first)
      let document = try await viewer.open(Fixture.data("unsigned_sigfield"))
      let signatures = try await viewer.signatures(for: document)
      _ = try await signatures.refresh()
      let field = try #require(try await signatures.unsignedFields().first).field
      try await signatures.fill(field, with: .asset(assetId: asset.id))
    }

    @Test func aFreeMarkIsAStamp() async throws {
      let (viewer, document, signatures) = try await opened("unsigned_sigfield", signer: false)
      let page = try await viewer.pages(of: document)[0].page
      let result = try await signatures.place(
        .source(bytes: Fixture.data("signature_artwork")), at: .free(page: page, at: Point(x: 50, y: 50), targetWidth: nil, rotation: nil))
      guard case .placed = result else { Issue.record("expected a placed annotation, got \(result)"); return }
    }

    @Test func aClosedDocumentsCapabilityThrows() async throws {
      let (viewer, document, signatures) = try await opened("signed_two_approvals")
      try await viewer.close(document)
      await #expect {
        _ = try await signatures.snapshot()
      } throws: { error in
        let error = error as? PDFError
        return error?.code == "instance-closed" && error?.name == "PluginError"
      }
    }

    @Test func droppingTheStreamUnsubscribes() async throws {
      let (_, _, signatures) = try await opened("signed_two_approvals")
      let task = Task { for await _ in signatures.events {} }
      try await Task.sleep(for: .milliseconds(20))
      task.cancel()
      _ = await task.value
      _ = try await signatures.refresh()
    }

    @Test func validateCancelsWhenTheTaskIsCancelled() async throws {
      let (_, _, signatures) = try await opened("signed_two_approvals")
      let task = Task { try await signatures.validate() }
      task.cancel()
      do { _ = try await task.value } catch { expectNoDifference((error as? PDFError)?.code, "operation-cancelled") }
      _ = try await signatures.status()
    }
  }
}

struct CancellingSigner: SignatureSigner {
  func certificateChain() throws(ViewerError) -> [Data] { [] }
  func algorithm() -> SignatureKeyAlgorithm { .rsaPkcs1 }
  func digest() -> SignatureDigest { .sha256 }
  func sign(message: Data) throws(ViewerError) -> Data {
    throw .Kernel(name: "UserCancelled", code: "operation-cancelled", detail: "the user cancelled")
  }
}
```

Every other `CancelToken` member (`refresh`, `validateField`, `analyzeChanges`, `readRevision`, `sign`, `prepareSignature`, `completeSignature`, `abortPending`, `fill`, `clearField`, `place`) gets its own `<member>CancelsWhenTheTaskIsCancelled` in the same shape; write each as you write the member in Step 4. `PDFViewer.hasStampPlugin` is `true` when `Stamp.swift` is compiled in; define it in `Stamp.swift` if plan 09 has merged and as `false` here otherwise. In `ViewerTests.swift`, the test that expected a locked open to throw now expects `.locked`, matching Task 2's Rust change.

- [x] **Step 3: Run them to verify they fail**

Run: `bash platforms/apple/build-xcframework.sh && (cd platforms/apple/EmbedPDF && xcodebuild test -scheme EmbedPDF-Package -destination 'platform=iOS Simulator,name=iPhone 17' -only-testing:EmbedPDFTests/BaseSuite/SignatureTests -only-testing:EmbedPDFTests/BaseSuite/SecurityTests -only-testing:EmbedPDFTests/BaseSuite/KeychainSignerTests)`
Expected: FAIL to compile.

- [x] **Step 4: Write `KeychainSigner.swift`**

```swift
public import EmbedPDFBindings
import Foundation
public import Security

/// A `SignatureSigner` over a `SecKey` that never leaves the Keychain (or
/// the Secure Enclave), and its certificate chain. Build one from a
/// `SecIdentity` the app imported, or from a key and certificates.
public final class KeychainSigner: SignatureSigner, @unchecked Sendable {
  let key: SecKey
  let chain: [Data]
  let keyAlgorithm: SignatureKeyAlgorithm
  let fieldSize: Int

  public convenience init(identity: SecIdentity) throws {
    var key: SecKey?
    var certificate: SecCertificate?
    guard SecIdentityCopyPrivateKey(identity, &key) == errSecSuccess, let key,
      SecIdentityCopyCertificate(identity, &certificate) == errSecSuccess, let certificate
    else { throw KeychainSignerError.unreadableIdentity }
    try self.init(privateKey: key, certificates: Self.chain(from: certificate))
  }

  /// `certificates` leaf first.
  public init(privateKey: SecKey, certificates: [SecCertificate]) throws {
    guard let attributes = SecKeyCopyAttributes(privateKey) as? [CFString: Any],
      let type = attributes[kSecAttrKeyType] as? String,
      let bits = attributes[kSecAttrKeySizeInBits] as? Int
    else { throw KeychainSignerError.unreadableKey }
    key = privateKey
    chain = certificates.map { SecCertificateCopyData($0) as Data }
    keyAlgorithm = type == (kSecAttrKeyTypeECSECPrimeRandom as String) ? .ecdsa : .rsaPkcs1
    fieldSize = (bits + 7) / 8
  }

  public func certificateChain() throws(ViewerError) -> [Data] { chain }
  public func algorithm() -> SignatureKeyAlgorithm { keyAlgorithm }
  public func digest() -> SignatureDigest { .sha256 }

  /// Called on the kernel thread. A Keychain item with an access control
  /// shows its prompt from here; a cancelled prompt throws
  /// `operation-cancelled`.
  public func sign(message: Data) throws(ViewerError) -> Data {
    let algorithm: SecKeyAlgorithm =
      keyAlgorithm == .ecdsa ? .ecdsaSignatureMessageX962SHA256 : .rsaSignatureMessagePKCS1v15SHA256
    var error: Unmanaged<CFError>?
    guard let signature = SecKeyCreateSignature(key, algorithm, message as CFData, &error) as Data? else {
      let cfError = error?.takeRetainedValue()
      let cancelled = (cfError.map { CFErrorGetCode($0) } ?? 0) == Int(errSecUserCanceled)
      throw .Kernel(
        name: cancelled ? "UserCancelled" : "SigningError",
        code: cancelled ? "operation-cancelled" : "signing-failed",
        detail: cfError.map { "\($0)" } ?? "SecKeyCreateSignature failed")
    }
    guard keyAlgorithm == .ecdsa else { return signature }
    do { return try Self.rawECDSA(der: signature, fieldSize: fieldSize) } catch {
      throw .Kernel(name: "SigningError", code: "signing-failed", detail: "\(error)")
    }
  }

  /// X9.62 `SEQUENCE { INTEGER r, INTEGER s }` to `r || s`, each left-padded
  /// to `fieldSize` bytes, which is what `epdf-signature-core` expects.
  static func rawECDSA(der: Data, fieldSize: Int) throws -> Data {
    var bytes = [UInt8](der)[...]
    func take(_ tag: UInt8) throws -> ArraySlice<UInt8> {
      guard bytes.popFirst() == tag, let first = bytes.popFirst() else { throw KeychainSignerError.badSignature }
      var length = Int(first)
      if first & 0x80 != 0 {
        length = 0
        for _ in 0..<Int(first & 0x7f) {
          guard let b = bytes.popFirst() else { throw KeychainSignerError.badSignature }
          length = length << 8 | Int(b)
        }
      }
      guard bytes.count >= length else { throw KeychainSignerError.badSignature }
      defer { bytes = bytes.dropFirst(length) }
      return bytes.prefix(length)
    }
    var sequence = try take(0x30)
    swap(&bytes, &sequence)
    let r = try take(0x02).drop { $0 == 0 }
    let s = try take(0x02).drop { $0 == 0 }
    guard r.count <= fieldSize, s.count <= fieldSize else { throw KeychainSignerError.badSignature }
    return Data(repeating: 0, count: fieldSize - r.count) + r + Data(repeating: 0, count: fieldSize - s.count) + s
  }

  static func chain(from leaf: SecCertificate) -> [SecCertificate] {
    var trust: SecTrust?
    guard SecTrustCreateWithCertificates(leaf, SecPolicyCreateBasicX509(), &trust) == errSecSuccess, let trust,
      let chain = SecTrustCopyCertificateChain(trust) as? [SecCertificate]
    else { return [leaf] }
    return chain
  }

  public static func privateKey(pkcs8PEM: String) throws -> SecKey {
    let der = try pemBody(pkcs8PEM)
    // SecKeyCreateWithData takes PKCS#1 for RSA: strip the PKCS#8 wrapper.
    let pkcs1 = try unwrapPKCS8(der)
    let attributes: [CFString: Any] = [kSecAttrKeyType: kSecAttrKeyTypeRSA, kSecAttrKeyClass: kSecAttrKeyClassPrivate]
    var error: Unmanaged<CFError>?
    guard let key = SecKeyCreateWithData(pkcs1 as CFData, attributes as CFDictionary, &error) else {
      throw KeychainSignerError.unreadableKey
    }
    return key
  }

  public static func certificate(pem: String) throws -> SecCertificate {
    guard let certificate = SecCertificateCreateWithData(nil, try pemBody(pem) as CFData) else {
      throw KeychainSignerError.unreadableCertificate
    }
    return certificate
  }

  static func pemBody(_ pem: String) throws -> Data {
    let body = pem.split(separator: "\n").filter { !$0.hasPrefix("-----") }.joined()
    guard let data = Data(base64Encoded: body) else { throw KeychainSignerError.unreadableKey }
    return data
  }

  /// `PrivateKeyInfo { version, algorithm, OCTET STRING privateKey }`.
  static func unwrapPKCS8(_ der: Data) throws -> Data {
    // Walk the outer SEQUENCE, skip the version INTEGER and the algorithm
    // SEQUENCE, and return the OCTET STRING's contents. Same TLV reader as
    // `rawECDSA`; factor it out into a `DERReader` struct when writing this.
    try DERReader(der).privateKeyInfoOctets()
  }
}

public enum KeychainSignerError: Error, Sendable {
  case unreadableIdentity, unreadableKey, unreadableCertificate, badSignature
}

/// Trust anchors the app supplies, DER. iOS can't list the system's roots,
/// so anchors are the app's own (its organisation's CA).
public struct TrustAnchors: SignatureTrust, Sendable {
  public let certificates: [Data]

  public init(der: [Data]) { certificates = der }

  public init(pem: [String]) throws { certificates = try pem.map(KeychainSigner.pemBody) }

  public func anchors() throws(ViewerError) -> [Data] { certificates }
}
```

`DERReader` is a small `struct` over `[UInt8]` with `take(tag:)` as in `rawECDSA` and `privateKeyInfoOctets()`; write it in the same file and have `rawECDSA` use it too. `SecTrustCopyCertificateChain` is iOS 15+, under the package's iOS 17 floor.

- [x] **Step 5: Write `Signature.swift` and the viewer methods**

Typealiases for every record and enum Task 7 and 8 produced (same pattern as `Metadata.swift`), then:

```swift
/// A document's signature fields: reading, validating, signing and filling
/// them. Get one with `PDFViewer.signatures(for:)`; the viewer needs
/// `ViewerConfig(form: FormConfig(), signature: SignatureConfig())`. Signing
/// uses the signer from `PDFViewer.setSigner(_:)` unless a call names one.
/// It keeps the viewer alive. After its document closes, every call throws
/// `instance-closed`.
public struct SignatureCapability: Sendable {
  let handle: SignatureHandle
  let viewer: PDFViewer

  public func snapshot() async throws(PDFError) -> SignatureSnapshot? {
    try await bridged { try await handle.snapshot() }
  }

  public func status() async throws(PDFError) -> SignatureStatus {
    try await bridged { try await handle.status() }
  }

  public func signatures() async throws(PDFError) -> [SignatureInfo] {
    try await bridged { try await handle.signatures() }
  }

  public func unsignedFields() async throws(PDFError) -> [SignatureInfo] {
    try await bridged { try await handle.unsignedFields() }
  }

  public func protection() async throws(PDFError) -> DocumentProtection? {
    try await bridged { try await handle.protection() }
  }

  public func pending() async throws(PDFError) -> SignaturePending? {
    try await bridged { try await handle.pending() }
  }

  public func mode() async throws(PDFError) -> SignatureMode {
    try await bridged { try await handle.mode() }
  }

  public func canSign() async throws(PDFError) -> Bool {
    try await bridged { try await handle.canSign() }
  }

  public func canFill() async throws(PDFError) -> Bool {
    try await bridged { try await handle.canFill() }
  }

  public func refresh() async throws(PDFError) -> SignatureSnapshot? {
    try await bridgedCancellable { token in try await handle.refresh(cancel: token) }
  }

  public func validate(_ request: SignatureValidateRequest = SignatureValidateRequest()) async throws(PDFError)
    -> [SignatureVerdict]
  {
    try await bridgedCancellable { token in try await handle.validate(request: request, cancel: token) }
  }

  public func validate(_ field: FormFieldRef, _ request: SignatureValidateRequest = SignatureValidateRequest())
    async throws(PDFError) -> SignatureVerdict
  {
    try await bridgedCancellable { token in
      try await handle.validateField(field: field, request: request, cancel: token)
    }
  }

  public func sign(_ input: SignatureSignInput, with signer: (any SignatureSigner)? = nil) async throws(PDFError)
    -> SignatureCompletion
  {
    try await bridgedCancellable { token in try await handle.sign(input: input, signer: signer, cancel: token) }
  }

  public func fill(_ field: FormFieldRef, with mark: SignatureMark) async throws(PDFError) {
    try await bridgedCancellable { token in try await handle.fillField(field: field, mark: mark, cancel: token) }
  }

  public func place(_ mark: SignatureMark, at target: SignaturePlaceTarget) async throws(PDFError)
    -> SignaturePlaceResult
  {
    try await bridgedCancellable { token in try await handle.placeMark(mark: mark, target: target, cancel: token) }
  }

  public var events: AsyncStream<SignatureEvent> {
    viewer.registry.stream { handle.subscribe(listener: $0) }
  }
}
```

plus one `bridged` or `bridgedCancellable` line for each remaining member (`signature(_:)`, `verdicts()`, `verdict(_:)`, `isBusy()`, `target()`, `analyzeChanges(since:until:)`, `readRevision(_:)`, `setTarget(_:)`, `requestInspection(_:)`, `canCertify()`, `prepareSignature(_:)`, `completeSignature(_:cms:)`, `abortPending()`, `clearField(_:)`), the `EventRelay: SignatureEventListener` extension with `onEvent` and `onEnd`, and:

```swift
extension PDFViewer {
  public func signatures(for document: PDFDocument) async throws(PDFError) -> SignatureCapability {
    let handle = try await bridged { try await host.signature(documentId: document.id) }
    return SignatureCapability(handle: handle, viewer: self)
  }

  /// The signer `sign` uses when a call names none. `nil` clears it.
  public func setSigner(_ signer: (any SignatureSigner)?) {
    host.setSignatureSigner(signer: signer)
  }

  /// The anchors validation trusts. `nil` makes every verdict top out at
  /// `.validUntrusted`.
  public func setTrust(_ trust: (any SignatureTrust)?) {
    host.setSignatureTrust(trust: trust)
  }
}
```

- [x] **Step 6: Run the Swift tests**

Run: `bash platforms/apple/build-xcframework.sh && (cd platforms/apple/EmbedPDF && xcodebuild test -scheme EmbedPDF-Package -destination 'platform=iOS Simulator,name=iPhone 17')`
Expected: PASS.

- [x] **Step 7: Commit**

```bash
git add platforms/apple
git commit -m "Add SignatureCapability, a Keychain signer and app trust anchors to the Swift package"
```

### Task 11: CI, docs, merge and clean up

**Files:**
- Modify: `.github/workflows/plugin-rust.yml`, `platforms/apple/README.md`, `docs/superpowers/plans/2026-10-01-swift-package.md`, `multiplatform-port-status.md`

- [ ] **Step 1:** Append `signature` to `crates=( ... )` in `plugin-rust.yml`.

- [x] **Step 2:** Add the README rows, and replace the README's note that document permissions aren't enforced with one line pointing at `ViewerConfig.scope`:

```markdown
| signature | `SignatureCapability`, `KeychainSigner`, `TrustAnchors` | `signature: SignatureConfig()` (with `form`) | `2026-10-06-swift-plugins/16-signature.md` |
```

and, under the table: "Trust anchors are the app's own: iOS can't list the system roots, and validation against the system store would need a `SecTrust`-evaluating port in `epdf-signature-core`."

- [x] **Step 3:** In `2026-10-01-swift-package.md`, mark §A.5 delivered by `16-signature.md` and Appendix B row B4 done. In `multiplatform-port-status.md`, move the signature engine paths from "Not started" to done, pointing at `crates/epdf-engine/tests/signature_*.rs`.

- [x] **Step 4: Run the definition of done**

```bash
cargo test -p epdf-plugin-signature --features uniffi
cargo test -p epdf-ffi -p epdf-viewer-host
cargo clippy -p epdf-plugin-signature -p epdf-viewer-host --all-targets --features epdf-plugin-signature/uniffi -- -D warnings
cargo test -p epdf-plugin-signature
cargo test -p pdfium -p epdf-engine -p epdf-signature-core
pnpm --filter @embedpdf/tooling-engine-conformance run signature-protection:check
bash platforms/apple/build-xcframework.sh
(cd platforms/apple/EmbedPDF && xcodebuild test -scheme EmbedPDF-Package -destination 'platform=iOS Simulator,name=iPhone 17')
cargo build -p epdf-viewer-host -p uniffi-bindgen
./target/debug/uniffi-bindgen generate --library target/debug/libepdf_viewer_host.dylib --language kotlin --no-format --out-dir "$TMPDIR/epdf-kotlin-check"
```

Expected: every command exits 0. The `pnpm` script name is whatever Task 3 named the protection vector's check.

- [x] **Step 5: Commit, with the size line**

```bash
ls -l target/aarch64-apple-ios-sim/release/libepdf_viewer_host.a
git add .github/workflows/plugin-rust.yml platforms/apple/README.md docs/superpowers/plans/2026-10-01-swift-package.md multiplatform-port-status.md
git commit -m "Test the signature exports in CI and document native signatures

libepdf_viewer_host.a (ios-sim, release): <before> -> <after> bytes"
```

- [ ] **Step 6: Merge, rerun, clean up**

From the integration worktree:

```bash
git merge --no-ff research/swift-signature
bash platforms/apple/build-xcframework.sh
(cd platforms/apple/EmbedPDF && xcodebuild test -scheme EmbedPDF-Package -destination 'platform=iOS Simulator,name=iPhone 17')
cargo build -p epdf-viewer-host -p uniffi-bindgen
./target/debug/uniffi-bindgen generate --library target/debug/libepdf_viewer_host.dylib --language kotlin --no-format --out-dir "$TMPDIR/epdf-kotlin-check"
git worktree remove .claude/worktrees/swift-signature
git branch -d research/swift-signature
git worktree prune
```

Expected: the rerun passes and both removals succeed without `--force`. Tick 16 in `00-umbrella.md`.
