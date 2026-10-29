/**
 * Hand-written: the viewer's own declarations reach into ~25 workspace
 * packages, so they can't ship in a self-contained tarball. The common fields
 * are typed; the rest of `EmbedPDF.init`'s options and the viewer handle pass
 * through loosely (see packages/viewer/chrome/src/viewer.tsx and handle.ts on
 * the source branch for the full shapes).
 */

/** A URL, a `File`/`Blob`, or the document's bytes. */
export type DocumentSrc = string | URL | Blob | ArrayBuffer | Uint8Array;

export interface DocumentEntry {
  src: DocumentSrc;
  name?: string;
  password?: string;
  [key: string]: unknown;
}

/** The built-in PDFium engine's options (`LocalEngineRecipeOptions`). */
export interface LocalEngineConfig {
  /** Exact URL of `embedpdf.wasm`. Defaults to the copy beside this bundle. */
  wasmUrl?: string;
  /** Folder holding `embedpdf.wasm` and the worker files. */
  assetsUrl?: string;
  /** URL of `embedpdf-worker.js`, for strict-CSP setups. */
  worker?: string;
  /** URL of `encoder-worker.js`. */
  encoderWorker?: string;
  [key: string]: unknown;
}

export interface CreateViewerOptions {
  /** The element, or a selector for it, the viewer is appended to. Give it a height. */
  target: HTMLElement | string;
  /** One document. Another value opens in its place; `null` closes it. */
  src?: DocumentSrc | null;
  /** Several documents, one tab each. With these, `src` is ignored. */
  documents?: readonly DocumentEntry[];
  /** Defaults to the built-in engine with Rust page-geometry reads on. */
  engine?: LocalEngineConfig | object | (() => unknown);
  locale?: 'auto' | (string & {});
  theme?: 'light' | 'dark' | 'system' | Record<string, unknown>;
  readOnly?: boolean | Record<string, unknown>;
  /** Features to turn off, e.g. `['forms', 'redaction']`. */
  disable?: readonly string[];
  annotations?: {
    fonts?: readonly { key: string; url: string; label: string; familyName: string }[];
    [key: string]: unknown;
  };
  onReady?: (viewer: Viewer) => void;
  onDocumentChange?: (documentId: string | null) => void;
  /** Every other `EmbedPDF.init` option, passed through as is. */
  [key: string]: unknown;
}

/** A plugin's API by name: `viewer.get('search')`, `viewer.get('zoom')`, ... */
export interface ScopedViewer {
  get(name: string): any;
  tryGet(name: string): any | null;
}

/** The viewer, from code: `element.viewer`, and what `onReady` gets. */
export interface Viewer extends ScopedViewer {
  readonly documents: any;
  readonly commands: any;
  forDocument(documentId: string): ScopedViewer;
  watch<T>(
    read: () => T,
    onChange: (value: T, previous: T) => void,
    isEqual?: (left: T, right: T) => boolean,
  ): () => void;
  [key: string]: any;
}

/** The `<embedpdf-viewer>` element. Fires `epdf:ready` once `viewer` is set. */
export interface EmbedPdfViewerElement extends HTMLElement {
  readonly viewer: Viewer | null;
}

/** Load the kernel's wasm and install its hosts. Idempotent; `createViewer` calls it. */
export function initKernel(): Promise<void>;

/** Create an `<embedpdf-viewer>` on the Rust kernel, configure it, and append it to `target`. */
export function createViewer(options: CreateViewerOptions): Promise<EmbedPdfViewerElement>;

/** Live sizes of the kernel's tables, for chasing leaks from the console. */
export function kernelCounts(): Record<string, number>;

/** The commit this bundle was built from. */
export const buildInfo: {
  readonly commit: string;
  readonly branch: string;
  readonly dirty: boolean;
  readonly builtAt: string;
};
