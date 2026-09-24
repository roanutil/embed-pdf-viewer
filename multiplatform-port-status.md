# Rust port status

What is ported, what pins it, and what is left. `multiplatform-port-order.md`
is the plan; this is where the plan actually got to. Regenerate nothing from
this file: every claim below points at a test you can run.

Run everything with `cargo test --workspace`, which is 44 suites at the time of
writing, plus `cargo clippy --workspace --all-targets -- -D warnings`.

## Done, and pinned to the TypeScript by vectors

A group is listed here when a Rust implementation exists AND a committed vector
file generated from the shipping TypeScript is replayed against it. Vectors
live under `tooling/engine-conformance/vectors/`, and every generator has a
`:check` script wired into `.github/workflows/engine-rust.yml`.

| Group | What | Pinned by |
|---|---|---|
| G1 | `MetadataPatch` three-state serialization | `vectors/schema/`, `tests/schema_parity.rs` |
| G2 | `pages.geometry` | `vectors/pages.geometry/`, `tests/geometry_corpus.rs` |
| G3 | `auth/scope` resolver | `vectors/scope/`, `tests/scope_conformance.rs` |
| G3 | security derivation | `vectors/security/`, `tests/security_state.rs` |
| G4 | render geometry | `vectors/render/`, `tests/render_geometry.rs` |
| G5 | search dialect, fold, literal, snippet | `vectors/search/`, `tests/search_dialect.rs` |
| G5 | character-to-text translation | `vectors/text/charmap.json`, `tests/charmap.rs` |
| G5 | `pages.text` | `vectors/pages.text/`, `tests/text_corpus.rs` |
| G5 | text layout, hit tests, segments | `vectors/text/layout.json`, `tests/text_layout.rs` |
| G6 | `metadata.read` | `vectors/metadata/read.json`, `tests/metadata_corpus.rs` |
| G6 | `pages.list` | `vectors/metadata/pages.json`, `tests/pages_corpus.rs` |
| G6 | SubmitForm flags, action vocabulary | `vectors/actions/`, `tests/submit_flags.rs` |

## Done, tested against fixtures rather than vectors

These have tests but no cross-language comparison, because the behaviour is
either PDFium's own or has no TypeScript counterpart to compare with. Treat
them as verified, not as pinned.

| Group | What | Tests |
|---|---|---|
| G3 | session lifecycle, locking, leases | `tests/document_session.rs` |
| G3 | control frames, base registry | `tests/session_registry.rs` |
| G3 | `SecurityReader` over encrypted fixtures | `tests/security_reader.rs` |
| G4 | render pixels, tiles, alpha, budget | `tests/render_pixels.rs` |
| G4 | font registry, fallback order | `tests/font_registry.rs` |
| G5 | `search.query` slices and cursors | `tests/search_reader.rs` |
| G9 | `pages.extract` | `tests/page_extract.rs` |
| cross-cutting | rotation normalization | `tests/rotation_normalization.rs` |
| cross-cutting | cancel traces | `tests/cancel_traces.rs` |

## Not started

`actions.read`'s PDFium walk (`ActionModelReader.ts`, 414 lines) and
`DocumentActionsReader.ts`. The vocabulary those consume is ported; the graph
walk is not. G7's link annotations wait on this.

G7 in full: annotation reads, roughly 4,900 lines across 60 files.

G8: attachments, forms reads, piece info.

G10 onward: Stage B session authority and every mutator. Phase 4.

## Known gaps inside what is done

`PageLayout.actions` is never populated, because page open and close actions
come from the unported action reader. `tests/pages_corpus.rs` asserts that no
corpus fixture carries one, so the day that changes it fails rather than
silently differing.

`open_layer_delta` and `open_layer_artifact` are tested only on their refusal
paths. Nothing produces a delta yet: `EPDFLayer_SaveDelta` is G10.

`SecurityReader::read_live` reproduces a defect rather than fixing it. It
reads `FPDF_GetDocUserPermissions` while describing it as the effective word,
so an owner-unlocked session reports itself restricted. That matches the
TypeScript, which is why it stays; `reader::LIVE_PERMISSIONS_DEFECT` and
`tests/security_reader.rs` both say so.

A word can extend past its own line in the text layout. The word walk follows
glyph flags and the line walk follows runs, so two text objects that abut with
no space between them give a word spanning two orientations. Reproduced from
the TypeScript and recorded in
`tests/text_layout.rs::a_word_can_cross_a_line_boundary_between_abutting_runs`.

## Fixtures we added

`tooling/engine-conformance/fixtures/generate.mjs` writes all of these, and CI
diffs its output, so they are reproducible rather than committed binaries
nobody can rebuild.

`encrypted-r2-rc4-40.pdf`, `encrypted-r3-rc4-128.pdf`,
`encrypted-r4-aes-128.pdf`, `encrypted-r6-aes-256.pdf`,
`encrypted-r4-all-allowed.pdf`, `encrypted-r4-empty-user-password.pdf` and
`unsupported-security.pdf` cover the security handler revisions and the
unsupported-handler path. `fixtures/encrypt.mjs` implements enough of the
Standard security handler to write them, RC4 included, because OpenSSL 3
dropped it.

`rotated_pages.pdf` is four pages at `/Rotate` 0, 90, 180 and 270. Nothing in
the repository set `/Rotate` before it, which is how `OwnedDocument::page`
shipped using the un-normalized loader.

`non_identity_text.pdf` is a page whose character list and text are different
sizes. Every other fixture has an identity char map.

`reverse_and_gapped_text.pdf` has a run reading right to left and a run with a
gap wide enough to split it. Mutation testing found both untested.

`rich_metadata.pdf` has an absent key, a present-but-empty key, a UTF-16 title
outside Latin-1, and dates in both the offset and Z forms.

## How the tests were checked

Every conformance runner in this port was mutation-tested: the implementation
was deliberately broken in several ways and the suite had to fail each time.
Where a mutation survived, the corpus gained a case or a fixture rather than
the mutation being written off. The commit messages name which mutations were
tried and which ones needed new coverage.
