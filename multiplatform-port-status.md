# Rust port status

What is ported, what pins it, and what is left. `multiplatform-port-order.md`
is the plan; this is where the plan actually got to. Regenerate nothing from
this file: every claim below points at a test you can run.

Run everything with `cargo test --workspace`, which is 52 suites at the time of
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
| G6 | `actions.read`, page and annotation action trees | `vectors/metadata/actions.json`, `tests/action_reader.rs` |
| G7 | annotation vocabulary: subtypes, flags, enums, `/IT`, `/State` | `vectors/annotation/vocabulary.json`, `tests/annotation_vocabulary.rs` |
| G7 | `annotations.listRawAll`, `annotations.listRawPage` | `vectors/annotation/read.json`, `tests/annotation_reader.rs` |
| G7 | `annotations.renderAppearances`, placement | `vectors/annotation/appearances.json`, `tests/annotation_appearance.rs` |
| G8 | `attachments.list`, `attachments.readFile` | `vectors/files/attachments.json`, `tests/attachment_reader.rs` |
| G8 | `forms.list`, and G7's widget-to-field join | `vectors/forms/snapshot.json`, `tests/form_reader.rs` |
| G8 | `pieceInfo.read`, `pieceInfo.applications` | `vectors/files/piece-info.json`, `tests/piece_info_reader.rs` |
| G9 | `pages.extract`, read back semantically | `vectors/pages/extract.json`, `tests/page_extract_corpus.rs` |

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
| G7 | appearance pixels, the clamp, the y-up flip | `tests/annotation_appearance.rs` |
| G8 | `forms.export`, FDF and XFDF envelopes | `tests/form_reader.rs` |
| G9 | `pages.extract` refusals and lifetime | `tests/page_extract.rs` |
| cross-cutting | rotation normalization | `tests/rotation_normalization.rs` |
| cross-cutting | cancel traces | `tests/cancel_traces.rs` |

## Not started

Phase 2 is done: every read group from G2 through G9 is ported and gated.

G10 onward: Stage B session authority and save, then every mutator. Phase 4.
That is `document.save*` plus the eleven write groups, and none of it is
blocked by anything outside the repository.

## Known gaps inside what is done

`open_layer_delta` and `open_layer_artifact` are tested only on their refusal
paths. Nothing produces a delta yet: `EPDFLayer_SaveDelta` is G10.

`SecurityReader::read_live` reproduces a defect rather than fixing it. It
reads `FPDF_GetDocUserPermissions` while describing it as the effective word,
so an owner-unlocked session reports itself restricted. That matches the
TypeScript, which is why it stays; `reader::LIVE_PERMISSIONS_DEFECT` and
`tests/security_reader.rs` both say so.

`openDestination` is absent from `actions.read`. It needs
`EPDFDoc_GetOpenActionDest`, which the pinned fork artifact `e4174c4f9` does
not export. The comparison drops the field rather than faking it, and
`tests/action_reader.rs::the_open_destination_form_is_not_read_by_this_build`
fails if a corpus document ever gains a destination-form `/OpenAction`.

The widget appearance-generation fallback in
`features/annotation/appearance.rs` never runs. The reader samples the
available-modes word once, before any generation could happen, and then skips
every mode whose bit is clear in that sample. Not a PDFium limitation:
`tests/annotation_appearance.rs::generating_a_form_fields_appearance_never_reaches_the_reader`
calls `EPDFAnnot_GenerateFormFieldAP` directly and it works. Reproduced from
the TypeScript, which has the same structure.

`forms.export` and `pages.extract` are compared semantically rather than
byte for byte, because the port order says the conformance suites do not
establish byte-stable PDF serialization. An export or an extract is reopened
and asked what it is; its object numbering is not compared to anything.

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

`page_actions.pdf` and `document_actions.pdf` carry the page and catalog
action slots. Nothing in the repository had a `/AA` on either.

`annotation_base.pdf` varies the annotation shell and holds the subtype
roughly constant: an indirect annotation, a direct one with `/NM`, a direct
one without, a direct one whose `/NM` is empty, all three `/RT` cases,
`/EMBD_Metadata`, a `/Rect` in reversed corner order, and a `/Sound` no reader
claims.

`annotation_kinds.pdf` does the opposite: one annotation per subtype, paired so
each resolved default sits beside an explicit entry. A decorated square against
a bare circle, a strikeout with `/IT` against one without, a free text with a
full `/DA` against one with none.

`annotation_appearances.pdf` is the only fixture with baked `/AP` streams, and
most of it is near misses for the rotation-stripping rule: a callout and a
polygon that carry rotation metadata and must still be placed by `/Rect`, and a
square carrying the logical box with no angle. It also carries the only
`/AcroForm` widget outside `form_fields.pdf`, plus an indirect widget attached
to nothing, which is what makes the widget-to-field join a test.

`attachments.pdf` is the first `/EmbeddedFiles` tree: a key that is not the
display name, an entry with nothing optional, a zero-byte file, two entries
sharing one stream, a filespec with no `/EF`, and a tree entry whose value is
the number 42.

`form_fields.pdf` is the first form: one field of every family, a radio group
that is one field with two widgets, a `/V` that is an array, a `/V` that is a
number, a field the `/AcroForm` tree forgot, and a `/CO` slot naming nothing.

`piece_info.pdf` puts different data on the catalog and on a page under one
application name, and gives the page more applications than the catalog so a
listing that used the wrong count loses one rather than running past the end.

## How the tests were checked

Every conformance runner in this port was mutation-tested: the implementation
was deliberately broken in several ways and the suite had to fail each time.
Where a mutation survived, the corpus gained a case or a fixture rather than
the mutation being written off. The commit messages name which mutations were
tried and which ones needed new coverage.

Pass the patterns to the harness as data, not through the shell. The first
version interpolated them into a `python3 -c` string, where a multi-line
pattern could silently fail to match while the mutant was still reported as
surviving. That produced one false negative, on sorting a `pages.extract`
selection, and the four mutants recorded as equivalent were re-verified
afterwards. Four still survive and are documented where they live: the
redundant appearance guard, the unexercised attachment-tree skip, the negative
blend-mode cast, and the stamp label's second route.
