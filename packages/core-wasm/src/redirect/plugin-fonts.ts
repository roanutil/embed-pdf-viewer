// `@embedpdf/plugin-fonts` with the eval build's stand-in factory (no Rust
// fonts plugin yet; see `fontsPlugin` in `../plugins`).
export * from '@embedpdf/plugin-fonts';
export { fontsPlugin } from '../plugins';
