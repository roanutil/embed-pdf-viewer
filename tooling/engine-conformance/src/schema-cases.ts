/**
 * Hand-written inputs for the schema parity vectors.
 *
 * These are the only vectors in this package that are not read out of a
 * running engine. They are JSON documents someone might send, and the question
 * they ask is narrow: does the handwritten zod schema and the Rust type agree
 * on which ones are well formed, and on what the accepted ones mean.
 *
 * `multiplatform-plan.md` Section 11 asks for exactly this, and names the
 * cases: "omitted/null/value, explicit undefined, fields that disallow null
 * (such as `MetadataPatch.trapped`), unknown enums, numeric widths, nested
 * unions, empty collections, invalid inputs and each schema's unknown-field
 * behavior." Explicit `undefined` is not JSON-expressible and lives in a
 * TypeScript-only test; everything else is here.
 *
 * The generator records zod's verdict; it does not encode one. If the schema
 * changes, the vector changes, and the Rust runner is what fails.
 */

/** Types the parity harness covers. One vector file each. */
export type SchemaName = 'MetadataPatch' | 'CustomMetadataPatch';

export interface SchemaCase {
  /** Stable slug, and what a failure names. */
  readonly name: string;
  /** Why the case exists, in one line. */
  readonly covers: string;
  /** The JSON document as it arrives on the wire. */
  readonly input: unknown;
}

export const SCHEMA_CASES: Readonly<Record<SchemaName, readonly SchemaCase[]>> = {
  MetadataPatch: [
    { name: 'empty', covers: 'Every field omitted: keep everything.', input: {} },
    {
      name: 'null-clears',
      covers: 'An explicit null is a clear, not an omission.',
      input: { title: null },
    },
    {
      name: 'value-sets',
      covers: 'A value is a set.',
      input: { title: 'Quarterly Report' },
    },
    {
      name: 'empty-string-is-a-set',
      covers: 'An empty string sets the field to empty; it is not a clear.',
      input: { title: '' },
    },
    {
      name: 'several-fields-at-once',
      covers: 'All three states in one document.',
      input: { title: null, author: 'Ada', subject: 'S' },
    },
    {
      name: 'unknown-field',
      covers: 'The schema is .strict(), so a typo is an error rather than a dropped key.',
      input: { titel: 'typo' },
    },
    {
      name: 'wrong-type',
      covers: 'A number where a string belongs.',
      input: { title: 42 },
    },
    {
      name: 'trapped-value',
      covers: 'The tri-valued enum, set.',
      input: { trapped: 'unknown' },
    },
    {
      name: 'trapped-null',
      covers: 'trapped is .optional() without .nullable(): null is an error, not an omission.',
      input: { trapped: null },
    },
    {
      name: 'trapped-unknown-member',
      covers: 'A value outside the enum.',
      input: { trapped: 'maybe' },
    },
    {
      name: 'custom-is-its-own-patch',
      covers:
        'Custom keys left MetadataPatch for their own patch in a4b8259f7, and the schema is strict, so a custom map here is an unknown key.',
      input: { custom: { Department: 'Legal' } },
    },
    {
      name: 'created-millisecond-precision',
      covers: 'The form iso_8601 emits.',
      input: { createdAt: '2026-09-22T08:00:00.000Z' },
    },
    {
      name: 'created-second-precision',
      covers: 'No fractional part.',
      input: { createdAt: '2026-09-22T08:00:00Z' },
    },
    {
      name: 'created-microsecond-precision',
      covers: 'zod allows any number of fractional digits.',
      input: { createdAt: '2026-09-22T08:00:00.123456Z' },
    },
    {
      name: 'created-leap-day',
      covers: 'February 29th in a leap year is a real date.',
      input: { createdAt: '2024-02-29T00:00:00Z' },
    },
    {
      name: 'created-impossible-date',
      covers: 'February 30th is not, in any year.',
      input: { createdAt: '2025-02-30T00:00:00Z' },
    },
    {
      name: 'created-offset',
      covers:
        'An explicit offset. IsoDateTime takes Z, an offset, or none since fd175ba50 (datetime with offset and local).',
      input: { createdAt: '2026-09-22T08:00:00+02:00' },
    },
    {
      name: 'created-not-a-date',
      covers: 'An arbitrary string. A Patch<String> would take it; the schema does not.',
      input: { createdAt: 'not-a-date' },
    },
    {
      name: 'created-null',
      covers: 'A date field is nullable, so null clears it.',
      input: { createdAt: null },
    },
    {
      name: 'created-without-offset',
      covers: 'A local time with no offset at all, which the local: true rule admits.',
      input: { createdAt: '2026-09-22T08:00:00' },
    },
    {
      name: 'created-pre-v3-spelling',
      covers:
        'The field was created until fd175ba50. Under the strict schema the old spelling is an unknown key, not an alias.',
      input: { created: '2026-09-22T08:00:00Z' },
    },
    {
      name: 'modified-not-a-date',
      covers: 'The same rule on the second date field, in case only one was wired.',
      input: { modifiedAt: 'yesterday' },
    },
  ],
  CustomMetadataPatch: [
    {
      name: 'custom-empty',
      covers: 'An empty patch is a present map: it changes nothing and is still well formed.',
      input: {},
    },
    {
      name: 'custom-per-key-three-state',
      covers: 'A key set, a key removed, and every other key left alone.',
      input: { Department: 'Legal', Obsolete: null },
    },
    {
      name: 'custom-null',
      covers:
        'The patch itself is a map: there is no "clear the concept of custom entries", so null is refused.',
      input: null,
    },
    {
      name: 'custom-empty-key',
      covers: 'An empty key is a key. Whether the engine then rejects it is a different gate.',
      input: { '': 'empty key' },
    },
  ],
};
