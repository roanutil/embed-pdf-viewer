/**
 * The bundler-facing location contract (`@embedpdf/default-stamps/urls`) —
 * the same shape as `@embedpdf/engine-runtime-wasm32/wasm-url`.
 *
 * Bundlers that support the static `new URL(..., import.meta.url)` asset form
 * (webpack 5 / Next, Vite build, Turbopack, Rspack, Parcel 2) resolve each
 * entry at BUILD time: they copy the PDF into the application's build output
 * and rewrite the expression to the emitted asset's URL. The library then
 * ships inside the consumer's own build, versioned with node_modules, and is
 * fetched only when a stamps picker asks for it.
 *
 * Toolchains that leave the expression untouched resolve it at RUNTIME
 * against wherever this file actually lives — right for native ESM served
 * from real files, wrong for flattened bundles, where the fetch fails and a
 * consumer may fall back to {@link CDN_URL_TEMPLATE}.
 */
export const LOCALES = ['en', 'de', 'nl', 'fr', 'es', 'zh-CN', 'sv', 'ja'];

export const urls = {
  en: new URL('./en/stamps.pdf', import.meta.url).href,
  de: new URL('./de/stamps.pdf', import.meta.url).href,
  nl: new URL('./nl/stamps.pdf', import.meta.url).href,
  fr: new URL('./fr/stamps.pdf', import.meta.url).href,
  es: new URL('./es/stamps.pdf', import.meta.url).href,
  'zh-CN': new URL('./zh-CN/stamps.pdf', import.meta.url).href,
  sv: new URL('./sv/stamps.pdf', import.meta.url).href,
  ja: new URL('./ja/stamps.pdf', import.meta.url).href,
};

/** Safety net for a failed bundler-resolved fetch: the published package on
 *  jsDelivr, pinned to this major (the artwork has no ABI to match). */
export const CDN_URL_TEMPLATE =
  'https://cdn.jsdelivr.net/npm/@embedpdf/default-stamps@1/{locale}/stamps.pdf';
