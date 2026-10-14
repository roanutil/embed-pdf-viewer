/**
 * The vectors pin the WIRE, which names a page by its object number. The
 * public engine API names it by `PageRef` (`9f13cea3c`, "Address pages by
 * PageRef everywhere"), so a generator that reads through the public API
 * converts at both ends: object numbers in, `pageObjectNumber` out.
 */

/** An object number as the public API's `PageRef`. */
export function pageRef(pageObjectNumber: number): {
  kind: 'objectNumber';
  pageObjectNumber: number;
} {
  return { kind: 'objectNumber', pageObjectNumber };
}

export function pageRefs(pageObjectNumbers: readonly number[]) {
  return pageObjectNumbers.map(pageRef);
}

/** A `PageRef` list back to the wire's object numbers. */
export function pageObjectNumbers(refs: unknown): number[] {
  return (refs as { pageObjectNumber: number }[]).map((ref) => ref.pageObjectNumber);
}

function isObjectNumberRef(value: unknown): value is { pageObjectNumber: number } {
  if (value === null || typeof value !== 'object' || Array.isArray(value)) return false;
  const keys = Object.keys(value);
  return (
    keys.length === 2 &&
    (value as { kind?: unknown }).kind === 'objectNumber' &&
    typeof (value as { pageObjectNumber?: unknown }).pageObjectNumber === 'number'
  );
}

/**
 * Every `ref` or `page` holding an object-number `PageRef` becomes
 * `pageObjectNumber`, in the same position, so key order matches the wire.
 */
export function flattenPageRefs(value: unknown): unknown {
  if (Array.isArray(value)) return value.map(flattenPageRefs);
  if (value === null || typeof value !== 'object') return value;
  const out: Record<string, unknown> = {};
  for (const [key, field] of Object.entries(value)) {
    if ((key === 'ref' || key === 'page') && isObjectNumberRef(field)) {
      out.pageObjectNumber = field.pageObjectNumber;
    } else {
      out[key] = flattenPageRefs(field);
    }
  }
  return out;
}
