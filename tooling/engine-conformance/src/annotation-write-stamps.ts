const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;

/**
 * The values a v3 annotation write stamps from the clock or a random source,
 * as placeholders. The engine gives a new annotation an `/NM` when the draft
 * names none (a UUID, bc0057f3a) and stamps `createdAt` on it and on an
 * attached file's info (fd175ba50 named the field), so a verbatim recording
 * would differ on every run. What matters is that each is there. Returns
 * `undefined` for a value that is not one of these.
 */
export function normalizeWriteStamps(key: string, value: unknown): unknown {
  if (key === 'createdAt' && typeof value === 'string') return '<stamped>';
  if (key === 'nm' && typeof value === 'string' && UUID.test(value)) return '<stamped>';
  return undefined;
}
