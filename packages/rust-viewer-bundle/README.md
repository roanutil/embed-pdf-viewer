# @mfb/embedpdf-rust-viewer

Evaluation only. The full EmbedPDF viewer running on the Rust kernel and plugins
(`crates/epdf-web`, through `@embedpdf/core-wasm`), as one self-contained folder: the viewer, the
kernel's wasm, PDFium's wasm, the engine's workers and Preact. No peer dependencies, no Rust
toolchain, no Vite plugin on the consumer side.

## Use

```sh
npm i ./vendor/mfb-embedpdf-rust-viewer-0.0.0-hackathon.1.tgz
```

```ts
import { createViewer, buildInfo } from '@mfb/embedpdf-rust-viewer';

const el = await createViewer({ target: '#viewer', src: '/report.pdf' });
el.addEventListener('epdf:ready', () => {
  const viewer = el.viewer!;
  console.log(viewer.documents.list());
});
console.log('Rust viewer from', buildInfo.commit);
```

Give `#viewer` a height. `createViewer` takes every `EmbedPDF.init` option (`theme`, `disable`,
`readOnly`, `documents`, `onReady`, ...).

Assets resolve relative to the bundle (`new URL(..., import.meta.url)`), so Vite, webpack 5 and
Rspack emit them into the app's build, and a plain `<script type="module">` works when `dist/` is
served as a folder. If a bundler loses them, copy `dist/` to a static path and pass
`engine: { assetsUrl: '/embedpdf/' }`.

Known gaps: signatures are drawn without sealing (no WebCrypto signer host is wired).

## Build (in the devcontainer)

```sh
pnpm --filter @mfb/embedpdf-rust-viewer build:all      # eval bootstrap + Rust wasm + this bundle
pnpm --filter @mfb/embedpdf-rust-viewer pack:tarball   # -> packages/rust-viewer-bundle/*.tgz
```

`build:all` needs the devcontainer's toolchain (wasm-bindgen-cli 0.2.127, binaryen 133) and
`.devcontainer/setup.sh` to have run. Bump `version` before each tarball you hand out, so installs
don't reuse a cached copy.
