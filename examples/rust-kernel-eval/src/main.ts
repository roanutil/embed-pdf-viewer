/**
 * The viewer/main dev harness (`packages/viewer/main/src/dev.ts`) on the Rust
 * kernel: the same element, chrome and engine worker, with `@embedpdf/core`
 * and `@embedpdf/plugin-*` redirected to `@embedpdf/core-wasm` by this app's
 * Vite config.
 *
 * Dropped from the dev harness: the signatures rung. The Rust signature
 * plugin runs with no signer (no `WebCrypto` or `PersonalKeyStore` host), so
 * a mark is drawn without sealing.
 */
import { initWasm } from '@embedpdf/core-wasm/browser';
import EmbedPDF from '../../../packages/viewer/main/src/doors/local';
import { installHosts } from './hosts';

// The kernel is built when the element mounts: the wasm module and the
// hosts must be in place first.
await initWasm();
installHosts();

const el = EmbedPDF.init({
  target: '#viewer',
  src: '/ebook.pdf',
  annotations: {
    fonts: [
      { key: 'roboto', url: '/fonts/Roboto-Regular.ttf', label: 'Roboto', familyName: 'Roboto' },
    ],
  },
});

// The DRIVE door from page script, as in viewer/main's index.html.
const label = document.querySelector<HTMLSpanElement>('#doc-label')!;
el.addEventListener('epdf:ready', () => {
  const viewer = el.viewer!;
  document.querySelector('#zoom-in')!.addEventListener('click', () => viewer.execute('zoom:in'));
  document.querySelector('#zoom-out')!.addEventListener('click', () => viewer.execute('zoom:out'));
  const show = () => {
    const docs = viewer.documents.list();
    label.textContent = docs.length
      ? docs.map((d) => `${d.id} (${d.status})`).join(', ')
      : 'no document';
  };
  show();
  viewer.on('opened', show);
  viewer.on('closed', show);
});
