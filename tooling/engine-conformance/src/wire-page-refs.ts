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

// ── forms ──

function isFormWidget(
  value: Record<string, unknown>,
): value is { annotObjectNumber: number; page: { pageObjectNumber: number } | null } {
  return (
    !('kind' in value) &&
    typeof value.annotObjectNumber === 'number' &&
    'page' in value &&
    (value.page === null || isObjectNumberRef(value.page))
  );
}

/**
 * A form widget record (`FormWidget`, 40c96e5ab and 9f13cea3c) back to the
 * wire's `FormWidgetRef`: the engine-computed `ref` is dropped, and `page`
 * becomes `pageObjectNumber` in the same position, `0` when unplaced. Every
 * other key (and every non-widget value) passes through, recursively.
 */
export function flattenFormWidgets(value: unknown): unknown {
  if (Array.isArray(value)) return value.map(flattenFormWidgets);
  if (value === null || typeof value !== 'object') return value;
  const record = value as Record<string, unknown>;
  const widget = isFormWidget(record);
  const out: Record<string, unknown> = {};
  for (const [key, field] of Object.entries(record)) {
    if (widget && key === 'ref') continue;
    if (widget && key === 'page') {
      out.pageObjectNumber = (field as { pageObjectNumber: number } | null)?.pageObjectNumber ?? 0;
    } else {
      out[key] = flattenFormWidgets(field);
    }
  }
  return out;
}

// ── annotation ──

/**
 * The inverse of `flattenPageRefs` for the one place a draft names a page:
 * every `destination` holding a wire `pageObjectNumber` gets `page: PageRef`
 * in the same position, which is what `PdfDestination` takes. Everything
 * else passes through untouched, so a recorded draft stays wire-shaped.
 */
export function destinationPageRefs(value: unknown): unknown {
  if (Array.isArray(value)) return value.map(destinationPageRefs);
  if (value === null || typeof value !== 'object' || value instanceof Uint8Array) return value;
  const out: Record<string, unknown> = {};
  for (const [key, field] of Object.entries(value)) {
    if (
      key === 'destination' &&
      field !== null &&
      typeof field === 'object' &&
      typeof (field as { pageObjectNumber?: unknown }).pageObjectNumber === 'number'
    ) {
      const destination: Record<string, unknown> = {};
      for (const [k, v] of Object.entries(field)) {
        if (k === 'pageObjectNumber') destination.page = pageRef(v as number);
        else destination[k] = v;
      }
      out[key] = destination;
    } else {
      out[key] = destinationPageRefs(field);
    }
  }
  return out;
}
