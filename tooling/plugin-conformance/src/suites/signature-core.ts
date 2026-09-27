/**
 * `packages/core/signature`, the cryptographic half of digital signatures.
 * Replayed by `crates/plugins/signature-core/tests/conformance.rs`.
 *
 * The package is a library, not a plugin, but most of it is asynchronous
 * (WebCrypto), and function cases must be synchronous. So the parts that
 * return promises run as scenarios through a small workspace plugin,
 * `signature-core`, whose capability is one wrapper per exported function.
 * `parseDetachedCms`, `profileFor` and `certificateCommonName` are
 * synchronous and are function cases.
 *
 * Inputs come from `fixtures/signature/fixtures.json` (keys, certificates,
 * CMS blobs), generated once because key generation and ECDSA / RSA-PSS
 * signatures are random. Two adapter conventions:
 *
 *   - A `Date` travels as epoch milliseconds (`signingTime`, `genTime`);
 *     a returned `Date` prints as `{$date}`.
 *   - The WebCrypto adapters (`webCryptoSigner`, `generateSigningKeyPair`,
 *     `selfSignedCertificate`, `createTestSigner`, `personalSigner`) run
 *     against a scripted WebCrypto: `generateKey` hands out fixture keys,
 *     `sign` signs RSASSA-PKCS1-v1_5 for real and answers other algorithms
 *     with fixed bytes, `getRandomValues` fills with 0x5a, and each call is
 *     logged as a callback. The Rust port takes WebCrypto as a host trait and
 *     its test host answers the same way.
 */
import { createHash, createPrivateKey, sign as nodeSign, webcrypto } from 'node:crypto';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

import type { AnyPlugin } from '@embedpdf/core';

import {
  buildDetachedCms,
  certificateCommonName,
  createTestSigner,
  generateSigningKeyPair,
  memoryKeyStore,
  parseDetachedCms,
  personalSigner,
  profileFor,
  remoteSigner,
  selfSignedCertificate,
  sign,
  validateChain,
  validateSignatures,
  verifyCmsSignature,
  verifyForCompletion,
  webCryptoSigner,
  type PersonalKeyRecord,
  type RawSigner,
} from '../../../../packages/core/signature/src/index';

import type { FakeDocumentSpec, ResponseSpec } from '../fake-engine';
import { registerPlugin, type PluginEnv, type ScenarioCase, type Step } from '../scenario';
import type { FunctionCase, Suite } from '../vectors';

// ── fixtures ─────────────────────────────────────────────────────────────────

interface Fixtures {
  keys: Record<string, { kind: 'rsa' | 'p256' | 'p384' | 'p521'; pkcs8: string; spki: string }>;
  certs: Record<string, string>;
  cms: Record<string, string>;
  signatures: Record<string, string>;
}
const fixtures = JSON.parse(
  readFileSync(join(dirname(fileURLToPath(import.meta.url)), '../../fixtures/signature/fixtures.json'), 'utf8'),
) as Fixtures;

const bytesOf = (b64: string) => new Uint8Array(Buffer.from(b64, 'base64'));
const b64 = (bytes: Uint8Array) => Buffer.from(bytes).toString('base64');
const $b = (b64text: string) => ({ $bytes: b64text });
const cmsArg = (name: string) => {
  const value = fixtures.cms[name];
  if (!value) throw new Error(`[signature-core] no CMS fixture "${name}"`);
  return $b(value);
};
const certArg = (name: string) => {
  const value = fixtures.certs[name];
  if (!value) throw new Error(`[signature-core] no certificate fixture "${name}"`);
  return $b(value);
};
const sha256hex = (bytes: Uint8Array) => createHash('sha256').update(bytes).digest('hex');
const digestBytes = (n: number, seed: number) => new Uint8Array(n).map((_, i) => (i * 7 + seed) & 0xff);
const DIGEST_LENGTH = { sha256: 32, sha384: 48, sha512: 64 } as const;

/** The raw signature a fixture key makes over `data`: RSASSA-PKCS1-v1_5 is
 *  deterministic and is computed; RSA-PSS and ECDSA come from the table the
 *  fixture script recorded. */
function rawSignature(key: string, algorithm: RawSigner['algorithm'], hash: RawSigner['hash'], data: Uint8Array): Uint8Array {
  if (algorithm === 'RSA-PKCS1-v1_5') {
    const pk = createPrivateKey({ key: Buffer.from(fixtures.keys[key].pkcs8, 'base64'), format: 'der', type: 'pkcs8' });
    return new Uint8Array(nodeSign(hash, data, pk));
  }
  const recorded = fixtures.signatures[sha256hex(data)];
  if (!recorded) throw new Error(`[signature-core] no recorded ${algorithm} signature for these bytes; rerun fixtures/signature/generate.ts`);
  return bytesOf(recorded);
}

/** What a raw signer must answer for one build: run the build once with a
 *  capturing signer, and sign the bytes it asked for. */
async function signatureFor(input: {
  key: string;
  chain: string[];
  algorithm: RawSigner['algorithm'];
  hash: RawSigner['hash'];
  profile: 'pkcs7' | 'cades-b';
  digest: Uint8Array;
  signingTime?: number;
}): Promise<string> {
  let answer: Uint8Array | null = null;
  await buildDetachedCms({
    digest: input.digest,
    hash: input.hash,
    profile: input.profile,
    signingTime: input.signingTime === undefined ? undefined : new Date(input.signingTime),
    signer: {
      kind: 'raw',
      certificateChain: input.chain.map((c) => bytesOf(fixtures.certs[c])),
      algorithm: input.algorithm,
      hash: input.hash,
      sign: async (data) => (answer = rawSignature(input.key, input.algorithm, input.hash, data)),
    },
  });
  return b64(answer!);
}

// ── the scripted WebCrypto ───────────────────────────────────────────────────

type Log = PluginEnv['log'];
type Subtle = Record<string, unknown>;

async function fixtureKeyPair(params: { name: string; hash?: unknown; namedCurve?: string }, extractable: boolean, fixture = 'leaf') {
  const subtle = webcrypto.subtle;
  const hashName = typeof params.hash === 'string' ? params.hash : (params.hash as { name?: string } | undefined)?.name;
  if (params.name === 'ECDSA') {
    const key = { 'P-256': 'p256', 'P-384': 'p384', 'P-521': 'p521' }[params.namedCurve ?? 'P-256'] ?? 'p256';
    const algorithm = { name: 'ECDSA', namedCurve: params.namedCurve ?? 'P-256' };
    return {
      privateKey: await subtle.importKey('pkcs8', bytesOf(fixtures.keys[key].pkcs8), algorithm, extractable, ['sign']),
      publicKey: await subtle.importKey('spki', bytesOf(fixtures.keys[key].spki), algorithm, true, ['verify']),
    };
  }
  const algorithm = { name: params.name, hash: hashName ?? 'SHA-256' };
  return {
    privateKey: await subtle.importKey('pkcs8', bytesOf(fixtures.keys[fixture].pkcs8), algorithm, extractable, ['sign']),
    publicKey: await subtle.importKey('spki', bytesOf(fixtures.keys[fixture].spki), algorithm, true, ['verify']),
  };
}

const describeKey = (key: CryptoKey) => ({
  type: key.type,
  extractable: key.extractable,
  algorithm: key.algorithm,
  usages: key.usages,
});

/** Fixed bytes for a signature WebCrypto would randomise. */
function fakeSignature(key: CryptoKey): Uint8Array {
  const algorithm = key.algorithm as { name: string; namedCurve?: string };
  if (algorithm.name === 'ECDSA') {
    const size = { 'P-256': 64, 'P-384': 96, 'P-521': 132 }[algorithm.namedCurve ?? 'P-256'] ?? 64;
    return new Uint8Array(size).map((_, i) => (i * 3 + 1) & 0xff);
  }
  return new Uint8Array(256).map((_, i) => (i * 5 + 2) & 0xff);
}

async function withWebCrypto<T>(log: Log, run: () => Promise<T>): Promise<T> {
  const subtle = webcrypto.subtle as unknown as Subtle;
  const real = {
    sign: webcrypto.subtle.sign.bind(webcrypto.subtle),
    exportKey: webcrypto.subtle.exportKey.bind(webcrypto.subtle),
  };
  const cryptoObject = globalThis.crypto as unknown as Record<string, unknown>;
  const realRandom = cryptoObject.getRandomValues;
  subtle.generateKey = async (params: { name: string }, extractable: boolean, usages: string[]) => {
    log('webcrypto.generateKey', [params, extractable, usages]);
    return fixtureKeyPair(params, extractable);
  };
  subtle.sign = async (params: { name: string }, key: CryptoKey, data: BufferSource) => {
    const bytes = new Uint8Array(ArrayBuffer.isView(data) ? data.buffer.slice(data.byteOffset, data.byteOffset + data.byteLength) : data);
    log('webcrypto.sign', [params, describeKey(key), bytes]);
    if (params.name === 'RSASSA-PKCS1-v1_5') return real.sign(params, key, bytes);
    return fakeSignature(key).buffer;
  };
  subtle.exportKey = async (format: 'spki', key: CryptoKey) => {
    log('webcrypto.exportKey', [format, describeKey(key)]);
    return real.exportKey(format, key);
  };
  cryptoObject.getRandomValues = <A extends ArrayBufferView>(array: A): A => {
    log('webcrypto.getRandomValues', [array.byteLength]);
    new Uint8Array(array.buffer, array.byteOffset, array.byteLength).fill(0x5a);
    return array;
  };
  try {
    return await run();
  } finally {
    delete subtle.generateKey;
    delete subtle.sign;
    delete subtle.exportKey;
    cryptoObject.getRandomValues = realRandom;
  }
}

/** A private key described by the vector: a fixture key imported with an
 *  algorithm, or (`fake`) an object with only `algorithm`, for algorithms
 *  WebCrypto would not import. */
interface KeySpec {
  readonly fixture?: string;
  readonly algorithm: { name: string; hash?: string; namedCurve?: string };
  readonly fake?: boolean;
}
async function importKeySpec(spec: KeySpec): Promise<CryptoKey> {
  if (spec.fake) return { algorithm: spec.algorithm } as unknown as CryptoKey;
  const fixture = fixtures.keys[spec.fixture ?? 'leaf'];
  return webcrypto.subtle.importKey('pkcs8', bytesOf(fixture.pkcs8), spec.algorithm, false, ['sign']);
}

const describeSigner = (signer: RawSigner) => ({
  kind: signer.kind,
  certificateChain: signer.certificateChain,
  algorithm: signer.algorithm,
  hash: signer.hash,
});

// ── the harness plugin ───────────────────────────────────────────────────────

const toDate = (ms: unknown) => (typeof ms === 'number' ? new Date(ms) : undefined);
const withAt = (at: { kind: string; genTime?: number } | undefined) =>
  at === undefined ? undefined : at.kind === 'timestamp' ? { ...at, genTime: new Date(at.genTime!) } : at;

function signaturePlugin(env: PluginEnv): AnyPlugin {
  return {
    id: 'signature-core',
    token: env.token('signature-core'),
    capability: (ctx) => ({
      verifyCmsSignature: (cms: Uint8Array) => verifyCmsSignature(cms),
      buildDetachedCms: (input: Parameters<typeof buildDetachedCms>[0] & { signingTime?: number }) =>
        buildDetachedCms({ ...input, signingTime: toDate(input.signingTime) }),
      verifyForCompletion: (input: Parameters<typeof verifyForCompletion>[0]) => verifyForCompletion(input),
      validateChain: (cms: Uint8Array, trust: Parameters<typeof validateChain>[1], at?: { kind: string; genTime?: number }) =>
        validateChain(cms, trust, withAt(at) as Parameters<typeof validateChain>[2]),
      sign: (documentId: string, input: Parameters<typeof sign>[1]) => sign(ctx.documentHandle(documentId)!, input),
      validateSignatures: (
        documentId: string,
        options?: Parameters<typeof validateSignatures>[1] & { at?: { kind: string; genTime?: number } },
      ) =>
        validateSignatures(
          ctx.documentHandle(documentId)!,
          options === undefined ? undefined : ({ ...options, at: withAt(options.at) } as Parameters<typeof validateSignatures>[1]),
        ),
      remoteSign: (port: Parameters<typeof remoteSigner>[0], request: Parameters<ReturnType<typeof remoteSigner>['sign']>[0]) => {
        const signer = remoteSigner(port);
        return signer.sign(request);
      },
      memoryKeyStore: async (ops: { op: 'load' | 'save' | 'remove'; subject: string; record?: unknown }[]) => {
        const store = memoryKeyStore();
        const results: unknown[] = [];
        for (const step of ops) {
          if (step.op === 'load') results.push(await store.load(step.subject));
          else if (step.op === 'save') results.push(await store.save(step.subject, step.record as PersonalKeyRecord));
          else results.push(await store.remove(step.subject));
        }
        return results;
      },
      webCryptoSigner: (key: KeySpec, certificateChain: Uint8Array[], hash?: RawSigner['hash'], data?: Uint8Array) =>
        withWebCrypto(env.log, async () => {
          const signer = webCryptoSigner({ privateKey: await importKeySpec(key), certificateChain, hash });
          const described = describeSigner(signer);
          return data === undefined ? described : { ...described, signature: await signer.sign(data) };
        }),
      generateSigningKeyPair: (
        algorithm: Parameters<typeof generateSigningKeyPair>[0],
        hash: Parameters<typeof generateSigningKeyPair>[1],
        extractable: boolean,
      ) =>
        withWebCrypto(env.log, async () => {
          const pair = await generateSigningKeyPair(algorithm, hash, extractable);
          return { privateKey: describeKey(pair.privateKey), publicKey: describeKey(pair.publicKey) };
        }),
      selfSignedCertificate: (input: {
        key: KeySpec;
        commonName: string;
        hash: 'sha256' | 'sha384' | 'sha512';
        validityDays?: number;
      }) =>
        withWebCrypto(env.log, async () => {
          const pair = await fixtureKeyPair({ ...input.key.algorithm, hash: input.key.algorithm.hash }, false, input.key.fixture);
          return selfSignedCertificate({
            publicKey: pair.publicKey,
            privateKey: pair.privateKey,
            commonName: input.commonName,
            hash: input.hash,
            validityDays: input.validityDays,
          });
        }),
      createTestSigner: (options?: Parameters<typeof createTestSigner>[0], data?: Uint8Array) =>
        withWebCrypto(env.log, async () => {
          const signer = await createTestSigner(options);
          const described = { ...describeSigner(signer), certificate: signer.certificate, privateKey: describeKey(signer.privateKey) };
          return data === undefined ? described : { ...described, signature: await signer.sign(data) };
        }),
      personalSigner: (
        input: Omit<Parameters<typeof personalSigner>[0], 'store'> & { store?: Parameters<typeof personalSigner>[0]['store'] },
        times = 1,
      ) =>
        withWebCrypto(env.log, async () => {
          const store = input.store ?? memoryKeyStore();
          const out: unknown[] = [];
          for (let i = 0; i < times; i += 1) {
            const signer = await personalSigner({ ...input, store });
            out.push({ ...describeSigner(signer), certificate: signer.certificate, subject: signer.subject });
          }
          return out;
        }),
    }),
  };
}

registerPlugin('signature-core', { factory: (_config, env) => signaturePlugin(env) });

// ── function cases ───────────────────────────────────────────────────────────

const parseFns = {
  parseDetachedCms: (cms: Uint8Array) => parseDetachedCms(cms),
  profileFor: (subFilter: 'adbe.pkcs7.detached' | 'ETSI.CAdES.detached') => profileFor(subFilter),
  certificateCommonName: (der: Uint8Array) => certificateCommonName(der),
};

const parseCases: FunctionCase[] = [
  ...Object.keys(fixtures.cms).map((name) => ({
    name: `parse-${name}`,
    fn: 'parseDetachedCms',
    args: [cmsArg(name)],
  })),
  {
    name: 'parse-empty',
    covers: 'An empty CMS is refused before BER parsing.',
    fn: 'parseDetachedCms',
    args: [$b('')],
  },
  {
    name: 'parse-trailing-zeros',
    covers: 'The /Contents padding after the CMS is ignored: BER parsing reads the first element.',
    fn: 'parseDetachedCms',
    args: [$b(b64(new Uint8Array([...bytesOf(fixtures.cms.rsaCades), ...new Uint8Array(64)])))],
  },
  { name: 'profile-pkcs7', fn: 'profileFor', args: ['adbe.pkcs7.detached'] },
  { name: 'profile-cades', fn: 'profileFor', args: ['ETSI.CAdES.detached'] },
  ...[
    'leaf', 'root', 'corpus', 'noCommonName', 'emptyCommonName', 'integerCommonName', 'bmpCommonName', 'leafP256',
    'universalStringCommonName', 'utf8AccentedCommonName', 'rdnSequence', 'attributeAsSet', 'attributeOneMember', 'signatureAsOctets',
  ].map((name) => ({
    name: `common-name-${name}`,
    fn: 'certificateCommonName',
    args: [certArg(name)],
  })),
  {
    name: 'common-name-not-der',
    covers: 'A certificate that is not BER throws pkijs\'s AsnError.',
    fn: 'certificateCommonName',
    args: [$b('AQID')],
  },
  {
    name: 'common-name-not-certificate',
    covers: 'BER that is not a certificate throws.',
    fn: 'certificateCommonName',
    args: [$b(b64(new Uint8Array([0x30, 0x03, 0x02, 0x01, 0x05])))],
  },
];

// ── scenario helpers ─────────────────────────────────────────────────────────

const plugins = [{ factory: 'signature-core' }];
const JAN_2026 = Date.UTC(2026, 0, 15);
const start: Step = { op: 'call', target: 'kernel', method: 'start' };
const toJan2026: Step = { op: 'advance', ms: JAN_2026 };
const openDoc: Step = { op: 'call', target: 'documents', method: 'open', args: [{ kind: 'bytes', id: 'd', bytes: $b('') }] };
const call = (method: string, ...args: unknown[]): Step => ({ op: 'call', target: { token: 'signature-core' }, method, args });

/** One scenario: the plugin, the clock at 2026-01-15, then `steps`. */
function scenario(name: string, covers: string, steps: Step[], engine?: Record<string, FakeDocumentSpec>): ScenarioCase {
  return {
    name,
    covers,
    plugins,
    snapshot: 'none',
    awaitCalls: true,
    ...(engine ? { engine: { documents: engine } } : {}),
    steps: [start, toJan2026, ...(engine ? [openDoc] : []), ...steps],
  };
}

const rawSignerArg = (chain: string[], algorithm: RawSigner['algorithm'], hash: RawSigner['hash'], answer: unknown) => ({
  kind: 'raw',
  certificateChain: chain.map(certArg),
  algorithm,
  hash,
  sign: { $fn: 'raw.sign', ...(answer as object) },
});

const prepared = (over: Record<string, unknown> = {}) => ({
  signingId: 'signing-1',
  digest: $b(b64(digestBytes(32, 3))),
  algorithm: 'sha256',
  byteRange: [0, 100, 8292, 50],
  contentsSize: 8192,
  subFilter: 'ETSI.CAdES.detached',
  expectedVersion: { baseSha256: 'base', editsVersion: 0 },
  expiresAt: null,
  ...over,
});

// ── verify ───────────────────────────────────────────────────────────────────

const verifyNames = [
  'rsaCades', 'rsaPkcs7', 'rsaSha384', 'rsaSha512', 'psssha256', 'psssha384', 'psssha512',
  'ecP256', 'ecP384', 'ecP521', 'ecP256Sha512', 'pdfCertified', 'pdfApproval', 'pdfCompat', 'skiSid', 'withTimestamp',
  'unknownSignatureAlgorithm', 'sha1Digest', 'rsaCadesTampered', 'ecP256Tampered', 'ecP256GarbageSignature',
  'ecP256NotSequence', 'rsaShortSignature', 'essMismatch', 'essExplicitSha256', 'essSha1', 'essNoHash',
  'contentTypeNotData', 'contentTypeEmpty', 'noSignedAttrs', 'unknownDigest', 'twoSigners', 'noCertificates',
  'notSignedData', 'truncated', 'unknownCurve', 'ecNoParams', 'dsaKey',
  // Keys labelled with algorithms that are not key algorithms, then ECDSA and RSA-PSS encodings.
  'rsaKeyAsSha256WithRsa', 'rsaKeyAsOaep', 'rsaKeyAsEcdh', 'rsaKeyAsAesCbc', 'rsaKeyAsAesGcm', 'rsaKeyAsAesCfb', 'rsaKeyAsAesKw',
  'rsaKeyAsHmac', 'rsaKeyAsDh', 'rsaKeyAsSha1', 'rsaKeyAsPbkdf2', 'ecKeyAsEcdsaSha512', 'ecdsaSAsOctets', 'ecdsaRPadded',
  'ecdsaZeroIntegers', 'ecP521Sha384', 'pssSaltZero', 'pssSalt256', 'pssSha1', 'pssSha1ParamsOverSha256', 'pssUnknownHash',
];
const verifyCases: ScenarioCase[] = verifyNames.map((name) =>
  scenario(`verify-${name}`, `verifyCmsSignature over the ${name} fixture.`, [call('verifyCmsSignature', cmsArg(name))]),
);

// ── build ────────────────────────────────────────────────────────────────────

const leafChain = ['leaf', 'inter'];
const buildInput = async (over: {
  key?: string;
  chain?: string[];
  algorithm?: RawSigner['algorithm'];
  hash?: RawSigner['hash'];
  signerHash?: RawSigner['hash'];
  profile?: 'pkcs7' | 'cades-b';
  signingTime?: number;
  digest?: Uint8Array;
  answer?: unknown;
}) => {
  const hash = over.hash ?? 'sha256';
  const signerHash = over.signerHash ?? hash;
  const algorithm = over.algorithm ?? 'RSA-PKCS1-v1_5';
  const chain = over.chain ?? leafChain;
  const profile = over.profile ?? 'cades-b';
  const digest = over.digest ?? digestBytes(DIGEST_LENGTH[hash], 3);
  const answer =
    over.answer ??
    ({
      resolves: $b(
        await signatureFor({ key: over.key ?? 'leaf', chain, algorithm, hash, profile, digest, signingTime: over.signingTime }),
      ),
    } as const);
  return {
    digest: $b(b64(digest)),
    hash,
    profile,
    ...(over.signingTime === undefined ? {} : { signingTime: over.signingTime }),
    signer: rawSignerArg(chain, algorithm, signerHash, answer),
  };
};

const SEPT_2026 = Date.UTC(2026, 8, 11);
const SIGNING_TIMES = [
  Date.UTC(1950, 0, 1, 0, 0, 1), Date.UTC(1970, 0, 1), Date.UTC(1984, 1, 29, 6, 7, 8), Date.UTC(1999, 11, 31, 23, 59, 59, 999),
  Date.UTC(2000, 0, 1), Date.UTC(2000, 1, 29, 13, 45, 27), Date.UTC(2000, 2, 1, 1, 2, 3), Date.UTC(2023, 11, 31, 12),
  Date.UTC(2024, 0, 1), Date.UTC(2024, 1, 28), Date.UTC(2024, 1, 29), Date.UTC(2024, 2, 1), Date.UTC(2049, 11, 31, 23, 59, 59),
  ...Array.from({ length: 12 }, (_, month) => Date.UTC(1988, month, 1, month, month * 5, month * 4)),
  ...Array.from({ length: 12 }, (_, month) => Date.UTC(2037, month, 31 - (month % 3), 23 - month, 59 - month, 1 + month)),
  // Outside 1900 to 2099 asn1js writes a year of more than two characters as nothing, or a negative one.
  Date.UTC(2100, 0, 1, 1, 1, 1), Date.UTC(1899, 11, 31, 22, 58, 57), Date.UTC(1890, 5, 15),
  // Before 1 March of year 0 the day count is negative.
  Date.UTC(-5, 6, 20, 3, 4, 5), Date.UTC(-1, 1, 28, 12),
];
// A PKCS#7 build whose signer answers nothing: only the attributes matter.
const PKCS7_NO_SIGNATURE = await buildInput({ profile: 'pkcs7', answer: { resolves: $b('') } });
const buildCases: ScenarioCase[] = [
  scenario('build-rsa-cades', 'A CAdES-B CMS: content-type, message-digest and ESS signing-certificate-v2, signed over the attribute SET.', [
    call('buildDetachedCms', await buildInput({})),
  ]),
  scenario('build-rsa-pkcs7-time', 'A PKCS#7 CMS carries the signing-time attribute it was given, as UTCTime.', [
    call('buildDetachedCms', await buildInput({ profile: 'pkcs7', signingTime: SEPT_2026 })),
  ]),
  scenario('build-rsa-pkcs7-no-time', 'A PKCS#7 CMS without a signing time has only content-type and message-digest.', [
    call('buildDetachedCms', await buildInput({ profile: 'pkcs7' })),
  ]),
  scenario('build-cades-ignores-time', 'CAdES never carries signing-time, even when one is passed.', [
    call('buildDetachedCms', await buildInput({ signingTime: SEPT_2026 })),
  ]),
  scenario('build-rsa-sha384', 'sha384WithRSAEncryption and a 48-byte digest.', [
    call('buildDetachedCms', await buildInput({ hash: 'sha384' })),
  ]),
  scenario('build-rsa-sha512', 'sha512WithRSAEncryption and a 64-byte digest.', [
    call('buildDetachedCms', await buildInput({ hash: 'sha512' })),
  ]),
  ...(['sha256', 'sha384', 'sha512'] as const).map((hash) =>
    scenario(`build-pss-${hash}`, `RSASSA-PSS parameters for ${hash}: hash, MGF1 over the same hash, salt the hash length.`, []),
  ),
  scenario('build-ecdsa-p256', 'ECDSA: the raw r||s becomes a DER SEQUENCE of two INTEGERs.', []),
  scenario('build-ecdsa-p384', 'ECDSA over P-384 with SHA-384.', []),
  scenario('build-ecdsa-p521', 'ECDSA over P-521 in a PKCS#7 CMS; r and s are 66 bytes each.', []),
  scenario('build-ecdsa-odd-length', 'An ECDSA answer of odd length encodes as an empty signature.', [
    call('buildDetachedCms', await buildInput({ algorithm: 'ECDSA', chain: ['leafP256'], answer: { resolves: $b('AQID') } })),
  ]),
  scenario('build-ecdsa-leading-zeros', 'Leading zero bytes of r and s are stripped, and a high bit gets a zero byte.', [
    call(
      'buildDetachedCms',
      await buildInput({
        algorithm: 'ECDSA',
        chain: ['leafP256'],
        answer: { resolves: $b(b64(new Uint8Array(64).map((_, i) => (i < 3 ? 0 : i === 32 ? 0x80 : i)))) },
      }),
    ),
  ]),
  scenario('build-wrong-digest-length', 'A digest whose length does not fit the hash is refused before anything is signed.', [
    call('buildDetachedCms', await buildInput({ digest: digestBytes(20, 3), answer: { resolves: $b('') } })),
  ]),
  scenario('build-signer-hash-mismatch', 'A signer that hashes with another algorithm than the document was prepared with is refused.', [
    call('buildDetachedCms', await buildInput({ signerHash: 'sha384', answer: { resolves: $b('') } })),
  ]),
  scenario('build-no-certificate', 'A signer without a certificate is refused.', [
    call('buildDetachedCms', await buildInput({ chain: [], answer: { resolves: $b('') } })),
  ]),
  scenario('build-signer-rejects', 'A signer that rejects rejects the build with its error.', [
    call('buildDetachedCms', await buildInput({ answer: { rejects: { name: 'NotAllowedError', message: 'user cancelled' } } })),
  ]),
  scenario('build-signature-128-bytes', 'A 128-byte signature takes a long-form length.', [
    call('buildDetachedCms', await buildInput({ answer: { resolves: $b(b64(new Uint8Array(128).map((_, i) => i))) } })),
  ]),
  scenario(
    'build-signing-times',
    'Signing times across the UTCTime range: leap days, era and month edges, and a time of day.',
    SIGNING_TIMES.map((signingTime) => call('buildDetachedCms', { ...PKCS7_NO_SIGNATURE, signingTime })),
  ),
  scenario('build-bad-certificate', 'A chain entry that is not a certificate throws pkijs\'s error.', [
    call('buildDetachedCms', {
      ...(await buildInput({ answer: { resolves: $b('') } })),
      signer: { kind: 'raw', certificateChain: [$b('AQID')], algorithm: 'RSA-PKCS1-v1_5', hash: 'sha256', sign: { $fn: 'raw.sign', resolves: $b('') } },
    }),
  ]),
];
// The PSS and ECDSA builds answer from the recorded signatures, keyed by the bytes signed.
const recorded: Record<string, Parameters<typeof buildInput>[0]> = {
  'build-pss-sha256': { algorithm: 'RSA-PSS', hash: 'sha256' },
  'build-pss-sha384': { algorithm: 'RSA-PSS', hash: 'sha384' },
  'build-pss-sha512': { algorithm: 'RSA-PSS', hash: 'sha512' },
  'build-ecdsa-p256': { algorithm: 'ECDSA', key: 'p256', chain: ['leafP256', 'inter'] },
  'build-ecdsa-p384': { algorithm: 'ECDSA', key: 'p384', chain: ['leafP384', 'inter'], hash: 'sha384' },
  'build-ecdsa-p521': { algorithm: 'ECDSA', key: 'p521', chain: ['leafP521', 'inter'], hash: 'sha512', profile: 'pkcs7' },
};
for (const scenarioCase of buildCases) {
  const over = recorded[scenarioCase.name];
  if (over) (scenarioCase.steps as Step[]).push(call('buildDetachedCms', await buildInput(over)));
}

// ── the completion gate ──────────────────────────────────────────────────────

const gate = (cms: unknown, profile: 'pkcs7' | 'cades-b', over: Record<string, unknown> = {}) =>
  call('verifyForCompletion', { cms, prepared: prepared(over), profile });
const gateCases: ScenarioCase[] = [
  scenario('gate-cades-ok', 'A CAdES CMS over the prepared digest passes.', [gate(cmsArg('rsaCades'), 'cades-b')]),
  scenario('gate-pkcs7-ok', 'A PKCS#7 CMS passes the PKCS#7 profile, signing-time and all.', [gate(cmsArg('rsaPkcs7'), 'pkcs7')]),
  scenario('gate-too-large', 'A CMS bigger than the reserved /Contents is refused first.', [gate(cmsArg('rsaCades'), 'cades-b', { contentsSize: 100 })]),
  scenario('gate-malformed', 'A CMS that does not parse is `malformed`, with the parser\'s message.', [gate(cmsArg('twoSigners'), 'cades-b')]),
  scenario('gate-malformed-type-error', 'A parser failure that is not a CmsError reports its text.', [gate(cmsArg('contentTypeEmpty'), 'cades-b')]),
  scenario('gate-algorithm-mismatch', 'A CMS digesting with another algorithm than prepared.', [gate(cmsArg('rsaSha384'), 'cades-b')]),
  scenario('gate-digest-mismatch', 'A CMS whose message digest is not the prepared digest.', [
    gate(cmsArg('rsaCades'), 'cades-b', { digest: $b(b64(digestBytes(32, 4))) }),
  ]),
  scenario('gate-digest-length-mismatch', 'A prepared digest of another length is a digest mismatch, not an error.', [
    gate(cmsArg('rsaCades'), 'cades-b', { digest: $b(b64(digestBytes(31, 3))) }),
  ]),
  scenario('gate-cades-without-ess', 'CAdES requires signing-certificate-v2.', [gate(cmsArg('rsaPkcs7NoTime'), 'cades-b')]),
  scenario('gate-cades-with-time', 'PAdES forbids signing-time; checked after the ESS attribute.', [gate(cmsArg('cadesWithTime'), 'cades-b')]),
  scenario('gate-pkcs7-with-time', 'The same CMS is refused for its signature, not its profile, under PKCS#7.', [gate(cmsArg('generalizedTime'), 'pkcs7')]),
  scenario('gate-signature-invalid', 'A signature that does not verify.', [gate(cmsArg('rsaCadesTampered'), 'cades-b')]),
  scenario('gate-exact-size', 'A CMS exactly as large as the reserved /Contents fits.', [
    gate(cmsArg('rsaCades'), 'cades-b', { contentsSize: bytesOf(fixtures.cms.rsaCades).length }),
  ]),
  scenario('gate-fractional-size', 'A reserved size that is not whole prints as JavaScript prints it.', [
    gate(cmsArg('rsaCades'), 'cades-b', { contentsSize: 100.5 }),
  ]),
  scenario('gate-signature-unsupported', 'An unsupported algorithm is refused with its own detail.', [gate(cmsArg('unknownCurve'), 'cades-b')]),
];

// ── trust ────────────────────────────────────────────────────────────────────

const anchors = (...names: string[]) => ({ anchors: { $fn: 'trust.anchors', resolves: names.map(certArg) } });
const chain = (cms: string, trust: unknown, at?: unknown) =>
  at === undefined ? call('validateChain', cmsArg(cms), trust) : call('validateChain', cmsArg(cms), trust, at);
const trustCases: ScenarioCase[] = [
  scenario('trust-no-port', 'No trust port is `unknown`: nothing was checked.', [chain('rsaCades', null)]),
  scenario('trust-no-anchors', 'A port with no anchors is `unknown` too.', [chain('rsaCades', anchors())]),
  scenario('trust-anchors-reject', 'A port whose anchors() rejects rejects the validation.', [
    chain('rsaCades', { anchors: { $fn: 'trust.anchors', rejects: { message: 'trust list unavailable' } } }),
  ]),
  scenario('trust-bad-anchor', 'An anchor that is not a certificate rejects the validation.', [
    chain('rsaCades', { anchors: { $fn: 'trust.anchors', resolves: [$b('AQID')] } }),
  ]),
  scenario('trust-unparseable-cms', 'A CMS that does not parse is `untrusted` with the parser\'s message.', [chain('twoSigners', anchors('root'))]),
  scenario('trust-root', 'The chain engine starts from the LAST certificate the CMS carries (the intermediate here) and reaches the root.', [
    chain('rsaCades', anchors('root')),
  ]),
  scenario('trust-root-with-root-carried', 'A carried copy of the anchor is dropped before the path is built.', [chain('withRoot', anchors('root'))]),
  scenario('trust-intermediate-last', 'Carried order decides the start: [leaf, root, inter] starts from the intermediate.', [
    chain('interFirst', anchors('root')),
  ]),
  scenario('trust-expired-leaf-not-checked', 'An expired signer followed by a valid intermediate is trusted: the signer is never on the path.', [
    chain('leafExpired', anchors('root')),
  ]),
  scenario('trust-leaf-only-root', 'A lone signer whose issuer is neither carried nor trusted has no path.', [chain('leafOnly', anchors('root'))]),
  scenario('trust-leaf-only-intermediate', 'A lone signer trusted through its issuer.', [chain('leafOnly', anchors('inter'))]),
  scenario('trust-intermediate-anchor-carried', 'A carried certificate that is itself an anchor is a path of two copies.', [
    chain('rsaCades', anchors('root', 'inter')),
  ]),
  scenario('trust-self-signed-anchor', 'A self-signed signer trusted directly.', [chain('selfSigned', anchors('selfSigned'))]),
  scenario('trust-self-signed-other', 'A self-signed signer with an unrelated anchor.', [chain('selfSigned', anchors('root'))]),
  scenario('trust-other-root', 'An unrelated root is no path.', [chain('rsaCades', anchors('otherRoot'))]),
  scenario('trust-by-issuer-name', 'A signer without an authority key identifier finds its issuer by name.', [chain('noAkiLeaf', anchors('inter'))]),
  scenario('trust-expired-intermediate', 'An expired certificate on the path.', [chain('interExpired', anchors('root'))]),
  scenario('trust-expired-signer', 'An expired lone signer.', [chain('expiredLeaf', anchors('inter'))]),
  scenario('trust-not-yet-valid', 'A validation time before every certificate\'s notBefore.', [
    chain('rsaCades', anchors('root'), { kind: 'timestamp', genTime: Date.UTC(2005, 0, 1), tokenVerified: true }),
  ]),
  scenario('trust-at-timestamp', 'A validation time inside the validity window.', [
    chain('rsaCades', anchors('root'), { kind: 'timestamp', genTime: Date.UTC(2030, 5, 1), tokenVerified: true }),
  ]),
  scenario('trust-at-now', 'An explicit `now` is the clock.', [chain('rsaCades', anchors('root'), { kind: 'now' })]),
  scenario('trust-ca-without-basic-constraints', 'An issuer with keyCertSign but no BasicConstraints.', [
    chain('underNoBasicLeaf', anchors('interNoBasic')),
  ]),
  scenario('trust-ca-without-cert-sign', 'A CA whose key usage lacks keyCertSign.', [chain('underNoCertSignLeaf', anchors('interNoCertSign'))]),
  scenario('trust-ca-unknown-critical', 'A CA with a critical extension pkijs cannot parse.', [chain('underUnknownLeaf', anchors('interUnknownCritical'))]),
  scenario('trust-issuer-not-ca', 'An issuer that says it is not a CA.', [chain('underNotCaLeaf', anchors('interNotCa'))]),
  scenario('trust-wrong-issuer-name', 'Found by key identifier, refused by name chaining.', [chain('wrongIssuerNameLeaf', anchors('inter'))]),
  scenario('trust-pss-certificate', 'A certificate signed with RSASSA-PSS (SHA-384) verifies against its issuer.', [chain('pssCertLeaf', anchors('inter'))]),
  scenario('trust-ec-leaf', 'An EC signer issued by an RSA intermediate.', [chain('ecP256Leaf', anchors('inter'))]),
  scenario('trust-ec-self-signed', 'An EC self-signed signer with an unrelated anchor: its own ECDSA signature is checked first.', [
    chain('ecSelfSigned', anchors('root')),
  ]),
  scenario('trust-ec-self-signed-anchor', 'An EC self-signed signer trusted directly.', [chain('ecSelfSigned', anchors('selfSignedP256'))]),
];

// The chain engine's policy and name-constraint half: each CMS carries
// [signer, CA, sub-CA] under the root, so the path is sub-CA, CA, root.
for (const name of Object.keys(fixtures.cms).filter((n) => n.startsWith('policy-'))) {
  trustCases.push(
    scenario(`trust-${name}`, `Certificate policies and name constraints: the ${name.slice(7)} chain.`, [chain(name, anchors('root'))]),
  );
}

// The chain engine's rules one at a time, and its policy half on paths the loop above does not build.
const genTime = (ms: number) => ({ kind: 'timestamp', genTime: ms, tokenVerified: true });
trustCases.push(
  scenario('trust-at-not-before', 'A validation time exactly at notBefore is inside the window.', [
    chain('rsaCades', anchors('root'), genTime(Date.UTC(2020, 0, 1))),
  ]),
  scenario('trust-at-not-after', 'A validation time exactly at notAfter is inside the window.', [
    chain('rsaCades', anchors('root'), genTime(Date.UTC(2045, 0, 1))),
  ]),
  scenario('trust-tampered-signer', 'A signer whose certificate signature does not verify has no issuer.', [chain('leafTampered', anchors('inter'))]),
  scenario('trust-self-signed-twin', 'A valid self-signature makes the certificate its only issuer, even when an anchor holds its key and name.', [
    chain('selfSigned', anchors('selfSignedTwin')),
  ]),
  scenario('trust-self-named-other-key', 'A certificate named as its own issuer whose self-signature fails is issued by the anchor of that name.', [
    chain('twinCaLeaf', anchors('twinCaAnchor')),
  ]),
  scenario('trust-empty-aki', 'An empty authority key identifier finds the issuer by name.', [chain('leafEmptyAki', anchors('inter'))]),
  scenario('trust-aki-issuer-only', 'An authority key identifier naming only the issuer matches nothing.', [chain('leafAkiIssuerOnly', anchors('inter'))]),
  scenario('trust-ca-critical-not-ber', 'A CA whose critical extension does not parse is not a CA.', [
    chain('underUnknownLeaf', anchors('interCriticalNotBer')),
  ]),
  scenario('trust-sha1-certificate', 'A certificate signed with sha1WithRSAEncryption verifies against its issuer.', [chain('leafSha1Signed', anchors('inter'))]),
  scenario('trust-sha384-certificate', 'A certificate signed with sha384WithRSAEncryption.', [chain('leafSha384Signed', anchors('inter'))]),
  scenario('trust-sha512-certificate', 'A certificate signed with sha512WithRSAEncryption.', [chain('leafSha512Signed', anchors('inter'))]),
  scenario('trust-ec-issuer', 'An RSA signer issued by an EC CA: the issuer key\'s curve comes from its parameters.', [
    chain('leafUnderEc', anchors('selfSignedP256')),
  ]),
  scenario('trust-ca-path-length-only', 'BasicConstraints with a path length and no cA is no CA.', [
    chain('underNoBasicLeaf', anchors('interPathLengthOnly')),
  ]),
  scenario('trust-two-anchor-issuers', 'Two anchors issue the signer: the first shortest path wins.', [chain('leafOnly', anchors('inter', 'interTwin'))]),
  scenario('trust-shorter-path', 'An anchor issuer and a carried issuer: the shorter path wins.', [
    chain('leafLastCarried', anchors('root', 'interSelfSigned')),
  ]),
  scenario('trust-policy-two-certificates', 'A two-certificate path still runs the policy checks.', [chain('leafInhibitsOwnMapping', anchors('inter'))]),
  scenario('trust-anchor-constraints-ignored', 'The anchor\'s own name constraints are not applied.', [chain('leafOnly', anchors('interExcludingLeaf'))]),
);

// ── sign ─────────────────────────────────────────────────────────────────────

const signDoc = (responses: ResponseSpec[], services = ['signatures']): Record<string, FakeDocumentSpec> => ({
  d: { pages: [], services, responses },
});
const completed = { status: 'completed', signature: { index: 0, fieldName: 'Sig1' }, version: { sha256: 'new', byteLength: 10 } };
const field = { kind: 'name', name: 'Sig1' };
const rsaAnswer = async (profile: 'pkcs7' | 'cades-b', signingTime?: number, digest = digestBytes(32, 3), hash: RawSigner['hash'] = 'sha256') => ({
  resolves: $b(await signatureFor({ key: 'leaf', chain: leafChain, algorithm: 'RSA-PKCS1-v1_5', hash, profile, digest, signingTime })),
});
const signCall = (input: Record<string, unknown>) => call('sign', 'd', { field, ...input });
const cmsSigner = (answer: unknown) => ({ kind: 'cms', sign: { $fn: 'cms.sign', ...(answer as object) } });

const signCases: ScenarioCase[] = [
  scenario('sign-no-service', 'An engine without signatures refuses at once.', [
    signCall({ signer: cmsSigner({ resolves: cmsArg('rsaCades') }) }),
  ], signDoc([], [])),
  scenario('sign-raw-cades', 'prepare with CAdES and the signer\'s hash, build, gate, complete.', [
    signCall({ signer: rawSignerArg(leafChain, 'RSA-PKCS1-v1_5', 'sha256', await rsaAnswer('cades-b')) }),
  ], signDoc([
    { path: 'signatures.prepare', value: prepared() },
    { path: 'signatures.complete', value: completed },
  ])),
  scenario('sign-raw-pkcs7', 'The PKCS#7 sub-filter builds with signing-time from the clock.', [
    signCall({
      subFilter: 'adbe.pkcs7.detached',
      signer: rawSignerArg(leafChain, 'RSA-PKCS1-v1_5', 'sha256', await rsaAnswer('pkcs7', JAN_2026)),
    }),
  ], signDoc([
    { path: 'signatures.prepare', value: prepared({ subFilter: 'adbe.pkcs7.detached' }) },
    { path: 'signatures.complete', value: completed },
  ])),
  scenario('sign-raw-sha384', 'A raw signer\'s hash is the prepared digest when none is asked for.', [
    signCall({
      signer: rawSignerArg(leafChain, 'RSA-PKCS1-v1_5', 'sha384', await rsaAnswer('cades-b', undefined, digestBytes(48, 3), 'sha384')),
    }),
  ], signDoc([
    { path: 'signatures.prepare', value: prepared({ algorithm: 'sha384', digest: $b(b64(digestBytes(48, 3))) }) },
    { path: 'signatures.complete', value: completed },
  ])),
  scenario('sign-cms-signer', 'A CMS signer gets the digest, algorithm and sub-filter; sha256 unless asked.', [
    signCall({ contentsSize: 16384, signer: cmsSigner({ resolves: cmsArg('rsaCades') }) }),
  ], signDoc([
    { path: 'signatures.prepare', value: prepared() },
    { path: 'signatures.complete', value: completed },
  ])),
  scenario('sign-explicit-digest', 'An explicit digest is passed to prepare as is.', [
    signCall({ digest: 'sha512', signer: cmsSigner({ resolves: cmsArg('rsaCades') }) }),
  ], signDoc([
    { path: 'signatures.prepare', value: prepared() },
    { path: 'signatures.complete', value: completed },
  ])),
  scenario('sign-gate-refuses', 'A CMS the gate refuses aborts the signing and rejects with SigningError.', [
    signCall({ signer: cmsSigner({ resolves: cmsArg('rsaSha384') }) }),
  ], signDoc([
    { path: 'signatures.prepare', value: prepared() },
    { path: 'signatures.abort', value: { status: 'aborted' } },
  ])),
  scenario('sign-signer-rejects', 'A signer failure aborts and rethrows.', [
    signCall({ signer: cmsSigner({ rejects: { name: 'NetworkError', message: 'signing service down' } }) }),
  ], signDoc([
    { path: 'signatures.prepare', value: prepared() },
    { path: 'signatures.abort', value: { status: 'aborted' } },
  ])),
  scenario('sign-complete-rejects', 'A failed complete aborts and rethrows the engine error.', [
    signCall({ signer: cmsSigner({ resolves: cmsArg('rsaCades') }) }),
  ], signDoc([
    { path: 'signatures.prepare', value: prepared() },
    { path: 'signatures.complete', error: { name: 'EngineError', message: 'stale base', code: 'StaleBase' } },
    { path: 'signatures.abort', value: { status: 'already-completed' } },
  ])),
  scenario('sign-abort-rejects', 'An abort that fails is swallowed; the original error wins.', [
    signCall({ signer: cmsSigner({ resolves: cmsArg('twoSigners') }) }),
  ], signDoc([
    { path: 'signatures.prepare', value: prepared() },
    { path: 'signatures.abort', error: { message: 'gone' } },
  ])),
  scenario('sign-prepare-rejects', 'A failed prepare rejects without an abort.', [
    signCall({ signer: cmsSigner({ resolves: cmsArg('rsaCades') }) }),
  ], signDoc([{ path: 'signatures.prepare', error: { name: 'EngineError', message: 'field is signed', code: 'Conflict' } }])),
  scenario('sign-raw-sha1-prepared', 'A raw signer over a document prepared with SHA-1 is refused by the build.', [
    signCall({ signer: rawSignerArg(leafChain, 'RSA-PKCS1-v1_5', 'sha256', { resolves: $b('') }) }),
  ], signDoc([
    { path: 'signatures.prepare', value: prepared({ algorithm: 'sha1' }) },
    { path: 'signatures.abort', value: { status: 'aborted' } },
  ])),
  scenario('sign-build-throws', 'A raw signer whose build throws aborts.', [
    signCall({ signer: rawSignerArg([], 'RSA-PKCS1-v1_5', 'sha256', { resolves: $b('') }) }),
  ], signDoc([
    { path: 'signatures.prepare', value: prepared() },
    { path: 'signatures.abort', value: { status: 'aborted' } },
  ])),
];

// ── validateSignatures ───────────────────────────────────────────────────────

const sigDto = (over: Record<string, unknown> = {}) => ({
  index: 0,
  field,
  fieldName: 'Sig1',
  widget: null,
  signed: true,
  kind: 'signature',
  filter: 'Adobe.PPKLite',
  subFilter: 'ETSI.CAdES.detached',
  byteRange: [0, 100, 8292, 50],
  contentsSize: 8192,
  coverage: 'whole-revision',
  revisionIndex: 0,
  signer: { name: 'Conformance Signer' },
  docMdp: null,
  catalogCertification: false,
  fieldMdp: null,
  lock: null,
  seedValue: null,
  ...over,
});
const snapshot = (signatures: unknown[], over: Record<string, unknown> = {}) => ({
  chainValid: true,
  revisions: [{ index: 0 }],
  signatures,
  protection: {},
  ...over,
});
const analysis = (current: Record<string, unknown>, over: Record<string, unknown> = {}) => ({
  current,
  basis: { source: 'persisted' },
  later: { revisionCount: 0, undoneObjectNumbers: [] },
  ...over,
});
const goodDigest = $b(b64(digestBytes(32, 3)));
const validate = (options?: unknown) => (options === undefined ? call('validateSignatures', 'd') : call('validateSignatures', 'd', options));
const contentsOf = (cms: string): ResponseSpec => ({ path: 'signatures.contents', value: cmsArg(cms) });
const digestAnswer: ResponseSpec = { path: 'signatures.digest', value: goodDigest };

const validateCases: ScenarioCase[] = [
  scenario('validate-no-service', 'An engine without signatures refuses.', [validate()], signDoc([], [])),
  scenario('validate-empty', 'No signatures, no verdicts.', [validate()], signDoc([{ path: 'signatures.list', value: snapshot([]) }])),
  scenario('validate-valid-untrusted', 'Intact, verifying, no trust port, sealing the last revision: `valid-untrusted`, unchanged.', [validate()], signDoc([
    { path: 'signatures.list', value: snapshot([sigDto(), sigDto({ index: 1, signed: false, fieldName: 'Empty' })]) },
    contentsOf('rsaCades'),
    digestAnswer,
  ])),
  scenario('validate-trusted', 'With an anchor that validates: `valid`.', [validate({ trust: anchors('root') })], signDoc([
    { path: 'signatures.list', value: snapshot([sigDto()]) },
    contentsOf('rsaCades'),
    digestAnswer,
  ])),
  scenario('validate-untrusted-reason', 'A chain that fails carries its reason.', [validate({ trust: anchors('otherRoot') })], signDoc([
    { path: 'signatures.list', value: snapshot([sigDto()]) },
    contentsOf('rsaCades'),
    digestAnswer,
  ])),
  scenario('validate-digest-mismatch', 'Bytes that do not hash to the message digest: integrity invalid.', [validate()], signDoc([
    { path: 'signatures.list', value: snapshot([sigDto()]) },
    contentsOf('rsaCades'),
    { path: 'signatures.digest', value: $b(b64(digestBytes(32, 9))) },
  ])),
  scenario('validate-partial-coverage', 'Partial coverage is invalid even when the digest matches.', [validate()], signDoc([
    { path: 'signatures.list', value: snapshot([sigDto({ coverage: 'partial' })]) },
    contentsOf('rsaCades'),
    digestAnswer,
  ])),
  scenario('validate-chain-invalid', 'A broken revision chain makes integrity indeterminate without reading the CMS.', [validate()], signDoc([
    { path: 'signatures.list', value: snapshot([sigDto()], { chainValid: false }) },
  ])),
  scenario('validate-coverage-malformed', 'Malformed coverage on a valid chain is invalid.', [validate()], signDoc([
    { path: 'signatures.list', value: snapshot([sigDto({ coverage: 'malformed' })]) },
  ])),
  scenario('validate-unsupported-cms', 'A CMS this engine cannot read is indeterminate, not tampering.', [validate()], signDoc([
    { path: 'signatures.list', value: snapshot([sigDto()]) },
    contentsOf('twoSigners'),
  ])),
  scenario('validate-malformed-cms', 'A malformed CMS is invalid.', [validate()], signDoc([
    { path: 'signatures.list', value: snapshot([sigDto()]) },
    contentsOf('noCertificates'),
  ])),
  scenario('validate-contents-rejects', 'An engine failure reading the CMS is indeterminate.', [validate()], signDoc([
    { path: 'signatures.list', value: snapshot([sigDto()]) },
    { path: 'signatures.contents', error: { name: 'EngineError', message: 'io', code: 'Io' } },
  ])),
  scenario('validate-crypto-invalid', 'A signature that does not verify is invalid.', [validate()], signDoc([
    { path: 'signatures.list', value: snapshot([sigDto()]) },
    contentsOf('rsaCadesTampered'),
    digestAnswer,
  ])),
  scenario('validate-crypto-unsupported', 'An unsupported algorithm is indeterminate.', [validate()], signDoc([
    { path: 'signatures.list', value: snapshot([sigDto()]) },
    contentsOf('unknownCurve'),
    digestAnswer,
  ])),
  scenario('validate-no-revision', 'A signature that seals no whole revision has indeterminate modifications.', [validate()], signDoc([
    { path: 'signatures.list', value: snapshot([sigDto({ revisionIndex: null })]) },
    contentsOf('rsaCades'),
    digestAnswer,
  ])),
  scenario('validate-forbidden', 'A forbidden change after the signature: invalid, with the object and rule.', [validate()], signDoc([
    { path: 'signatures.list', value: snapshot([sigDto()], { revisions: [{ index: 0 }, { index: 1 }] }) },
    contentsOf('rsaCades'),
    digestAnswer,
    {
      path: 'signatures.analyze',
      value: analysis(
        { verdict: 'forbidden', primary: { verdict: 'forbidden', objectNumber: 12, rule: 'page-content', detail: 'content stream replaced' } },
        { later: { revisionCount: 1, undoneObjectNumbers: [] } },
      ),
    },
  ])),
  scenario('validate-forbidden-no-detail', 'A forbidden primary without detail.', [validate()], signDoc([
    { path: 'signatures.list', value: snapshot([sigDto()], { revisions: [{ index: 0 }, { index: 1 }] }) },
    contentsOf('rsaCades'),
    digestAnswer,
    { path: 'signatures.analyze', value: analysis({ verdict: 'forbidden', primary: { verdict: 'forbidden', objectNumber: 3, rule: 'catalog' } }) },
  ])),
  scenario('validate-incomplete', 'An incomplete primary: indeterminate, with its detail.', [validate()], signDoc([
    { path: 'signatures.list', value: snapshot([sigDto()], { revisions: [{ index: 0 }, { index: 1 }] }) },
    contentsOf('rsaCades'),
    digestAnswer,
    { path: 'signatures.analyze', value: analysis({ verdict: 'indeterminate', primary: { verdict: 'incomplete', objectNumber: 7, detail: 'stream too large' } }) },
  ])),
  scenario('validate-incomplete-no-detail', 'An incomplete primary without detail says so.', [validate()], signDoc([
    { path: 'signatures.list', value: snapshot([sigDto()], { revisions: [{ index: 0 }, { index: 1 }] }) },
    contentsOf('rsaCades'),
    digestAnswer,
    { path: 'signatures.analyze', value: analysis({ verdict: 'indeterminate', primary: { verdict: 'incomplete', objectNumber: 7 } }) },
  ])),
  scenario('validate-permitted-undone', 'Permitted later changes, some undone; the working copy was asked for.', [
    validate({ until: 'working-copy', at: { kind: 'timestamp', genTime: Date.UTC(2030, 5, 1), tokenVerified: true } }),
  ], signDoc([
    { path: 'signatures.list', value: snapshot([sigDto()]) },
    contentsOf('rsaCades'),
    digestAnswer,
    {
      path: 'signatures.analyze',
      value: analysis(
        { verdict: 'permitted', primary: { verdict: 'permitted', objectNumber: 4 } },
        { basis: { source: 'working-copy' }, later: { revisionCount: 2, undoneObjectNumbers: [4] } },
      ),
    },
  ])),
  scenario('validate-analyze-rejects', 'A failed analysis is indeterminate with its message.', [validate()], signDoc([
    { path: 'signatures.list', value: snapshot([sigDto()], { revisions: [{ index: 0 }, { index: 1 }] }) },
    contentsOf('rsaCades'),
    digestAnswer,
    { path: 'signatures.analyze', error: { message: 'revision 1 unreadable' } },
  ])),
];

// ── signers ──────────────────────────────────────────────────────────────────

const rsaKey = (hash = 'SHA-256', name = 'RSASSA-PKCS1-v1_5'): KeySpec => ({ fixture: 'leaf', algorithm: { name, hash } });
const toSign = $b(b64(new TextEncoder().encode('signed attributes')));
const signerCases: ScenarioCase[] = [
  scenario('remote-signer', 'remoteSigner forwards the request to the port and answers with its CMS.', [
    call('remoteSign', { sign: { $fn: 'remote.sign', resolves: cmsArg('rsaCades') } }, {
      digest: goodDigest, algorithm: 'sha256', subFilter: 'ETSI.CAdES.detached',
    }),
  ]),
  scenario('memory-key-store', 'The memory store loads null, then what was saved, until it is removed.', [
    call('memoryKeyStore', [
      { op: 'load', subject: 'Ada' },
      { op: 'save', subject: 'Ada', record: { certificate: certArg('selfSigned') } },
      { op: 'load', subject: 'Ada' },
      { op: 'load', subject: 'Grace' },
      { op: 'remove', subject: 'Ada' },
      { op: 'load', subject: 'Ada' },
    ]),
  ]),
  ...(['SHA-256', 'SHA-384', 'SHA-512'] as const).map((hash) =>
    scenario(`webcrypto-rsa-${hash}`, `An RSASSA-PKCS1-v1_5 key's own hash (${hash}) decides the signer's hash.`, [
      call('webCryptoSigner', rsaKey(hash), [certArg('leaf')], 'sha512', toSign),
    ]),
  ),
  scenario('webcrypto-pss', 'An RSA-PSS key signs with the salt length of its hash.', [
    call('webCryptoSigner', rsaKey('SHA-384', 'RSA-PSS'), [certArg('leaf')], undefined, toSign),
  ]),
  scenario('webcrypto-ecdsa-default-hash', 'An ECDSA key carries no hash: sha256 unless given.', [
    call('webCryptoSigner', { fixture: 'p256', algorithm: { name: 'ECDSA', namedCurve: 'P-256' } }, [certArg('leafP256')], undefined, toSign),
  ]),
  scenario('webcrypto-ecdsa-hash', 'An ECDSA signer with an explicit hash.', [
    call('webCryptoSigner', { fixture: 'p384', algorithm: { name: 'ECDSA', namedCurve: 'P-384' } }, [certArg('leafP384')], 'sha384', toSign),
  ]),
  scenario('webcrypto-unsupported-algorithm', 'A key WebCrypto signs with but this package does not.', [
    call('webCryptoSigner', { fake: true, algorithm: { name: 'Ed25519' } }, [certArg('leaf')]),
  ]),
  scenario('webcrypto-unsupported-hash', 'An RSA key hashing with SHA-1.', [
    call('webCryptoSigner', { fake: true, algorithm: { name: 'RSASSA-PKCS1-v1_5', hash: { name: 'SHA-1' } } as never }, [certArg('leaf')]),
  ]),
  ...(
    [
      ['RSA-PKCS1-v1_5', 'sha256', true],
      ['RSA-PSS', 'sha384', false],
      ['ECDSA', 'sha512', false],
    ] as const
  ).map(([algorithm, hash, extractable]) =>
    scenario(`generate-${algorithm}`, `generateSigningKeyPair's WebCrypto parameters for ${algorithm}.`, [
      call('generateSigningKeyPair', algorithm, hash, extractable),
    ]),
  ),
  scenario('self-signed-rsa', 'A self-signed certificate over an RSA key: serial from getRandomValues, a day back to a year ahead.', [
    call('selfSignedCertificate', { key: rsaKey(), commonName: 'Ada Lovelace', hash: 'sha256' }),
  ]),
  scenario('self-signed-rsa-sha512-days', 'The key\'s own hash wins over the one passed; a custom validity.', [
    call('selfSignedCertificate', { key: rsaKey('SHA-512'), commonName: 'Grace Hopper', hash: 'sha256', validityDays: 30 }),
  ]),
  scenario('self-signed-rsa-sha1', 'An RSA key hashing with SHA-1 signs its certificate with sha1WithRSAEncryption.', [
    call('selfSignedCertificate', { key: rsaKey('SHA-1'), commonName: 'Sha One', hash: 'sha256' }),
  ]),
  scenario('self-signed-validity-past-2100', 'A validity that ends after 2099 writes notAfter without its year, as asn1js does.', [
    call('selfSignedCertificate', { key: rsaKey(), commonName: 'Far Future', hash: 'sha256', validityDays: 30000 }),
  ]),
  scenario('self-signed-rsa-3072', 'A 3072-bit key: its 384-byte modulus survives the power-of-two rounding of the JWK import.', [
    call('selfSignedCertificate', { key: { ...rsaKey(), fixture: 'rsa3072' }, commonName: 'Long Key', hash: 'sha256' }),
  ]),
  ...(['P-384', 'P-521'] as const).map((namedCurve) =>
    scenario(`self-signed-ecdsa-${namedCurve}`, `A self-signed certificate over a ${namedCurve} key.`, [
      call('selfSignedCertificate', { key: { algorithm: { name: 'ECDSA', namedCurve } }, commonName: 'EC Person', hash: 'sha384' }),
    ]),
  ),
  scenario('self-signed-pss', 'A self-signed certificate over an RSA-PSS key.', [
    call('selfSignedCertificate', { key: rsaKey('SHA-384', 'RSA-PSS'), commonName: 'PSS Person', hash: 'sha384' }),
  ]),
  scenario('self-signed-ecdsa', 'A self-signed certificate over an ECDSA key, signed with the hash passed.', [
    call('selfSignedCertificate', { key: { algorithm: { name: 'ECDSA', namedCurve: 'P-256' } }, commonName: 'EC Person', hash: 'sha384' }),
  ]),
  scenario('test-signer-default', 'createTestSigner: an extractable RSA key pair, a self-signed certificate, a signer over it.', [
    call('createTestSigner', undefined, toSign),
  ]),
  scenario('test-signer-ecdsa', 'createTestSigner with ECDSA and a name.', [
    call('createTestSigner', { algorithm: 'ECDSA', hash: 'sha384', commonName: 'EC Tester' }),
  ]),
  scenario('test-signer-pss', 'createTestSigner with RSA-PSS.', [call('createTestSigner', { algorithm: 'RSA-PSS', hash: 'sha512' })]),
  scenario('personal-signer-memory', 'personalSigner creates the identity on first use and reuses it on the second.', [
    call('personalSigner', { subject: 'Ada Lovelace' }, 2),
  ]),
  scenario('personal-signer-store-port', 'personalSigner asks the store, generates a non-extractable key, and saves the record.', [
    call('personalSigner', {
      subject: 'Grace Hopper',
      algorithm: 'ECDSA',
      hash: 'sha384',
      store: {
        load: { $fn: 'store.load', resolves: null },
        save: { $fn: 'store.save', resolves: { $undefined: true } },
        remove: { $fn: 'store.remove', resolves: { $undefined: true } },
      },
    }),
  ]),
  scenario('personal-signer-store-fails', 'A store that cannot save rejects.', [
    call('personalSigner', {
      subject: 'Grace Hopper',
      store: {
        load: { $fn: 'store.load', resolves: null },
        save: { $fn: 'store.save', rejects: { name: 'QuotaExceededError', message: 'full' } },
        remove: { $fn: 'store.remove', resolves: { $undefined: true } },
      },
    }),
  ]),
];

export const suites: Suite[] = [
  {
    kind: 'function',
    file: 'core/signature-parse.json',
    describes: 'packages/core/signature: parseDetachedCms over built, altered and third-party CMS; profileFor; certificateCommonName.',
    functions: parseFns,
    cases: parseCases,
  },
  {
    kind: 'scenario',
    file: 'core/signature-verify.json',
    describes: 'packages/core/signature: verifyCmsSignature, buildDetachedCms and the completion gate.',
    cases: [...verifyCases, ...buildCases, ...gateCases],
  },
  {
    kind: 'scenario',
    file: 'core/signature-trust.json',
    describes: 'packages/core/signature: validateChain over pkijs\'s chain engine.',
    cases: trustCases,
  },
  {
    kind: 'scenario',
    file: 'core/signature-document.json',
    describes: 'packages/core/signature: sign() and validateSignatures() over a document handle\'s signatures service.',
    cases: [...signCases, ...validateCases],
  },
  {
    kind: 'scenario',
    file: 'core/signature-signers.json',
    describes: 'packages/core/signature: the signer ports and WebCrypto adapters, against a scripted WebCrypto.',
    cases: signerCases,
  },
];
