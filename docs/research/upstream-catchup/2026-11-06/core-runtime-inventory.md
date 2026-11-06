# Current core/runtime inventory — planning only

Authoritative target: `8297bad8d388eb9c70324c22cb001c59248f95f4`; runtime: `4346c5131cd009c4086fdce13bde7fd43fbd4ce0`. Branch baselines are in [pinned-targets.json](pinned-targets.json). This document and [the main plan](scope-and-plan.md) supersede the C7 core/runtime inventory; the byte-exact historical record remains [here](historical/c7/core-runtime-inventory.md).

## Retained C7 requirements

| Requirement | Exact current owners and planned proof |
|---|---|
| Sibling subtype conversion | TS `packages/engine/core/src/annotation/siblings.ts`, annotation resolvers and services `AnnotationMutator.ts`; Rust `crates/epdf-engine/src/features/annotation/write/{patch,prepare,update}.rs`, `crates/pdfium/src/annot_write.rs`. Preserve ref/NM/authority; invalid destination fields and non-siblings refuse without mutation. |
| No stroke / dash | TS base schema and dash resolver/readStyle; Rust annotation write/read style and PDFium writers. Explicit null writes `/C []`, omitted keeps default; empty dash `[]` refuses, null clears, nonnegative nonempty `[0]` accepts, omitted dashed default `[3,3]`. Read/save/reopen and raster tests must agree. |
| Rich comments and fonts | Existing annotation rich-text/read/write owners and raw document font facts. Markup RC source/default/plain projection differs from FreeText appearance; retain no-redraw semantics and target font display fallback chain. Runtime reader defaults fix is now inside the mandatory runtime target. |
| XYZ destination | TS `features/destinations/readDestination.ts` and target destination tests; Rust `features/actions/destination.rs`, `pdfium/src/action.rs`. Read raw shortened arrays/null axes; zoom zero becomes null. Preserve positive zoom; superseded intermediate behavior is not a second task. |
| Ink fitting | TS `packages/core/annotation/src/ink-curves.ts` and Bezier owners; Rust annotation-core scene/ink geometry; matching native appearance fitter. P1 owns the portable algorithm once; engine tests consume it. |

Historical C7 owner/commit detail is supporting evidence, not authority for renamed final payloads. Compare the final pinned source and [contract excerpts](contract-migration-map.txt).

## New work beyond C7

| Group and source commits | Target requirement and owning boundary | Planned acceptance |
|---|---|---|
| Signature box / mark `14fbaf093`, runtime `0b3eb0939` | TS widget declaration (`hasMark`), form prediction, annotation mutator/read/write and signature appearance fit. Rust annotation read/write, `features/forms/{snapshot,widgets,write}.rs`, `features/signature/{reader,mutator}.rs`, shared DTO and plugin form/signature painters. Native existing `EPDFAnnot_GenerateFormFieldAP` draws Sig/orphan boxes. | New unsigned signature/orphan widget draws box. Visual or signed mark survives move/resize/save/reopen. Style changes refuse before dictionary mutation, current appearance remains intact; prediction and engine response agree. Upstream `signature-visual-fill.test.ts`, `widget-appearance-refresh.test.ts`, `widget-painter-parity.test.ts` supply cases. |
| Clean text `ac625667e` | TS core `text/charmap.ts`, `text/lineBreaks.ts`, service `PageTextReader.ts`, annotation read/write and `richTextWire.ts`. Rust `features/text/{reader,charmap}.rs`, page-space text, annotation rich-text/read/write. | CRLF→LF, lone CR→LF, PDFium noncharacters produce no text while original `charCount`/geometry stay fixed and sparse anchors remain valid. Include joined hyphen, NUL marker, surrogate pair, search spans and selection. API plain/rich annotation lines use LF; PDF strings/engine rich-text JSON use CR. TS `core-text-parity.test.ts`, `search-joined-hyphen.test.ts`, core charmap/lineBreak tests; Rust text/search/annotation round trips. |
| Engine values / cache / search pins `f480cf6cf`, `5b7fed894`, `7b20b7ade`, `28bec8fa1`, `f981f743e` | TS `wire/{token,tokenSchemas,cachedReads,apiVersion}.ts`, document manifest/search; CloudPDF contract/engine/server cache read owners. Rust `features/search/{cursor,reader,mod}.rs`, `dto.rs`, host manifests/change application. | Stored `contentsVersion` and `layoutVersion` invalidate search when appropriate; annotation/metadata churn does not manufacture text changes. Query canonicalization preserves regex/case/diacritic options. HTTP owner validates pinned reads, engine/shape tokens and direct API/version headers; cached bodies stay determined by URLs, CDN requests avoid extra version-header preflight. No new Rust HTTP server is required. |
| Tolerant reads `420d5a72a`, `58ec72910` | TS `wire/reading.ts`, response schemas, `conformance/tolerantReading.ts`, contract/OpenAPI and SDK fixtures. Rust deserializers and actual JSON/WASM/native consumers. | Unknown nested response fields ignored; unknown open union/enum maps to declared catch-all; malformed known kind fails; closed sets and strict write requests reject invalid values. Use real representative response fixtures, not blanket serde defaults. SDK HTTP clients and native .NET facade are distinct surfaces; audit each actual consumer. |
| Error/null/date/name/tag migration `ff3030aaf`, `8bbed33af`, `72dd61b2e`, `e9ef4f2d5`, `26a686502`, `c4b286a69`, `a780243ba` | TS final DTO/schema/forms/auth/wire and shared Rust DTO/read/write/page-space adapters. Includes action, metadata, attachments, signature, fonts, render and grant records. | Assert exact serialized keys and required null outputs. ISO dates preserve parsed PDF offsets (including no offset); generated dates UTC Z; invalid PDF date→null; fractional seconds dropped on write. Field discriminants use final kind, operation outcomes final status; named permission/submit flags preserve PDF bit encoding. Worker fields now include contentsSizeBytes/displaySha256/mimeType; round-trip each caller. |

The new runtime target is 4 history commits / 2 functional commits beyond C7's viewer-selected `f1ba46f0…`, and 2 history commits / 1 functional commit beyond its recommended `eeac787c…`. Runtime `epdf_annot.h`/`epdf_form.h` comments now document the signature-box semantics; no new declaration is introduced by that final delta. Job 1 must verify the final built export association rather than reusing old counts. Preserve plugin runtime `82f2dfabe…` canonical macOS static targets as well as earlier fork target-args/emsdk/layer ownership work. Runtime work remains a merge into the fork, never a raw canonical pointer replacement.

## Contract migration procedure

1. Capture final TS schemas, worker request/result definitions and public API snapshots from the frozen commit. The machine [contract excerpts](contract-migration-map.txt) enumerate changed declarations; the full [functional diff](viewer-functional.diff) is definitive.
2. At each Rust owning record, add a failing serialization/deserialization/vector test for actual final keys, tags, nullability and error shape. Keep strict requests separate from tolerant response parsing.
3. Update the one shared owner and its existing adapters; trace actual consumers before inventing a duplicate mapper. Regenerate vectors from the merged TS oracle with matching runtime.
4. Replay narrow affected tests, then enumerate all merged `:check` scripts and workspace tests/clippy. Check literal router names through runtime invocation as well as compile-time types.
5. Record fresh pass/fail/skip counts, source and library identities. Existing accepted NaN/Rust-only differences remain explicit; altered target semantics require deliberate reconciliation.

## Proposed commands and gate

Run only during authorized implementation, with matching runtime/resource wrappers and project dependencies installed:

```sh
pnpm --filter @embedpdf/tooling-engine-conformance annotation-update:check
CARGO_BUILD_JOBS=2 cargo test -p epdf-engine --test annotation_update --test write_transactions --test text_corpus --test text_layout --test search_slice_protocol --test signature_sign --test wire_contract
CARGO_BUILD_JOBS=2 cargo test --workspace --no-fail-fast
```

These are entry points, not the complete suite or a claim that current baselines pass. Add coverage at existing text/search/signature/metadata/font/form owners and select the merged engine package's full check scripts. Host/toolchain availability and exact runtime wrappers must be checked before execution. Jobs 1–3 own this work and their estimates; do not add it again to plugin/native tasks.

Next action (under two minutes): compare the new-work table against the five jobs in [scope-and-plan.md](scope-and-plan.md).
