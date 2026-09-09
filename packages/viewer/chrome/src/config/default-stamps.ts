/**
 * The viewer's BUILT-IN stamp library: `@embedpdf/default-stamps`, the
 * standard rubber stamps as one Acrobat-compatible PDF per locale.
 *
 * Delivery follows the engine's wasm rule (`wasm-source.ts`): the PDF is a
 * runtime-fetched asset, resolved on the main thread in a fixed order —
 *   1. `stamps.defaultLibrary === false`  → nothing, no request;
 *   2. a URL template (self-host)          → exactly that, never a CDN;
 *   3. the default                         → the bundler-resolved copy from
 *      the package (`@embedpdf/default-stamps/urls`), jsDelivr only when
 *      that fetch fails.
 *
 * Loading is LAZY (first open of the stamps panel, never at boot) and
 * locale-aware: the chrome locale leads, the browser languages break ties
 * for the locales the chrome does not translate itself, and a locale change
 * while a default library is loaded swaps it for the new one. The file names
 * itself (`/Title`, `Id: embedpdf-standard`), so no overrides are passed.
 */
import { CDN_URL_TEMPLATE, LOCALES, urls } from '@embedpdf/default-stamps/urls';
import { negotiateLocale } from '@embedpdf/react/i18n';
import type { StampCapability } from '@embedpdf/react/stamp';

/** The built-in library's PieceInfo id — excluded from persistence (it is
 *  fetched again on every first open, in the locale of that moment). */
export const DEFAULT_LIBRARY_ID = 'embedpdf-standard';

export type DefaultLibrarySource = false | string | undefined;

/** Which shipped locale to show for the chrome's locale + the browser's. */
export function resolveStampsLocale(chromeLocale: string): string {
  const languages = typeof navigator !== 'undefined' ? (navigator.languages ?? []) : [];
  return negotiateLocale(LOCALES, [chromeLocale, ...languages]) ?? 'en';
}

const fetchBytes = async (url: string): Promise<Uint8Array> => {
  const response = await fetch(url);
  if (!response.ok) throw new Error(`${url}: HTTP ${response.status}`);
  return new Uint8Array(await response.arrayBuffer());
};

async function fetchDefaultLibrary(locale: string, source: string | undefined) {
  if (source) return fetchBytes(source.replace('{locale}', locale));
  try {
    return await fetchBytes(urls[locale]);
  } catch (error) {
    // The bundler-default only: a flattened bundle left `import.meta.url`
    // pointing nowhere. An explicit source never phones a CDN.
    console.warn('[embedpdf] default stamps: bundler URL failed, trying the CDN', error);
    return fetchBytes(CDN_URL_TEMPLATE.replace('{locale}', locale));
  }
}

/** Per workspace: the locale the default library was last brought to, and
 *  the in-flight work — the panel mounts and unmounts with the sidebar, so
 *  this cannot live in component state. */
const loads = new WeakMap<StampCapability, { locale: string; promise: Promise<void> }>();

/**
 * Bring the built-in library to `locale`. Idempotent per workspace and
 * locale; a locale change swaps the loaded default (remove, then import);
 * a default the user removed in this session stays removed — the locale is
 * recorded, nothing is re-added. An embedder that seeded a library under
 * the same id is left alone.
 */
export function ensureDefaultLibrary(
  stamp: StampCapability,
  locale: string,
  source: DefaultLibrarySource,
): Promise<void> {
  if (source === false) return Promise.resolve();
  const current = loads.get(stamp);
  if (current?.locale === locale) return current.promise;
  const run = async () => {
    await current?.promise.catch(() => undefined);
    const loaded = stamp.library(DEFAULT_LIBRARY_ID);
    if (loaded?.locale === locale) return;
    if (current && !loaded) return; // removed by the user this session
    if (loaded) await stamp.removeLibrary(DEFAULT_LIBRARY_ID);
    const bytes = await fetchDefaultLibrary(locale, source);
    await stamp.importLibraryPdf(bytes);
  };
  const promise = run();
  loads.set(stamp, { locale, promise });
  return promise;
}
