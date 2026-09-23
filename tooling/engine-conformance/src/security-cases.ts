/**
 * The document-security corpus: how a probe becomes the state the wire
 * carries.
 *
 * `multiplatform-port-order.md` G3 gates on "open, wrong password,
 * permission, close, cancel traces". This file covers the derivation half of
 * that, which is pure and needs no document: given what PDFium reported, what
 * shape does the engine produce? The half that needs a real encrypted file is
 * a separate runner.
 *
 * The permission words matter more than they look. PDF Standard security
 * handlers reserve bits 1 and 2 as zero and PDFium masks owner permissions
 * through that convention, so "all allowed" is `0xFFFFFFFC` and not
 * `0xFFFFFFFF`. An implementation comparing against the latter calls every
 * owner-unlocked document restricted, and nothing else in the pipeline would
 * notice.
 */

export interface BuildCase {
  readonly name: string;
  readonly covers: string;
  readonly openedAs: 'none' | 'user' | 'owner';
  readonly permissionsBits: number;
  readonly securityHandlerRevision: number | null;
}

/**
 * Inputs to `hasAllStandardSecurityPermissions`, which coerces with `u32()`
 * before masking.
 *
 * This list carries deliberately signed words. JavaScript's bitwise operators
 * return a signed 32-bit int, so `0xFFFFFFFC & ~(1 << 3)` is `-12` and not
 * `4294967284`; every shipping caller launders the word through `normalizeU32`
 * first, and the predicate does it again internally. A Rust port whose
 * signature is `u32` cannot receive the signed form at all, which makes the
 * conversion point the thing worth pinning rather than the predicate.
 */
export interface AllAllowedCase {
  readonly name: string;
  readonly covers: string;
  readonly raw: number;
}

export interface OpenedAsCodeCase {
  readonly name: string;
  readonly code: number;
}

export interface AdvisoryCase {
  readonly name: string;
  readonly covers: string;
  /** Raw permission word, decoded through `decodePdfBits` first. */
  readonly raw: number | null;
}

export interface StateCase {
  readonly name: string;
  readonly covers: string;
  /** Names a case in BUILD_CASES, or one of the three unbuilt shapes. */
  readonly probe: string;
  readonly accessEndpoint?: string;
  readonly cdnRequired?: boolean;
}

/** A fixed instant, so the vectors do not change every time they are made. */
export const PROBED_AT = 1_700_000_000_000;

/** Permission words worth distinguishing. */
const ALL_ALLOWED = 0xfffffffc;

export const BUILD_CASES: readonly BuildCase[] = [
  {
    name: 'unencrypted',
    covers: 'openedAs none means encryptionState none and a nulled revision.',
    openedAs: 'none',
    permissionsBits: 0xffffffff,
    securityHandlerRevision: 3,
  },
  {
    name: 'unencrypted-nulls-a-revision-it-was-given',
    covers: 'The revision-nulling rule, with a revision deliberately supplied.',
    openedAs: 'none',
    permissionsBits: ALL_ALLOWED,
    securityHandlerRevision: 6,
  },
  {
    name: 'owner-all-allowed',
    covers: 'The 0xFFFFFFFC convention: an owner unlock is all-allowed.',
    openedAs: 'owner',
    permissionsBits: ALL_ALLOWED,
    securityHandlerRevision: 4,
  },
  {
    name: 'owner-word-with-reserved-bits-set',
    covers:
      '0xFFFFFFFF also passes the mask, so a handler that does not clear bits 1 and 2 is still all-allowed.',
    openedAs: 'owner',
    permissionsBits: 0xffffffff,
    securityHandlerRevision: 6,
  },
  {
    name: 'user-print-only',
    covers: 'One bit short of everything is not all-allowed.',
    // `>>> 0` because JavaScript's `&` returns a signed int and every
    // shipping caller passes an already-normalized word.
    openedAs: 'user',
    permissionsBits: (ALL_ALLOWED & ~(1 << 3)) >>> 0,
    securityHandlerRevision: 4,
  },
  {
    name: 'user-nothing-allowed',
    covers: 'A word with none of the surfaced bits.',
    openedAs: 'user',
    permissionsBits: 0,
    securityHandlerRevision: 2,
  },
  {
    name: 'user-revision-2',
    covers: 'RC4-40, the oldest standard handler.',
    openedAs: 'user',
    permissionsBits: 0xfffff0c0,
    securityHandlerRevision: 2,
  },
  {
    name: 'user-revision-6',
    covers: 'AES-256, the newest.',
    openedAs: 'user',
    permissionsBits: 0xfffff0c0,
    securityHandlerRevision: 6,
  },
  {
    name: 'encrypted-without-a-revision',
    covers: 'An encrypted document whose revision PDFium did not report.',
    openedAs: 'user',
    permissionsBits: ALL_ALLOWED,
    securityHandlerRevision: null,
  },
  {
    name: 'user-all-allowed',
    covers:
      'A user unlock can still carry every permission, so all-allowed is not a proxy for owner.',
    openedAs: 'user',
    permissionsBits: ALL_ALLOWED,
    securityHandlerRevision: 4,
  },
] as const;

export const ALL_ALLOWED_CASES: readonly AllAllowedCase[] = [
  { name: 'zero', covers: 'Nothing allowed.', raw: 0 },
  { name: 'all-allowed-unsigned', covers: 'The 0xFFFFFFFC convention, written unsigned.', raw: 0xfffffffc },
  { name: 'all-allowed-signed', covers: 'The same word as JavaScript computes it: -4.', raw: -4 },
  { name: 'all-ones-unsigned', covers: 'Reserved bits set too.', raw: 0xffffffff },
  { name: 'all-ones-signed', covers: 'The same word signed: -1.', raw: -1 },
  {
    name: 'one-bit-short-signed',
    covers: '0xFFFFFFFC & ~(1 << 3) is -12 in JavaScript, and is not all-allowed either way.',
    raw: -12,
  },
  {
    name: 'one-bit-short-unsigned',
    covers: 'The same word unsigned.',
    raw: 0xfffffff4,
  },
  {
    name: 'reserved-bits-only',
    covers: 'Bits 1 and 2 are outside the mask, so setting only them allows nothing.',
    raw: 0b11,
  },
] as const;

/** `EPDF_PASSWORD_PERMISSION_*`: invalid = 0, none = 1, user = 2, owner = 3. */
export const OPENED_AS_CODE_CASES: readonly OpenedAsCodeCase[] = [
  { name: 'invalid', code: 0 },
  { name: 'none', code: 1 },
  { name: 'user', code: 2 },
  { name: 'owner', code: 3 },
  { name: 'out-of-range', code: 4 },
  { name: 'negative', code: -1 },
] as const;

export const ADVISORY_CASES: readonly AdvisoryCase[] = [
  { name: 'unprobed', covers: 'Null bits advise nothing.', raw: null },
  { name: 'none', covers: 'A zero word.', raw: 0 },
  {
    name: 'print-high-without-print',
    covers: 'Bit 12 refines bit 3 and does not stand alone, matching the resolver.',
    raw: 1 << 11,
  },
  { name: 'print-and-high', covers: 'Both, so the refinement applies.', raw: (1 << 2) | (1 << 11) },
  { name: 'fill-without-annotate', covers: 'Bit 9 alone still allows filling.', raw: 1 << 8 },
  {
    name: 'annotate-without-modify',
    covers: 'canModifyForms needs bit 4 as well as bit 6.',
    raw: 1 << 5,
  },
  {
    name: 'annotate-and-modify',
    covers: 'Both, so form modification is advised.',
    raw: (1 << 5) | (1 << 3),
  },
  { name: 'all-allowed', covers: 'The 0xFFFFFFFC word.', raw: 0xfffffffc },
  { name: 'unencrypted-word', covers: 'What PDFium reports with no encryption dict.', raw: -1 },
] as const;

export const STATE_CASES: readonly StateCase[] = [
  {
    name: 'unencrypted',
    covers: 'Nothing is required and nothing can be upgraded.',
    probe: 'unencrypted',
  },
  {
    name: 'owner-all-allowed',
    covers: 'An owner unlock has no owner left to upgrade to.',
    probe: 'owner-all-allowed',
  },
  {
    name: 'user-print-only',
    covers: 'A user unlock can upgrade, and needs nothing further to read.',
    probe: 'user-print-only',
  },
  {
    name: 'password-blocked',
    covers: 'The one case that produces a password reason.',
    probe: 'password-blocked',
  },
  {
    name: 'password-blocked-with-endpoint',
    covers: 'The endpoint appears only when access is required.',
    probe: 'password-blocked',
    accessEndpoint: 'https://example.test/access',
  },
  {
    name: 'unencrypted-with-endpoint',
    covers: 'The same endpoint is withheld when nothing is required.',
    probe: 'unencrypted',
    accessEndpoint: 'https://example.test/access',
  },
  {
    name: 'unsupported',
    covers: 'An unsupported handler requires nothing, which is worth pinning rather than assuming.',
    probe: 'unsupported',
  },
  {
    name: 'unknown',
    covers: 'Only the unknown state produces permissions-unknown.',
    probe: 'unknown',
  },
  {
    name: 'unknown-with-cdn',
    covers: 'Reason order is part of the wire shape: cdn is pushed before permissions-unknown.',
    probe: 'unknown',
    cdnRequired: true,
  },
  {
    name: 'password-blocked-with-cdn',
    covers: 'And password before cdn.',
    probe: 'password-blocked',
    cdnRequired: true,
    accessEndpoint: 'https://example.test/access',
  },
  {
    name: 'unencrypted-with-cdn',
    covers: 'A cdn requirement alone is enough to require access.',
    probe: 'unencrypted',
    cdnRequired: true,
    accessEndpoint: 'https://example.test/access',
  },
] as const;
