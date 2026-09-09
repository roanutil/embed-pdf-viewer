/**
 * Acrobat's stamp-library convention, interpreted in ONE place. A library is
 * a PDF whose `/Names /Pages` registry names each stamp page with a key of
 * the form `identifier=label`:
 *
 *   - `identifier` (before the first `=`) is the stamp's durable identity
 *     and becomes the placed annotation's `/Name` — a standard name such as
 *     `Approved`, or a custom one such as Acrobat's `#LBGiYhk8V_oAfmqAPENiwD`;
 *   - `label` (after it) is what the picker shows and the placed
 *     annotation's default `/Subj`.
 *
 * The engine never interprets key text; this module does. A key without
 * `=` is its own label. `#` is never stripped — it is part of the identity.
 */
export interface StampKey {
  name: string;
  label: string;
}

export function parseStampKey(key: string): StampKey {
  const separator = key.indexOf('=');
  if (separator < 0) return { name: key, label: key };
  return { name: key.slice(0, separator), label: key.slice(separator + 1) };
}

export function stampKey(name: string, label: string): string {
  return `${name}=${label}`;
}

const BASE62 = 'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789';

/**
 * A fresh identifier for a stamp authored here, in Acrobat's own shape
 * (`#` + 22 base-62 characters) so a stamp created in this viewer keeps its
 * identity when the library is opened and placed in Acrobat.
 */
export function customStampName(random: () => number = Math.random): string {
  let out = '#';
  for (let i = 0; i < 22; i++) out += BASE62[Math.floor(random() * BASE62.length)];
  return out;
}

/** Asset ids are derived, not allocated: library id + identifier. */
export function assetIdFor(libraryId: string, name: string): string {
  return `${libraryId}:${name}`;
}
