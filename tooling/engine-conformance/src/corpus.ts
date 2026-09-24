/**
 * The cross-language conformance corpus: which document, which pages, which
 * operations. One list, read by the vector generator here and by the Rust
 * runners in `crates/epdf-engine/tests/`.
 *
 * Fixtures are referenced in place. The repository already keeps most of these
 * PDFs for its JavaScript suites, its examples and the multiplatform
 * prototype, and a second copy would let one of them drift. The exception is
 * `fixtures/` beside this file, which holds documents generated for the corpus
 * itself; `fixtures/generate.mjs` writes them.
 *
 * ## Embedded fonts are a membership requirement
 *
 * A document that names a standard-14 font and embeds no font program makes
 * PDFium substitute, and what it substitutes depends on the host. The WASM
 * build has no system fonts and falls back to its built-in mapper; a native
 * build on macOS reaches CoreText. Their glyph boxes differ by tenths of a
 * point, so such a file cannot be a cross-language oracle for anything
 * metric. Measured on `hello_world.pdf`: `looseBox.bottom` 47.012 through WASM
 * against 46.328 through the macOS dylib.
 *
 * That is a real product question for the native SDKs, not only a testing
 * inconvenience, and it belongs to G4's font registry. Until G4 pins a
 * fallback environment across hosts, every metric case here embeds its font.
 */

/** Operations a corpus case can carry vectors for. */
export type CorpusOperation = 'pages.geometry' | 'pages.text';

export interface CorpusCase {
  /** Stable slug. Names the vector file: `vectors/<operation>/<name>.json`. */
  readonly name: string;
  /** Repository-relative path to the PDF. */
  readonly fixture: string;
  /** Why this document is in the corpus, in one line. */
  readonly covers: string;
  /**
   * Zero-based page indices to emit, or `'all'`. Kept explicit so a large
   * fixture contributes the pages that carry the behaviour rather than a
   * megabyte of vector for the ones that do not.
   */
  readonly pages: readonly number[] | 'all';
  /** The user password, for encrypted fixtures. */
  readonly password?: string;
  /** Operations to emit for this case. */
  readonly operations: readonly CorpusOperation[];
}

export const CORPUS: readonly CorpusCase[] = [
  {
    name: 'embedded-orientations',
    fixture: 'tooling/engine-conformance/fixtures/embedded_orientations.pdf',
    covers:
      'Every run variant the reader discriminates, with the font program embedded: upright, rotated, shear-only, mirrored, quarter-turn, and a zero-size object whose glyphs are degenerate.',
    pages: 'all',
    operations: ['pages.geometry', 'pages.text'],
  },
  {
    name: 'report',
    fixture: 'pdfium-multiplatform-prototype/fixtures/report.pdf',
    covers:
      'Dense real-world text at scale with subsetted embedded fonts: ~5,000 and ~8,100 glyphs on one page.',
    // Two of fourteen. The whole document is 90,000 glyphs, and a vector that
    // large gets regenerated blind rather than reviewed. Page 0 is the cover,
    // page 4 is the densest page and the one the prototype's own report vector
    // uses.
    pages: [0, 4],
    operations: ['pages.geometry', 'pages.text'],
  },
  {
    name: 'font-size-zero',
    fixture: 'packages/engine/main/test/fixtures/font_size_zero.pdf',
    covers:
      'The degenerate-glyph convention in isolation. Names Helvetica and embeds nothing, which is safe here and only here: every glyph comes back zeroed and flagged empty, so no substituted metric reaches the snapshot.',
    pages: 'all',
    operations: ['pages.geometry', 'pages.text'],
  },
  {
    name: 'non-identity-text',
    fixture: 'tooling/engine-conformance/fixtures/non_identity_text.pdf',
    covers:
      "A page whose character list and text are different sizes: a Type0 font whose /ToUnicode maps one CID to a supplementary code point and leaves .notdef unmapped, so one character contributes two text units and another contributes none. Every other fixture here has an identity map, which left the translation pinned only against hand-built snapshots.",
    pages: 'all',
    operations: ['pages.geometry', 'pages.text'],
  },
  {
    name: 'colors',
    fixture: 'pdfium-multiplatform-prototype/fixtures/colors.pdf',
    covers:
      'Saturated interior colors and no text at all: the empty-snapshot case, and the document the raster gate will share.',
    pages: 'all',
    operations: ['pages.geometry', 'pages.text'],
  },
];
