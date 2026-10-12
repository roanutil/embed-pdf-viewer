/**
 * Writes `fixtures.json`: the keys, certificates, signatures and CMS blobs the
 * `core/signature` vectors are made from.
 *
 *   npx tsx --conditions=development fixtures/signature/generate.ts
 *
 * Key generation, ECDSA and RSA-PSS signing are random, so this runs once and
 * its output is committed. The vectors only read the file, which keeps
 * `vectors:check` deterministic. Re-running it with `fixtures.json` present
 * extends the file instead: the keys are read back, every certificate, CMS
 * blob and recorded signature the file already holds is written back as it
 * was, and only names it does not hold yet are added. Delete the file to
 * start over; every key changes then, and every vector that embeds one.
 *
 * Two kinds of signature are recorded under `signatures`, keyed by the
 * SHA-256 of the bytes signed: the ones this script made for the build cases
 * in `src/suites/signature-core.ts` (the scripted raw signer answers from
 * this table), and none else. Certificates are built with pkijs, the same
 * library `packages/core/signature` parses them with.
 */
import { constants, createHash, createPrivateKey, sign as nodeSign, webcrypto } from 'node:crypto';
import { existsSync } from 'node:fs';
import { readFile, writeFile } from 'node:fs/promises';
import { dirname, join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import { createRequire } from 'node:module';
import { buildDetachedCms, type RawSigner } from '../../../../packages/core/signature/src/index';
import { ensureEngine } from '../../../../packages/core/signature/src/cms/engine';

const here = dirname(fileURLToPath(import.meta.url));
const repo = resolve(here, '../../../..');
// The same module instances `packages/core/signature` loads (pkijs keeps its
// crypto engine in module state).
const requireFromSignature = createRequire(join(repo, 'packages/core/signature/package.json'));
const asn1js = requireFromSignature('asn1js') as typeof import('asn1js');
const pkijs = requireFromSignature('pkijs') as typeof import('pkijs');
const subtle = webcrypto.subtle as unknown as SubtleCrypto;
ensureEngine();

const b64 = (bytes: ArrayBuffer | Uint8Array) =>
  Buffer.from(bytes instanceof Uint8Array ? bytes : new Uint8Array(bytes)).toString('base64');
const sha256hex = (bytes: Uint8Array) => createHash('sha256').update(bytes).digest('hex');
const ab = (bytes: Uint8Array) => bytes.slice().buffer;

// ── keys ─────────────────────────────────────────────────────────────────────

type KeyKind = 'rsa' | 'p256' | 'p384' | 'p521';
interface KeyPair {
  kind: KeyKind;
  pkcs8: Uint8Array;
  spki: Uint8Array;
}

async function generate(kind: KeyKind, modulusLength = 2048): Promise<KeyPair> {
  const pair = (await subtle.generateKey(
    kind === 'rsa'
      ? { name: 'RSASSA-PKCS1-v1_5', modulusLength, publicExponent: new Uint8Array([1, 0, 1]), hash: 'SHA-256' }
      : { name: 'ECDSA', namedCurve: { p256: 'P-256', p384: 'P-384', p521: 'P-521' }[kind] },
    true,
    ['sign', 'verify'],
  )) as CryptoKeyPair;
  return {
    kind,
    pkcs8: new Uint8Array(await subtle.exportKey('pkcs8', pair.privateKey)),
    spki: new Uint8Array(await subtle.exportKey('spki', pair.publicKey)),
  };
}

async function fromPem(pem: string): Promise<KeyPair> {
  const der = Buffer.from(pem.replace(/-----[^-]+-----/g, '').replace(/\s+/g, ''), 'base64');
  const key = await subtle.importKey('pkcs8', der, { name: 'RSASSA-PKCS1-v1_5', hash: 'SHA-256' }, true, ['sign']);
  const jwk = await subtle.exportKey('jwk', key);
  const pub = await subtle.importKey(
    'jwk',
    { kty: 'RSA', n: jwk.n, e: jwk.e, alg: 'RS256', ext: true },
    { name: 'RSASSA-PKCS1-v1_5', hash: 'SHA-256' },
    true,
    ['verify'],
  );
  return { kind: 'rsa', pkcs8: new Uint8Array(der), spki: new Uint8Array(await subtle.exportKey('spki', pub)) };
}

/** A private key for one WebCrypto algorithm. */
async function privateKey(key: KeyPair, algorithm: 'RSASSA-PKCS1-v1_5' | 'RSA-PSS' | 'ECDSA', hash = 'SHA-256') {
  const params =
    key.kind === 'rsa'
      ? { name: algorithm, hash }
      : { name: 'ECDSA', namedCurve: { p256: 'P-256', p384: 'P-384', p521: 'P-521' }[key.kind as 'p256'] };
  return subtle.importKey('pkcs8', key.pkcs8, params, true, ['sign']);
}

// ── certificates ─────────────────────────────────────────────────────────────

const OID = {
  commonName: '2.5.4.3',
  organization: '2.5.4.10',
  basicConstraints: '2.5.29.19',
  keyUsage: '2.5.29.15',
  subjectKeyIdentifier: '2.5.29.14',
  authorityKeyIdentifier: '2.5.29.35',
};

interface CertSpec {
  subject: string;
  issuer?: { name: string; key: KeyPair; skid?: Uint8Array };
  key: KeyPair;
  serial: number;
  notBefore: string;
  notAfter: string;
  /** `true` / `false` writes BasicConstraints with that cA; absent writes none. */
  ca?: boolean;
  /** Key usage first byte and unused bits; absent writes none. */
  keyUsage?: [number, number];
  ski?: boolean;
  aki?: boolean;
  /** An extension pkijs cannot parse, marked critical. */
  unknownCritical?: boolean;
  sign?: 'RSASSA-PKCS1-v1_5' | 'RSA-PSS' | 'ECDSA';
  hash?: string;
  /** The subject name's attributes, replacing the default O and CN. */
  subjectAttributes?: pkijs.AttributeTypeAndValue[];
  /** Change the public key info after it is set from the key. */
  spki?: (spki: pkijs.PublicKeyInfo) => void;
  /** Further extensions: `[oid, value DER, critical]`. */
  extra?: [string, ArrayBuffer, boolean?][];
}

const skid = (spki: Uint8Array) => new Uint8Array(createHash('sha1').update(spki).digest()).slice(0, 20);

function name(cn: string): pkijs.RelativeDistinguishedNames {
  const rdn = new pkijs.RelativeDistinguishedNames();
  rdn.typesAndValues.push(
    new pkijs.AttributeTypeAndValue({ type: OID.organization, value: new asn1js.PrintableString({ value: 'EmbedPDF Conformance' }) }),
    new pkijs.AttributeTypeAndValue({ type: OID.commonName, value: new asn1js.Utf8String({ value: cn }) }),
  );
  return rdn;
}

async function certificate(spec: CertSpec): Promise<Uint8Array> {
  const cert = new pkijs.Certificate();
  cert.version = 2;
  cert.serialNumber = new asn1js.Integer({ value: spec.serial });
  cert.subject = name(spec.subject);
  if (spec.subjectAttributes) {
    cert.subject = new pkijs.RelativeDistinguishedNames();
    cert.subject.typesAndValues.push(...spec.subjectAttributes);
  }
  cert.issuer = name(spec.issuer?.name ?? spec.subject);
  cert.notBefore.value = new Date(spec.notBefore);
  cert.notAfter.value = new Date(spec.notAfter);
  const extensions: pkijs.Extension[] = [];
  if (spec.ca !== undefined) {
    const bc = new pkijs.BasicConstraints({ cA: spec.ca });
    extensions.push(new pkijs.Extension({ extnID: OID.basicConstraints, critical: true, extnValue: bc.toSchema().toBER(false), parsedValue: bc }));
  }
  if (spec.keyUsage) {
    const bits = new asn1js.BitString({ valueHex: new Uint8Array([spec.keyUsage[0]]).buffer, unusedBits: spec.keyUsage[1] });
    extensions.push(new pkijs.Extension({ extnID: OID.keyUsage, critical: true, extnValue: bits.toBER(false) }));
  }
  if (spec.ski) {
    const value = new asn1js.OctetString({ valueHex: ab(skid(spec.key.spki)) });
    extensions.push(new pkijs.Extension({ extnID: OID.subjectKeyIdentifier, extnValue: value.toBER(false) }));
  }
  if (spec.aki && spec.issuer) {
    const aki = new pkijs.AuthorityKeyIdentifier({
      keyIdentifier: new asn1js.OctetString({ valueHex: ab(skid(spec.issuer.key.spki)) }),
    });
    extensions.push(new pkijs.Extension({ extnID: OID.authorityKeyIdentifier, extnValue: aki.toSchema().toBER(false) }));
  }
  if (spec.unknownCritical) {
    extensions.push(new pkijs.Extension({ extnID: '1.3.6.1.4.1.99999.1', critical: true, extnValue: new asn1js.Null().toBER(false) }));
  }
  for (const [extnID, extnValue, critical] of spec.extra ?? []) {
    extensions.push(new pkijs.Extension({ extnID, critical: critical ?? false, extnValue }));
  }
  if (extensions.length) cert.extensions = extensions;
  cert.subjectPublicKeyInfo.fromSchema(asn1js.fromBER(ab(spec.key.spki)).result);
  spec.spki?.(cert.subjectPublicKeyInfo);
  const signer = spec.issuer?.key ?? spec.key;
  const algorithm = spec.sign ?? (signer.kind === 'rsa' ? 'RSASSA-PKCS1-v1_5' : 'ECDSA');
  await cert.sign(await privateKey(signer, algorithm, spec.hash ?? 'SHA-256'), spec.hash ?? 'SHA-256');
  return new Uint8Array(cert.toSchema(true).toBER(false));
}

// ── raw signers over the keys ────────────────────────────────────────────────

const signatures: Record<string, string> = {};

function rawSigner(
  key: KeyPair,
  chain: Uint8Array[],
  algorithm: RawSigner['algorithm'],
  hash: RawSigner['hash'],
): RawSigner {
  const webHash = { sha256: 'SHA-256', sha384: 'SHA-384', sha512: 'SHA-512' }[hash];
  return {
    kind: 'raw',
    certificateChain: chain,
    algorithm,
    hash,
    async sign(data) {
      const k = await privateKey(key, algorithm === 'RSA-PKCS1-v1_5' ? 'RSASSA-PKCS1-v1_5' : algorithm, webHash);
      const params =
        algorithm === 'RSA-PSS'
          ? { name: 'RSA-PSS', saltLength: { sha256: 32, sha384: 48, sha512: 64 }[hash] }
          : algorithm === 'ECDSA'
            ? { name: 'ECDSA', hash: webHash }
            : { name: 'RSASSA-PKCS1-v1_5' };
      const signature = new Uint8Array(await subtle.sign(params, k, data));
      signatures[sha256hex(data)] = b64(signature);
      return signature;
    },
  };
}

// ── CMS surgery ──────────────────────────────────────────────────────────────

/** Parse a CMS, let `edit` change its SignedData, and write it back. */
function editCms(cms: Uint8Array, edit: (signedData: pkijs.SignedData) => void): Uint8Array {
  const info = new pkijs.ContentInfo({ schema: asn1js.fromBER(ab(cms)).result });
  const signedData = new pkijs.SignedData({ schema: info.content });
  edit(signedData);
  const out = new pkijs.ContentInfo({ contentType: info.contentType, content: signedData.toSchema(true) });
  return new Uint8Array(out.toSchema().toBER(false));
}

async function pdfContents(path: string): Promise<Uint8Array[]> {
  const bytes = await readFile(path);
  const text = bytes.toString('latin1');
  const out: Uint8Array[] = [];
  for (const match of text.matchAll(/\/Contents\s*<([0-9A-Fa-f]+)>/g)) {
    let blob = Buffer.from(match[1], 'hex');
    let end = blob.length;
    while (end > 0 && blob[end - 1] === 0) end -= 1;
    blob = blob.subarray(0, end);
    if (blob.length > 100) out.push(new Uint8Array(blob));
  }
  return out;
}

// ── the fixture set ──────────────────────────────────────────────────────────

const VALID = { notBefore: '2020-01-01T00:00:00Z', notAfter: '2045-01-01T00:00:00Z' };

// The file this run extends, when there is one.
interface Stored {
  keys: Record<string, { kind: KeyKind; pkcs8: string; spki: string }>;
  certs: Record<string, string>;
  cms: Record<string, string>;
  signatures: Record<string, string>;
}
const storedPath = join(here, 'fixtures.json');
const stored = existsSync(storedPath) ? (JSON.parse(await readFile(storedPath, 'utf8')) as Stored) : undefined;
const fromB64 = (text: string) => new Uint8Array(Buffer.from(text, 'base64'));
const storedKey = (name: string): KeyPair | undefined => {
  const key = stored?.keys[name];
  return key && { kind: key.kind, pkcs8: fromB64(key.pkcs8), spki: fromB64(key.spki) };
};

const corpusKey = await fromPem(
  await readFile(join(repo, 'packages/engine/main/test/fixtures/signature-compat/keys/TEST-ONLY-private-key.pem'), 'utf8'),
);
const keys = {
  root: storedKey('root') ?? (await generate('rsa')),
  inter: storedKey('inter') ?? (await generate('rsa')),
  leaf: storedKey('leaf') ?? (await generate('rsa')),
  other: storedKey('other') ?? (await generate('rsa')),
  p256: storedKey('p256') ?? (await generate('p256')),
  p384: storedKey('p384') ?? (await generate('p384')),
  p521: storedKey('p521') ?? (await generate('p521')),
  corpus: storedKey('corpus') ?? corpusKey,
  // A 3072-bit key: its modulus is not a power of two bytes long, which pkijs's JWK import rounds.
  rsa3072: storedKey('rsa3072') ?? (await generate('rsa', 3072)),
  // An 8192-bit key: past the 4096 bits RustCrypto's `rsa` imports by default, within WebCrypto's 16384.
  rsa8192: storedKey('rsa8192') ?? (await generate('rsa', 8192)),
};

const rootName = 'Conformance Root CA';
const interName = 'Conformance Intermediate CA';
const certs: Record<string, Uint8Array> = {};
certs.root = await certificate({ subject: rootName, key: keys.root, serial: 1, ...VALID, ca: true, keyUsage: [0x06, 1], ski: true });
const rootIssuer = { name: rootName, key: keys.root };
certs.inter = await certificate({
  subject: interName, issuer: rootIssuer, key: keys.inter, serial: 2, ...VALID, ca: true, keyUsage: [0x06, 1], ski: true, aki: true,
});
const interIssuer = { name: interName, key: keys.inter };
certs.leaf = await certificate({
  subject: 'Conformance Signer', issuer: interIssuer, key: keys.leaf, serial: 3, ...VALID, keyUsage: [0xc0, 6], ski: true, aki: true,
});
certs.leafNoAki = await certificate({
  subject: 'Conformance Signer Without AKI', issuer: interIssuer, key: keys.leaf, serial: 4, ...VALID, keyUsage: [0x80, 7],
});
certs.leafExpired = await certificate({
  subject: 'Conformance Expired Signer', issuer: interIssuer, key: keys.leaf, serial: 5,
  notBefore: '2000-01-01T00:00:00Z', notAfter: '2010-01-01T00:00:00Z', ski: true, aki: true,
});
certs.selfSigned = await certificate({
  subject: 'Conformance Self-Signed', key: keys.leaf, serial: 6, ...VALID, ca: true, keyUsage: [0x84, 2],
});
certs.otherRoot = await certificate({ subject: 'Unrelated Root CA', key: keys.other, serial: 7, ...VALID, ca: true, keyUsage: [0x06, 1], ski: true });
// Intermediates the chain engine refuses, each issued by the root.
certs.interNoBasic = await certificate({
  subject: 'Conformance CA Without BasicConstraints', issuer: rootIssuer, key: keys.inter, serial: 8, ...VALID, keyUsage: [0x06, 1], ski: true, aki: true,
});
certs.leafUnderNoBasic = await certificate({
  subject: 'Signer Under Bad CA', issuer: { name: 'Conformance CA Without BasicConstraints', key: keys.inter }, key: keys.leaf,
  serial: 9, ...VALID, ski: true, aki: true,
});
certs.interNoCertSign = await certificate({
  subject: 'Conformance CA Without keyCertSign', issuer: rootIssuer, key: keys.inter, serial: 10, ...VALID, ca: true, keyUsage: [0x80, 7], ski: true, aki: true,
});
certs.leafUnderNoCertSign = await certificate({
  subject: 'Signer Under CA Without keyCertSign', issuer: { name: 'Conformance CA Without keyCertSign', key: keys.inter }, key: keys.leaf,
  serial: 11, ...VALID, ski: true, aki: true,
});
certs.interUnknownCritical = await certificate({
  subject: 'Conformance CA With Unknown Extension', issuer: rootIssuer, key: keys.inter, serial: 12, ...VALID, ca: true, keyUsage: [0x06, 1], ski: true, aki: true, unknownCritical: true,
});
certs.leafUnderUnknown = await certificate({
  subject: 'Signer Under CA With Unknown Extension', issuer: { name: 'Conformance CA With Unknown Extension', key: keys.inter }, key: keys.leaf,
  serial: 13, ...VALID, ski: true, aki: true,
});
certs.interNotCa = await certificate({
  subject: 'Conformance Non-CA Intermediate', issuer: rootIssuer, key: keys.inter, serial: 14, ...VALID, ca: false, ski: true, aki: true,
});
certs.leafUnderNotCa = await certificate({
  subject: 'Signer Under Non-CA', issuer: { name: 'Conformance Non-CA Intermediate', key: keys.inter }, key: keys.leaf,
  serial: 15, ...VALID, ski: true, aki: true,
});
certs.interExpired = await certificate({
  subject: interName, issuer: rootIssuer, key: keys.inter, serial: 16,
  notBefore: '2000-01-01T00:00:00Z', notAfter: '2010-01-01T00:00:00Z', ca: true, keyUsage: [0x06, 1], ski: true, aki: true,
});
// Leaves over EC keys, issued by the RSA intermediate; and one PSS-signed certificate.
for (const curve of ['p256', 'p384', 'p521'] as const) {
  certs[`leaf${curve.toUpperCase()}`] = await certificate({
    subject: `Conformance ${curve.toUpperCase()} Signer`, issuer: interIssuer, key: keys[curve], serial: 20 + ['p256', 'p384', 'p521'].indexOf(curve),
    ...VALID, keyUsage: [0x80, 7], ski: true, aki: true,
  });
}
certs.selfSignedP256 = await certificate({ subject: 'Conformance EC Self-Signed', key: keys.p256, serial: 30, ...VALID, ca: true, keyUsage: [0x84, 2] });
certs.leafPssSigned = await certificate({
  subject: 'Conformance Signer With PSS Certificate', issuer: interIssuer, key: keys.leaf, serial: 31, ...VALID, ski: true, aki: true,
  sign: 'RSA-PSS', hash: 'SHA-384',
});
certs.corpus = new Uint8Array(
  Buffer.from(
    (await readFile(join(repo, 'packages/engine/main/test/fixtures/signature-compat/keys/TEST-ONLY-signer.pem'), 'utf8'))
      .replace(/-----[^-]+-----/g, '')
      .replace(/\s+/g, ''),
    'base64',
  ),
);

// Certificates for certificateCommonName, and for verify's refusals.
const org = new pkijs.AttributeTypeAndValue({ type: OID.organization, value: new asn1js.PrintableString({ value: 'EmbedPDF Conformance' }) });
certs.noCommonName = await certificate({ subject: 'x', key: keys.leaf, serial: 40, ...VALID, subjectAttributes: [org] });
certs.emptyCommonName = await certificate({
  subject: 'x', key: keys.leaf, serial: 41, ...VALID,
  subjectAttributes: [org, new pkijs.AttributeTypeAndValue({ type: OID.commonName, value: new asn1js.Utf8String({ value: '' }) })],
});
certs.integerCommonName = await certificate({
  subject: 'x', key: keys.leaf, serial: 42, ...VALID,
  subjectAttributes: [new pkijs.AttributeTypeAndValue({ type: OID.commonName, value: new asn1js.Integer({ value: 42 }) })],
});
certs.bmpCommonName = await certificate({
  subject: 'x', key: keys.leaf, serial: 43, ...VALID,
  subjectAttributes: [new pkijs.AttributeTypeAndValue({ type: OID.commonName, value: new asn1js.BmpString({ value: 'Bmp Name' }) })],
});
certs.leafUnknownCurve = await certificate({
  subject: 'Conformance Unknown Curve Signer', issuer: interIssuer, key: keys.p256, serial: 44, ...VALID, ski: true, aki: true,
  spki: (spki) => {
    spki.algorithm.algorithmParams = new asn1js.ObjectIdentifier({ value: '1.3.132.0.10' });
  },
});
certs.leafEcNoParams = await certificate({
  subject: 'Conformance EC Signer Without Parameters', issuer: interIssuer, key: keys.p256, serial: 45, ...VALID, ski: true, aki: true,
  spki: (spki) => {
    delete (spki.algorithm as { algorithmParams?: unknown }).algorithmParams;
  },
});
certs.leafDsaKey = await certificate({
  subject: 'Conformance DSA-Labelled Signer', issuer: interIssuer, key: keys.leaf, serial: 46, ...VALID, ski: true, aki: true,
  spki: (spki) => {
    spki.algorithm.algorithmId = '1.2.840.10040.4.1';
  },
});
certs.leafWrongIssuerName = await certificate({
  subject: 'Conformance Signer With Wrong Issuer Name', issuer: { name: 'Somebody Else', key: keys.inter }, key: keys.leaf,
  serial: 47, ...VALID, ski: true, aki: true,
});

// Chains for the chain engine's policy and name-constraint half: a CA under
// the root, a sub-CA under it, and a signer under the sub-CA. The CMS carries
// [signer, CA, sub-CA], so the path is sub-CA, CA, root.
const P1 = '1.3.6.1.4.1.99999.2.1';
const P2 = '1.3.6.1.4.1.99999.2.2';
const ANY_POLICY = '2.5.29.32.0';
const seq = (...value: asn1js.BaseBlock[]) => new asn1js.Sequence({ value });
const policiesExt = (...ids: string[]): [string, ArrayBuffer] => [
  '2.5.29.32',
  seq(...ids.map((id) => seq(new asn1js.ObjectIdentifier({ value: id })))).toBER(false),
];
const mappingsExt = (...pairs: [string, string][]): [string, ArrayBuffer] => [
  '2.5.29.33',
  seq(...pairs.map(([a, b]) => seq(new asn1js.ObjectIdentifier({ value: a }), new asn1js.ObjectIdentifier({ value: b })))).toBER(false),
];
const ctxInt = (tag: number, value: number) =>
  new asn1js.Primitive({ idBlock: { tagClass: 3, tagNumber: tag }, valueHex: new asn1js.Integer({ value }).valueBlock.toBER(false) });
const constraintsExt = (require?: number, inhibit?: number): [string, ArrayBuffer] => [
  '2.5.29.36',
  seq(...(require === undefined ? [] : [ctxInt(0, require)]), ...(inhibit === undefined ? [] : [ctxInt(1, inhibit)])).toBER(false),
];
const inhibitAnyExt = (n: number): [string, ArrayBuffer] => ['2.5.29.54', new asn1js.Integer({ value: n }).toBER(false)];
const ia5 = (tag: number, text: string) =>
  new asn1js.Primitive({ idBlock: { tagClass: 3, tagNumber: tag }, valueHex: ab(new TextEncoder().encode(text)) });
const ipName = (bytes: number[]) => new asn1js.Primitive({ idBlock: { tagClass: 3, tagNumber: 7 }, valueHex: ab(new Uint8Array(bytes)) });
const dirName = (cn: string) => new asn1js.Constructed({ idBlock: { tagClass: 3, tagNumber: 4 }, value: [name(cn).toSchema()] });
const subtree = (base: asn1js.BaseBlock) => seq(base);
const nameConstraintsExt = (permitted: asn1js.BaseBlock[], excluded: asn1js.BaseBlock[]): [string, ArrayBuffer, boolean] => [
  '2.5.29.30',
  seq(
    ...(permitted.length ? [new asn1js.Constructed({ idBlock: { tagClass: 3, tagNumber: 0 }, value: permitted.map(subtree) })] : []),
    ...(excluded.length ? [new asn1js.Constructed({ idBlock: { tagClass: 3, tagNumber: 1 }, value: excluded.map(subtree) })] : []),
  ).toBER(false),
  true,
];
const altNamesExt = (...names: asn1js.BaseBlock[]): [string, ArrayBuffer] => ['2.5.29.17', seq(...names).toBER(false)];

interface PolicyChain {
  ca: [string, ArrayBuffer, boolean?][];
  sub: [string, ArrayBuffer, boolean?][];
  subAttributes?: pkijs.AttributeTypeAndValue[];
}
const policyChains: Record<string, PolicyChain> = {
  policiesMatch: { ca: [policiesExt(P1)], sub: [policiesExt(P1)] },
  explicitNoIntersection: { ca: [policiesExt(P1), constraintsExt(0)], sub: [policiesExt(P2)] },
  explicitAnyPolicy: { ca: [policiesExt(ANY_POLICY), constraintsExt(0)], sub: [policiesExt(ANY_POLICY)] },
  explicitShared: { ca: [policiesExt(P1, P2), constraintsExt(0)], sub: [policiesExt(P1, P2)] },
  mappingInhibited: { ca: [constraintsExt(undefined, 0)], sub: [mappingsExt([P1, P2])] },
  mappingAnyPolicy: { ca: [policiesExt(P1), mappingsExt([ANY_POLICY, P1])], sub: [policiesExt(P1)] },
  mappingMapped: { ca: [policiesExt(P1), mappingsExt([P1, P2])], sub: [policiesExt(P2)] },
  mappingUnknownIssuer: { ca: [mappingsExt([P2, P1])], sub: [policiesExt(P1)] },
  mappingNoPolicies: { ca: [policiesExt(P1), mappingsExt([P1, P2])], sub: [] },
  inhibitAny: { ca: [inhibitAnyExt(0), policiesExt(ANY_POLICY)], sub: [policiesExt(P1)] },
  pendingConstraints: { ca: [constraintsExt(1, 1), inhibitAnyExt(1)], sub: [constraintsExt(2, 3), inhibitAnyExt(2), policiesExt(P1)] },
  pendingOnly: { ca: [constraintsExt(2, 2), inhibitAnyExt(2)], sub: [] },
  dnsPermitted: { ca: [nameConstraintsExt([ia5(2, 'example.com')], [])], sub: [altNamesExt(ia5(2, 'ca.example.com'))] },
  dnsRefused: { ca: [nameConstraintsExt([ia5(2, 'example.com')], [])], sub: [altNamesExt(ia5(2, 'ca.evil.org'))] },
  dnsExcluded: { ca: [nameConstraintsExt([], [ia5(2, '.evil.org')])], sub: [altNamesExt(ia5(2, 'ca.evil.org'))] },
  emailPermitted: { ca: [nameConstraintsExt([ia5(1, 'example.com')], [])], sub: [altNamesExt(ia5(1, 'ca@example.com'))] },
  emailExcluded: { ca: [nameConstraintsExt([], [ia5(1, 'ca@evil.org')])], sub: [altNamesExt(ia5(1, 'ca@evil.org'))] },
  emailInSubject: {
    ca: [nameConstraintsExt([ia5(1, '.example.com')], [])],
    sub: [],
    subAttributes: [
      new pkijs.AttributeTypeAndValue({ type: '1.2.840.113549.1.9.1', value: new asn1js.IA5String({ value: 'ca@mail.evil.org' }) }),
      new pkijs.AttributeTypeAndValue({ type: OID.commonName, value: new asn1js.Utf8String({ value: 'Policy Sub-CA' }) }),
    ],
  },
  uriPermitted: { ca: [nameConstraintsExt([ia5(6, '.example.com')], [])], sub: [altNamesExt(ia5(6, 'https://ca.example.com:443/x'))] },
  uriExcluded: { ca: [nameConstraintsExt([], [ia5(6, 'ca.evil.org')])], sub: [altNamesExt(ia5(6, 'http://ca.evil.org/'))] },
  ipPermitted: { ca: [nameConstraintsExt([ipName([10, 0, 0, 0, 255, 0, 0, 0])], [])], sub: [altNamesExt(ipName([10, 1, 2, 3]))] },
  ipExcluded: { ca: [nameConstraintsExt([], [ipName([10, 0, 0, 0, 255, 0, 0, 0])])], sub: [altNamesExt(ipName([10, 1, 2, 3]))] },
  directoryPermitted: { ca: [nameConstraintsExt([dirName('Policy Sub-CA')], [])], sub: [] },
  directoryExcluded: { ca: [nameConstraintsExt([], [dirName('Policy Sub-CA')])], sub: [] },
  directoryRefused: { ca: [nameConstraintsExt([dirName('Somebody Else')], [])], sub: [] },
};
let policySerial = 100;
const policyCms: Record<string, Uint8Array> = {};
for (const [id, chain] of Object.entries(policyChains)) {
  const caName = 'Policy CA';
  const subName = 'Policy Sub-CA';
  const ca = await certificate({
    subject: caName, issuer: rootIssuer, key: keys.inter, serial: policySerial++, ...VALID, ca: true, keyUsage: [0x06, 1], ski: true, aki: true, extra: chain.ca,
  });
  const sub = await certificate({
    subject: subName, issuer: { name: caName, key: keys.inter }, key: keys.other, serial: policySerial++, ...VALID, ca: true, keyUsage: [0x06, 1],
    ski: true, aki: true, extra: chain.sub, subjectAttributes: chain.subAttributes,
  });
  const signerCert = await certificate({
    subject: 'Policy Signer', issuer: { name: subName, key: keys.other }, key: keys.leaf, serial: policySerial++, ...VALID, ski: true, aki: true,
  });
  policyCms[id] = await buildDetachedCms({
    digest: new Uint8Array(32).map((_, i) => (i * 7 + 3) & 0xff),
    hash: 'sha256',
    profile: 'cades-b',
    signer: rawSigner(keys.leaf, [signerCert, ca, sub], 'RSA-PKCS1-v1_5', 'sha256'),
  });
}

// ── CMS blobs ────────────────────────────────────────────────────────────────

const digest = (n: number, seed: number) => new Uint8Array(n).map((_, i) => (i * 7 + seed) & 0xff);
const chain = [certs.leaf, certs.inter];
const cms: Record<string, Uint8Array> = {};

const build = async (
  signer: RawSigner,
  profile: 'pkcs7' | 'cades-b',
  hash: RawSigner['hash'] = signer.hash,
  signingTime?: Date,
) => buildDetachedCms({ digest: digest({ sha256: 32, sha384: 48, sha512: 64 }[hash], 3), hash, profile, signer, signingTime });

cms.rsaCades = await build(rawSigner(keys.leaf, chain, 'RSA-PKCS1-v1_5', 'sha256'), 'cades-b');
cms.rsaPkcs7 = await build(rawSigner(keys.leaf, chain, 'RSA-PKCS1-v1_5', 'sha256'), 'pkcs7', 'sha256', new Date('2026-09-11T00:00:00Z'));
cms.rsaPkcs7NoTime = await build(rawSigner(keys.leaf, chain, 'RSA-PKCS1-v1_5', 'sha256'), 'pkcs7');
cms.rsaSha384 = await build(rawSigner(keys.leaf, chain, 'RSA-PKCS1-v1_5', 'sha384'), 'cades-b');
cms.rsaSha512 = await build(rawSigner(keys.leaf, chain, 'RSA-PKCS1-v1_5', 'sha512'), 'cades-b');
certs.leaf8192 = await certificate({
  subject: 'Conformance 8192-bit Signer', issuer: interIssuer, key: keys.rsa8192, serial: 501, ...VALID, keyUsage: [0xc0, 6], ski: true, aki: true,
});
cms.rsa8192 = await build(rawSigner(keys.rsa8192, [certs.leaf8192, certs.inter], 'RSA-PKCS1-v1_5', 'sha256'), 'cades-b');
cms.pss8192 = await build(rawSigner(keys.rsa8192, [certs.leaf8192, certs.inter], 'RSA-PSS', 'sha256'), 'cades-b');
for (const hash of ['sha256', 'sha384', 'sha512'] as const) {
  cms[`pss${hash}`] = await build(rawSigner(keys.leaf, chain, 'RSA-PSS', hash), 'cades-b');
}
cms.ecP256 = await build(rawSigner(keys.p256, [certs.leafP256, certs.inter], 'ECDSA', 'sha256'), 'cades-b');
cms.ecP384 = await build(rawSigner(keys.p384, [certs.leafP384, certs.inter], 'ECDSA', 'sha384'), 'cades-b');
cms.ecP521 = await build(rawSigner(keys.p521, [certs.leafP521, certs.inter], 'ECDSA', 'sha512'), 'pkcs7');
cms.ecP256Sha512 = await build(rawSigner(keys.p256, [certs.leafP256], 'ECDSA', 'sha512'), 'cades-b');
cms.selfSigned = await build(rawSigner(keys.leaf, [certs.selfSigned], 'RSA-PKCS1-v1_5', 'sha256'), 'cades-b');
cms.leafOnly = await build(rawSigner(keys.leaf, [certs.leaf], 'RSA-PKCS1-v1_5', 'sha256'), 'cades-b');
cms.leafNoAki = await build(rawSigner(keys.leaf, [certs.leafNoAki, certs.inter], 'RSA-PKCS1-v1_5', 'sha256'), 'cades-b');
cms.leafExpired = await build(rawSigner(keys.leaf, [certs.leafExpired, certs.inter], 'RSA-PKCS1-v1_5', 'sha256'), 'cades-b');
cms.withRoot = await build(rawSigner(keys.leaf, [certs.leaf, certs.inter, certs.root], 'RSA-PKCS1-v1_5', 'sha256'), 'cades-b');
cms.interFirst = await build(rawSigner(keys.leaf, [certs.leaf, certs.root, certs.inter], 'RSA-PKCS1-v1_5', 'sha256'), 'cades-b');
cms.underNoBasic = await build(rawSigner(keys.leaf, [certs.leafUnderNoBasic, certs.interNoBasic], 'RSA-PKCS1-v1_5', 'sha256'), 'cades-b');
cms.underNoCertSign = await build(rawSigner(keys.leaf, [certs.leafUnderNoCertSign, certs.interNoCertSign], 'RSA-PKCS1-v1_5', 'sha256'), 'cades-b');
cms.underUnknown = await build(rawSigner(keys.leaf, [certs.leafUnderUnknown, certs.interUnknownCritical], 'RSA-PKCS1-v1_5', 'sha256'), 'cades-b');
cms.underNotCa = await build(rawSigner(keys.leaf, [certs.leafUnderNotCa, certs.interNotCa], 'RSA-PKCS1-v1_5', 'sha256'), 'cades-b');
cms.interExpired = await build(rawSigner(keys.leaf, [certs.leaf, certs.interExpired], 'RSA-PKCS1-v1_5', 'sha256'), 'cades-b');
cms.pssCert = await build(rawSigner(keys.leaf, [certs.leafPssSigned, certs.inter], 'RSA-PKCS1-v1_5', 'sha256'), 'cades-b');
cms.unknownCurve = await build(rawSigner(keys.p256, [certs.leafUnknownCurve], 'ECDSA', 'sha256'), 'cades-b');
cms.ecNoParams = await build(rawSigner(keys.p256, [certs.leafEcNoParams], 'ECDSA', 'sha256'), 'cades-b');
cms.dsaKey = await build(rawSigner(keys.leaf, [certs.leafDsaKey], 'RSA-PKCS1-v1_5', 'sha256'), 'cades-b');
// One certificate each, so the chain engine starts from the signer.
cms.underNoBasicLeaf = await build(rawSigner(keys.leaf, [certs.leafUnderNoBasic], 'RSA-PKCS1-v1_5', 'sha256'), 'cades-b');
cms.underNoCertSignLeaf = await build(rawSigner(keys.leaf, [certs.leafUnderNoCertSign], 'RSA-PKCS1-v1_5', 'sha256'), 'cades-b');
cms.underUnknownLeaf = await build(rawSigner(keys.leaf, [certs.leafUnderUnknown], 'RSA-PKCS1-v1_5', 'sha256'), 'cades-b');
cms.underNotCaLeaf = await build(rawSigner(keys.leaf, [certs.leafUnderNotCa], 'RSA-PKCS1-v1_5', 'sha256'), 'cades-b');
cms.wrongIssuerNameLeaf = await build(rawSigner(keys.leaf, [certs.leafWrongIssuerName], 'RSA-PKCS1-v1_5', 'sha256'), 'cades-b');
cms.pssCertLeaf = await build(rawSigner(keys.leaf, [certs.leafPssSigned], 'RSA-PKCS1-v1_5', 'sha256'), 'cades-b');
cms.noAkiLeaf = await build(rawSigner(keys.leaf, [certs.leafNoAki], 'RSA-PKCS1-v1_5', 'sha256'), 'cades-b');
cms.expiredLeaf = await build(rawSigner(keys.leaf, [certs.leafExpired], 'RSA-PKCS1-v1_5', 'sha256'), 'cades-b');
cms.ecP256Leaf = await build(rawSigner(keys.p256, [certs.leafP256], 'ECDSA', 'sha256'), 'cades-b');
cms.ecSelfSigned = await build(rawSigner(keys.p256, [certs.selfSignedP256], 'ECDSA', 'sha256'), 'cades-b');

// Surgery on the RSA CAdES blob. Each variant targets one refusal in parse.ts or verify.ts.
const flip = (bytes: Uint8Array, at: number) => {
  const out = bytes.slice();
  out[at] ^= 0x01;
  return out;
};
cms.rsaCadesTampered = flip(cms.rsaCades, cms.rsaCades.length - 1);
cms.ecP256Tampered = editCms(cms.ecP256, (sd) => {
  const sig = new Uint8Array(sd.signerInfos[0].signature.valueBlock.valueHexView);
  // Keep the DER of r and s intact: change a byte inside s.
  sig[sig.length - 2] ^= 0x01;
  sd.signerInfos[0].signature = new asn1js.OctetString({ valueHex: ab(sig) });
});
cms.ecP256GarbageSignature = editCms(cms.ecP256, (sd) => {
  sd.signerInfos[0].signature = new asn1js.OctetString({ valueHex: ab(new Uint8Array([1, 2, 3, 4])) });
});
cms.ecP256NotSequence = editCms(cms.ecP256, (sd) => {
  sd.signerInfos[0].signature = new asn1js.OctetString({ valueHex: new asn1js.Integer({ value: 5 }).toBER(false) });
});
cms.rsaShortSignature = editCms(cms.rsaCades, (sd) => {
  sd.signerInfos[0].signature = new asn1js.OctetString({ valueHex: ab(new Uint8Array(16)) });
});
const attrsOf = (sd: pkijs.SignedData) => sd.signerInfos[0].signedAttrs!.attributes;
cms.essMismatch = editCms(cms.rsaCades, (sd) => {
  const ess = attrsOf(sd).find((a) => a.type === '1.2.840.113549.1.9.16.2.47')!;
  const certsSeq = (ess.values[0] as asn1js.Sequence).valueBlock.value[0] as asn1js.Sequence;
  const id = certsSeq.valueBlock.value[0] as asn1js.Sequence;
  const hash = id.valueBlock.value[0] as asn1js.OctetString;
  const bytes = new Uint8Array(hash.valueBlock.valueHexView);
  bytes[0] ^= 0xff;
  id.valueBlock.value[0] = new asn1js.OctetString({ valueHex: ab(bytes) });
});
const essWith = (members: asn1js.BaseBlock[]) => (sd: pkijs.SignedData) => {
  const ess = attrsOf(sd).find((a) => a.type === '1.2.840.113549.1.9.16.2.47')!;
  const certsSeq = (ess.values[0] as asn1js.Sequence).valueBlock.value[0] as asn1js.Sequence;
  const id = certsSeq.valueBlock.value[0] as asn1js.Sequence;
  id.valueBlock.value = [...members, ...id.valueBlock.value];
};
cms.essExplicitSha256 = editCms(cms.rsaCades, (sd) => {
  const ess = attrsOf(sd).find((a) => a.type === '1.2.840.113549.1.9.16.2.47')!;
  const certsSeq = (ess.values[0] as asn1js.Sequence).valueBlock.value[0] as asn1js.Sequence;
  const id = certsSeq.valueBlock.value[0] as asn1js.Sequence;
  id.valueBlock.value = [
    new asn1js.Sequence({ value: [new asn1js.ObjectIdentifier({ value: '2.16.840.1.101.3.4.2.1' })] }),
    ...id.valueBlock.value,
  ];
});
cms.essSha1 = editCms(cms.rsaCades, essWith([new asn1js.Sequence({ value: [new asn1js.ObjectIdentifier({ value: '1.3.14.3.2.26' })] })]));
cms.essNoHash = editCms(cms.rsaCades, (sd) => {
  const ess = attrsOf(sd).find((a) => a.type === '1.2.840.113549.1.9.16.2.47')!;
  const certsSeq = (ess.values[0] as asn1js.Sequence).valueBlock.value[0] as asn1js.Sequence;
  const id = certsSeq.valueBlock.value[0] as asn1js.Sequence;
  id.valueBlock.value = id.valueBlock.value.slice(1);
});
cms.essEmpty = editCms(cms.rsaCades, (sd) => {
  const ess = attrsOf(sd).find((a) => a.type === '1.2.840.113549.1.9.16.2.47')!;
  ess.values = [new asn1js.Sequence({ value: [] })];
});
cms.noMessageDigest = editCms(cms.rsaCades, (sd) => {
  sd.signerInfos[0].signedAttrs!.attributes = attrsOf(sd).filter((a) => a.type !== '1.2.840.113549.1.9.4');
});
cms.messageDigestNotOctets = editCms(cms.rsaCades, (sd) => {
  const md = attrsOf(sd).find((a) => a.type === '1.2.840.113549.1.9.4')!;
  md.values = [new asn1js.Integer({ value: 7 })];
});
cms.contentTypeNotData = editCms(cms.rsaCades, (sd) => {
  const ct = attrsOf(sd).find((a) => a.type === '1.2.840.113549.1.9.3')!;
  ct.values = [new asn1js.ObjectIdentifier({ value: '1.2.840.113549.1.7.2' })];
});
cms.contentTypeEmpty = editCms(cms.rsaCades, (sd) => {
  const ct = attrsOf(sd).find((a) => a.type === '1.2.840.113549.1.9.3')!;
  ct.values = [];
});
cms.noContentType = editCms(cms.rsaCades, (sd) => {
  sd.signerInfos[0].signedAttrs!.attributes = attrsOf(sd).filter((a) => a.type !== '1.2.840.113549.1.9.3');
});
cms.generalizedTime = editCms(cms.rsaPkcs7, (sd) => {
  const st = attrsOf(sd).find((a) => a.type === '1.2.840.113549.1.9.5')!;
  st.values = [new asn1js.GeneralizedTime({ valueDate: new Date('2051-02-03T04:05:06Z') })];
});
cms.signingTimeNotTime = editCms(cms.rsaPkcs7, (sd) => {
  const st = attrsOf(sd).find((a) => a.type === '1.2.840.113549.1.9.5')!;
  st.values = [new asn1js.Utf8String({ value: 'yesterday' })];
});
cms.noSignedAttrs = editCms(cms.rsaCades, (sd) => {
  delete (sd.signerInfos[0] as { signedAttrs?: unknown }).signedAttrs;
});
cms.unknownDigest = editCms(cms.rsaCades, (sd) => {
  sd.signerInfos[0].digestAlgorithm = new pkijs.AlgorithmIdentifier({ algorithmId: '1.2.840.113549.2.5' });
});
cms.sha1Digest = editCms(cms.rsaCades, (sd) => {
  sd.signerInfos[0].digestAlgorithm = new pkijs.AlgorithmIdentifier({ algorithmId: '1.3.14.3.2.26' });
});
cms.twoSigners = editCms(cms.rsaCades, (sd) => {
  sd.signerInfos.push(sd.signerInfos[0]);
});
cms.noSigners = editCms(cms.rsaCades, (sd) => {
  sd.signerInfos = [];
});
cms.encapsulated = editCms(cms.rsaCades, (sd) => {
  sd.encapContentInfo = new pkijs.EncapsulatedContentInfo({
    eContentType: '1.2.840.113549.1.7.1',
    eContent: new asn1js.OctetString({ valueHex: ab(new Uint8Array([104, 105])) }),
  });
});
cms.noCertificates = editCms(cms.rsaCades, (sd) => {
  delete (sd as { certificates?: unknown }).certificates;
});
cms.wrongSignerCert = editCms(cms.rsaCades, (sd) => {
  sd.certificates = [pkijs.Certificate.fromBER(ab(certs.inter))];
});
cms.skiSid = editCms(cms.rsaCades, (sd) => {
  sd.signerInfos[0].version = 3;
  sd.signerInfos[0].sid = new asn1js.Primitive({ idBlock: { tagClass: 3, tagNumber: 0 }, valueHex: ab(skid(keys.leaf.spki)) });
});
cms.skiSidMixed = editCms(cms.rsaCades, (sd) => {
  // The signer found by key identifier past a certificate with no extensions
  // and one whose extensions carry no key identifier.
  sd.signerInfos[0].version = 3;
  sd.signerInfos[0].sid = new asn1js.Primitive({ idBlock: { tagClass: 3, tagNumber: 0 }, valueHex: ab(skid(keys.leaf.spki)) });
  sd.certificates = [certs.noCommonName, certs.leafNoAki, certs.leaf].map((der) => pkijs.Certificate.fromBER(ab(der)));
});
cms.cadesWithTime = editCms(cms.rsaCades, (sd) => {
  attrsOf(sd).push(
    new pkijs.Attribute({ type: '1.2.840.113549.1.9.5', values: [new asn1js.UTCTime({ valueDate: new Date('2026-09-11T00:00:00Z') })] }),
  );
});
cms.skiSidUnknown = editCms(cms.rsaCades, (sd) => {
  sd.signerInfos[0].version = 3;
  sd.signerInfos[0].sid = new asn1js.Primitive({ idBlock: { tagClass: 3, tagNumber: 0 }, valueHex: ab(new Uint8Array(20).fill(9)) });
});
cms.withTimestamp = editCms(cms.rsaCades, (sd) => {
  sd.signerInfos[0].unsignedAttrs = new pkijs.SignedAndUnsignedAttributes({
    type: 1,
    attributes: [
      new pkijs.Attribute({
        type: '1.2.840.113549.1.9.16.2.14',
        values: [new asn1js.Sequence({ value: [new asn1js.ObjectIdentifier({ value: '1.2.840.113549.1.7.2' }), new asn1js.Integer({ value: 1 })] })],
      }),
    ],
  });
});
cms.unknownSignatureAlgorithm = editCms(cms.rsaCades, (sd) => {
  sd.signerInfos[0].signatureAlgorithm = new pkijs.AlgorithmIdentifier({ algorithmId: '1.2.840.113549.1.1.4' });
});
cms.notSignedData = (() => {
  const info = new pkijs.ContentInfo({ contentType: '1.2.840.113549.1.7.1', content: new asn1js.OctetString({ valueHex: ab(new Uint8Array([1])) }) });
  return new Uint8Array(info.toSchema().toBER(false));
})();
cms.notContentInfo = new Uint8Array(new asn1js.Sequence({ value: [new asn1js.Integer({ value: 1 })] }).toBER(false));
cms.signedDataGarbage = (() => {
  const info = new pkijs.ContentInfo({ contentType: '1.2.840.113549.1.7.2', content: new asn1js.Sequence({ value: [new asn1js.Integer({ value: 1 })] }) });
  return new Uint8Array(info.toSchema().toBER(false));
})();
cms.truncated = cms.rsaCades.slice(0, 40);
cms.notBer = new Uint8Array([0x30, 0x84, 0xff, 0xff, 0xff, 0xff, 0x01]);

// Real third-party CMS, from the engine's signed fixtures (pyHanko and an
// OpenSSL-style `rsaEncryption` signer).
const fixturesDir = join(repo, 'packages/engine/main/test/fixtures');
cms.pdfCertified = (await pdfContents(join(fixturesDir, 'signed_certified.pdf')))[0];
cms.pdfApproval = (await pdfContents(join(fixturesDir, 'signed_two_approvals.pdf')))[0];
cms.pdfCompat = (await pdfContents(join(fixturesDir, 'signature-compat/v1/01-approval-control.pdf')))[0];

for (const [id, blob] of Object.entries(policyCms)) cms[`policy-${id}`] = blob;
// A signer identifier in the `[0]` constructed form, which carries no key
// identifier pkijs can read.
cms.skiSidConstructed = editCms(cms.rsaCades, (sd) => {
  sd.signerInfos[0].version = 3;
  sd.signerInfos[0].sid = new asn1js.Constructed({
    idBlock: { tagClass: 3, tagNumber: 0 },
    value: [new asn1js.OctetString({ valueHex: ab(skid(keys.leaf.spki)) })],
  });
});

// ── additions ────────────────────────────────────────────────────────────────
//
// Everything below was added after the first generation. The stored values
// replace the ones this run regenerated (the RSA ones come out the same; the
// ECDSA and RSA-PSS ones do not), so the additions are built from exactly
// the bytes the file holds.
if (stored) {
  for (const [name, value] of Object.entries(stored.certs)) certs[name] = fromB64(value);
  for (const [name, value] of Object.entries(stored.cms)) cms[name] = fromB64(value);
}

// A small DER reader and writer, for surgery pkijs would normalise away.
interface Der {
  id: number;
  raw: Uint8Array;
  body: Uint8Array;
  kids: Der[];
}
function derRead(bytes: Uint8Array, start = 0): [Der, number] {
  let at = start;
  const id = bytes[at++];
  let length = bytes[at++];
  if (length & 0x80) {
    const count = length & 0x7f;
    length = 0;
    for (let i = 0; i < count; i += 1) length = length * 256 + bytes[at++];
  }
  const body = bytes.subarray(at, at + length);
  const kids: Der[] = [];
  if (id & 0x20) {
    for (let cursor = 0; cursor < body.length; ) {
      const [kid, next] = derRead(body, cursor);
      kids.push(kid);
      cursor = next;
    }
  }
  return [{ id, raw: bytes.subarray(start, at + length), body, kids }, at + length];
}
const der = (bytes: Uint8Array) => derRead(bytes)[0];
const cat = (...parts: (Uint8Array | number[])[]) => {
  const out = new Uint8Array(parts.reduce((n, p) => n + p.length, 0));
  let at = 0;
  for (const part of parts) {
    out.set(part, at);
    at += part.length;
  }
  return out;
};
const derLength = (n: number): number[] => {
  if (n < 0x80) return [n];
  const bytes: number[] = [];
  for (let v = n; v > 0; v = Math.floor(v / 256)) bytes.unshift(v & 0xff);
  return [0x80 | bytes.length, ...bytes];
};
const tlv = (id: number, ...parts: (Uint8Array | number[])[]) => {
  const body = cat(...parts);
  return cat([id, ...derLength(body.length)], body);
};
const enc = (block: asn1js.BaseBlock) => new Uint8Array(block.toBER(false));
const oidDer = (oid: string) => enc(new asn1js.ObjectIdentifier({ value: oid }));
const attribute = (oid: string, ...values: Uint8Array[]) => tlv(0x30, oidDer(oid), tlv(0x31, ...values));
/** A Name from `[type, value DER]` pairs, one RDN each. */
const rawName = (...pairs: [string, Uint8Array][]) => tlv(0x30, ...pairs.map(([t, v]) => tlv(0x31, tlv(0x30, oidDer(t), v))));
const printable = (text: string) => enc(new asn1js.PrintableString({ value: text }));
const utf8 = (text: string) => enc(new asn1js.Utf8String({ value: text }));

/** A CMS taken apart: the SignedData members and the one SignerInfo's. */
interface CmsParts {
  version: Uint8Array;
  digests: Uint8Array;
  encap: Uint8Array;
  certs: Uint8Array[];
  signer: Uint8Array[];
}
function cmsParts(bytes: Uint8Array): CmsParts {
  const sd = der(bytes).kids[1].kids[0];
  const [version, digests, encap, ...rest] = sd.kids;
  const certsNode = rest.find((n) => n.id === 0xa0);
  return {
    version: version.raw,
    digests: digests.raw,
    encap: encap.raw,
    certs: certsNode ? certsNode.kids.map((k) => k.raw) : [],
    signer: rest[rest.length - 1].kids[0].kids.map((k) => k.raw),
  };
}
const cmsFrom = (p: CmsParts) =>
  tlv(
    0x30,
    oidDer('1.2.840.113549.1.7.2'),
    tlv(0xa0, tlv(0x30, p.version, p.digests, p.encap, tlv(0xa0, ...p.certs), tlv(0x31, tlv(0x30, ...p.signer)))),
  );
const editParts = (base: string, edit: (p: CmsParts) => void) => {
  const p = cmsParts(cms[base]);
  edit(p);
  return cmsFrom(p);
};
// SignerInfo members: version, sid, digestAlgorithm, [0] signedAttrs, signatureAlgorithm, signature.
const SID = 1;
const DIGEST_ALG = 2;
const ATTRS = 3;
const SIG_ALG = 4;
const SIG = 5;
const signedAttrsOf = (p: CmsParts) => der(p.signer[ATTRS]).kids.map((k) => k.raw);
const setAttrs = (p: CmsParts, attrs: Uint8Array[]) => {
  p.signer[ATTRS] = tlv(0xa0, ...attrs);
};
/** The bytes a signature over the signed attributes covers. */
const signedBytes = (p: CmsParts) => cat([0x31], p.signer[ATTRS].subarray(1));
const leafPrivate = createPrivateKey({ key: Buffer.from(keys.leaf.pkcs8), format: 'der', type: 'pkcs8' });

/** A certificate taken apart: the TBS members, the algorithm, the signature. */
interface CertParts {
  tbs: Uint8Array[];
  alg: Uint8Array;
  sig: Uint8Array;
}
const certParts = (bytes: Uint8Array): CertParts => {
  const c = der(bytes);
  return { tbs: c.kids[0].kids.map((k) => k.raw), alg: c.kids[1].raw, sig: c.kids[2].raw };
};
const certFrom = (p: CertParts) => tlv(0x30, tlv(0x30, ...p.tbs), p.alg, p.sig);
// TBS members: [0] version, serial, signature, issuer, validity, subject, spki, [3] extensions.
const ISSUER = 3;
const SUBJECT = 5;
const SPKI = 6;
const editCert = (bytes: Uint8Array, edit: (p: CertParts) => void) => {
  const p = certParts(bytes);
  edit(p);
  return certFrom(p);
};
/** The certificate with its key's algorithm OID replaced, parameters kept. */
const withSpkiOid = (bytes: Uint8Array, oid: string) =>
  editCert(bytes, (p) => {
    const spki = der(p.tbs[SPKI]);
    const [, ...params] = spki.kids[0].kids;
    p.tbs[SPKI] = tlv(0x30, tlv(0x30, oidDer(oid), ...params.map((n) => n.raw)), spki.kids[1].raw);
  });
if (b64(cmsFrom(cmsParts(cms.rsaCades))) !== b64(cms.rsaCades)) throw new Error('CMS surgery does not round-trip');

// Signer identifiers, names and certificate sets, each aimed at one rule in parse.rs or cert.rs.
const leafIssuerName = der(certParts(certs.leaf).tbs[ISSUER]).raw;
const serialOfLeaf = der(certParts(certs.leaf).tbs[1]).raw;
const sidWithIssuer = (name: Uint8Array) => (p: CmsParts) => {
  p.signer[SID] = tlv(0x30, name, serialOfLeaf);
};
// The attribute types swapped, the texts kept: pkijs compares types first.
cms.sidIssuerTypesSwapped = editParts(
  'rsaCades',
  sidWithIssuer(rawName(['2.5.4.3', printable('EmbedPDF Conformance')], ['2.5.4.10', utf8(interName)])),
);
// Case, spacing and string type differ; stringPrep makes them equal.
cms.sidIssuerStringPrep = editParts(
  'rsaCades',
  sidWithIssuer(rawName(['2.5.4.10', utf8('  embedpdf   CONFORMANCE ')], ['2.5.4.3', printable('conformance INTERMEDIATE ca')])),
);
// A signer whose issuer name carries a value that is not a string, matched by its encoding.
const numericIssuer = rawName(['2.5.4.10', printable('EmbedPDF Conformance')], ['2.5.4.5', enc(new asn1js.Integer({ value: 7 }))]);
cms.sidIssuerNotString = editParts('rsaCades', (p) => {
  p.certs = [editCert(p.certs[0], (c) => (c.tbs[ISSUER] = numericIssuer)), ...p.certs.slice(1)];
  sidWithIssuer(numericIssuer)(p);
});
cms.sidOneMember = editParts('rsaCades', (p) => {
  p.signer[SID] = tlv(0x30, leafIssuerName);
});
// Two certificates from the same issuer: the serial number decides.
cms.signerAfterSameIssuer = editParts('rsaCades', (p) => {
  p.certs = [certs.leafNoAki, certs.leaf, certs.inter];
});
// The other CertificateChoices: `[3]` constructed is skipped, `[3]` primitive is refused.
cms.otherCertificateFormat = editParts('rsaCades', (p) => {
  p.certs = [...p.certs, tlv(0xa3, oidDer('1.3.6.1.4.1.99999.4'), enc(new asn1js.Null()))];
});
cms.otherCertificatePrimitive = editParts('rsaCades', (p) => {
  p.certs = [...p.certs, tlv(0x83)];
});
// ContentInfo whose content is not wrapped in `[0]`.
cms.contentNotExplicit = (() => {
  const info = der(cms.rsaCades);
  return tlv(0x30, info.kids[0].raw, info.kids[1].kids[0].raw);
})();
// eContent as `[0]` primitive.
cms.econtentPrimitive = editParts('rsaCades', (p) => {
  p.encap = tlv(0x30, oidDer('1.2.840.113549.1.7.1'), tlv(0x80, [104, 105]));
});
// Attributes: one with no value SET, one whose values are a SEQUENCE.
cms.attributeWithoutValues = editParts('rsaCades', (p) => {
  setAttrs(p, [...signedAttrsOf(p), tlv(0x30, oidDer('1.3.6.1.4.1.99999.5'))]);
});
cms.attributeValuesNotSet = editParts('rsaCades', (p) => {
  setAttrs(p, [...signedAttrsOf(p), tlv(0x30, oidDer('1.3.6.1.4.1.99999.5'), tlv(0x30, enc(new asn1js.Null())))]);
});
// A content-type value that is an INTEGER over id-data's OID bytes.
cms.contentTypeInteger = editParts('rsaCades', (p) => {
  const idData = der(oidDer('1.2.840.113549.1.7.1')).body;
  setAttrs(p, [attribute('1.2.840.113549.1.9.3', tlv(0x02, idData)), ...signedAttrsOf(p).slice(1)]);
});
// Unsigned attributes as `[1]` primitive.
cms.unsignedPrimitive = editParts('rsaCades', (p) => {
  p.signer = [...p.signer, tlv(0x81)];
});
// Signature algorithm OIDs: a truncated last arc, and first arcs of 0, 1 and 2 at their edges.
const sigAlgBytes = (content: number[]) => (p: CmsParts) => {
  p.signer[SIG_ALG] = tlv(0x30, tlv(0x06, content));
};
cms.signatureOidTruncated = editParts('rsaCades', sigAlgBytes([0x2a, 0x86, 0x48, 0x86, 0xf7, 0x0d, 0x01, 0x01, 0x0b, 0x81]));
cms.signatureOidArc0 = editParts('rsaCades', sigAlgBytes([0x01, 0x02]));
cms.signatureOidArc40 = editParts('rsaCades', sigAlgBytes([0x28, 0x01]));
cms.signatureOidArc80 = editParts('rsaCades', sigAlgBytes([0x50, 0x01]));

// Signing times: long-form tag, 19xx, a fraction, a two-digit GeneralizedTime year.
const withSigningTime = (value: Uint8Array) => (p: CmsParts) => {
  setAttrs(p, [...signedAttrsOf(p).slice(0, 2), attribute('1.2.840.113549.1.9.5', value)]);
};
const ascii = (text: string) => [...new TextEncoder().encode(text)];
cms.signingTimeLongTag = editParts('rsaPkcs7', withSigningTime(cat([0x1f, 0x17, 13], ascii('260911083015Z'))));
cms.signingTime1999 = editParts('rsaPkcs7', withSigningTime(tlv(0x17, ascii('991231235959Z'))));
cms.signingTimeFraction = editParts('rsaPkcs7', withSigningTime(tlv(0x18, ascii('20260911083015.5Z'))));
cms.signingTimeYear0050 = editParts('rsaPkcs7', withSigningTime(tlv(0x18, ascii('00500615120000Z'))));

// Lengths: the outer SEQUENCE's length in eight bytes (allowed) and nine (refused), and indefinite.
const outer = der(cms.rsaCades);
const longLength = (count: number) => {
  const bytes = new Array(count).fill(0);
  for (let v = outer.body.length, i = count - 1; v > 0; v = Math.floor(v / 256), i -= 1) bytes[i] = v & 0xff;
  return bytes;
};
cms.lengthEightBytes = cat([0x30, 0x88, ...longLength(8)], outer.body);
cms.lengthNineBytes = cat([0x30, 0x89, ...longLength(9)], outer.body);
cms.indefiniteLength = (() => {
  const explicit = outer.kids[1];
  const signedData = explicit.kids[0];
  // The certificates `[0]` indefinite too: an end-of-contents misread there leaves garbage where the SignerInfos belong.
  const members = signedData.kids.map((k) => (k.id === 0xa0 ? cat([0xa0, 0x80], k.body, [0, 0]) : k.raw));
  const inner = cat([0x30, 0x80], ...members, [0, 0]);
  return cat([0x30, 0x80], outer.kids[0].raw, [0xa0, 0x80], inner, [0, 0], [0, 0]);
})();

// Verification: signer keys labelled with algorithms that are not key algorithms. The PKCS#7
// blob carries no ESS hash, so the certificate can change under a signature that still verifies.
const withSignerCert = (base: string, edit: (cert: Uint8Array) => Uint8Array) =>
  editParts(base, (p) => {
    p.certs = [edit(p.certs[0]), ...p.certs.slice(1)];
  });
for (const [name, oid] of [
  ['Sha256WithRsa', '1.2.840.113549.1.1.11'],
  ['Oaep', '1.2.840.113549.1.1.7'],
  ['Ecdh', '1.3.133.16.840.63.0.2'],
  ['AesCbc', '2.16.840.1.101.3.4.1.2'],
  ['AesGcm', '2.16.840.1.101.3.4.1.6'],
  ['AesCfb', '2.16.840.1.101.3.4.1.4'],
  ['AesKw', '2.16.840.1.101.3.4.1.5'],
  ['Hmac', '1.2.840.113549.2.7'],
  ['Dh', '1.2.840.113549.1.9.16.3.5'],
  ['Sha1', '1.3.14.3.2.26'],
  ['Pbkdf2', '1.2.840.113549.1.5.12'],
]) {
  cms[`rsaKeyAs${name}`] = withSignerCert('rsaPkcs7NoTime', (cert) => withSpkiOid(cert, oid));
}
cms.ecKeyAsEcdsaSha512 = withSignerCert('ecP521', (cert) => withSpkiOid(cert, '1.2.840.10045.4.3.4'));

// ECDSA signature encodings over the P-521 blob (PKCS#7, so no ESS hash in the way).
const ecSignature = () => der(der(cmsParts(cms.ecP521).signer[SIG]).body);
const withEcSignature = (value: Uint8Array) => (p: CmsParts) => {
  p.signer[SIG] = tlv(0x04, value);
};
{
  const [r, s] = ecSignature().kids;
  cms.ecdsaSAsOctets = editParts('ecP521', withEcSignature(tlv(0x30, r.raw, tlv(0x04, s.body))));
  // r padded with zeros to 67 bytes: one more than P-521 holds, even after one sign byte is dropped.
  const padded = cat(new Array(67 - r.body.length).fill(0), r.body);
  cms.ecdsaRPadded = editParts('ecP521', withEcSignature(tlv(0x30, tlv(0x02, padded), s.raw)));
  cms.ecdsaZeroIntegers = editParts('ecP521', withEcSignature(tlv(0x30, tlv(0x02, [0]), tlv(0x02, [0]))));
}
// SHA-384 over a P-521 key: a hash shorter than the field but more than half of it.
cms.ecP521Sha384 = editParts('ecP521', (p) => {
  p.signer[DIGEST_ALG] = tlv(0x30, oidDer('2.16.840.1.101.3.4.2.2'));
});

// RSA-PSS with a salt of zero, which signs deterministically.
const pssParams = (hash: string, salt: number[]) =>
  tlv(
    0x30,
    tlv(0xa0, tlv(0x30, oidDer(hash), enc(new asn1js.Null()))),
    tlv(0xa1, tlv(0x30, oidDer('1.2.840.113549.1.1.8'), tlv(0x30, oidDer(hash), enc(new asn1js.Null())))),
    tlv(0xa2, tlv(0x02, salt)),
  );
const pss = (nodeHash: string, oid: string, salt: number[]) =>
  editParts('rsaPkcs7NoTime', (p) => {
    const signature = nodeSign(nodeHash, signedBytes(p), { key: leafPrivate, padding: constants.RSA_PKCS1_PSS_PADDING, saltLength: 0 });
    p.signer[SIG_ALG] = tlv(0x30, oidDer('1.2.840.113549.1.1.10'), pssParams(oid, salt));
    p.signer[SIG] = tlv(0x04, signature);
  });
cms.pssSaltZero = pss('sha256', '2.16.840.1.101.3.4.2.1', [0]);
// The same signature, its salt length written as 256 in two octets.
cms.pssSalt256 = pss('sha256', '2.16.840.1.101.3.4.2.1', [1, 0]);
// Parameters naming SHA-1 over a SHA-256 signature, and a SHA-1 signature under them: WebCrypto
// verifies with the key's hash, which pkijs imports as the CMS digest algorithm.
cms.pssSha1 = pss('sha1', '1.3.14.3.2.26', [0]);
cms.pssSha1ParamsOverSha256 = pss('sha256', '1.3.14.3.2.26', [0]);
// A parameters hash pkijs cannot name throws before anything is verified.
cms.pssUnknownHash = pss('sha256', '1.2.3.4', [0]);

// Certificates for certificateCommonName, each refused or read by one rule.
const leafSubject = (subject: Uint8Array) => editCert(certs.leaf, (p) => (p.tbs[SUBJECT] = subject));
certs.universalStringCommonName = await certificate({
  subject: 'x', key: keys.leaf, serial: 50, ...VALID,
  subjectAttributes: [new pkijs.AttributeTypeAndValue({ type: OID.commonName, value: new asn1js.UniversalString({ value: 'Univ Name' }) })],
});
certs.utf8AccentedCommonName = await certificate({
  subject: 'x', key: keys.leaf, serial: 51, ...VALID,
  subjectAttributes: [new pkijs.AttributeTypeAndValue({ type: OID.commonName, value: new asn1js.Utf8String({ value: 'Zoë Ångström' }) })],
});
certs.rdnSequence = leafSubject(tlv(0x30, tlv(0x30, tlv(0x30, oidDer(OID.commonName), utf8('In A Sequence')))));
certs.attributeAsSet = leafSubject(tlv(0x30, tlv(0x31, tlv(0x31, oidDer(OID.commonName), utf8('In A Set')))));
certs.attributeOneMember = leafSubject(tlv(0x30, tlv(0x31, tlv(0x30, oidDer(OID.commonName)))));
certs.signatureAsOctets = editCert(certs.leaf, (p) => (p.sig = tlv(0x04, der(p.sig).body)));

// Chains for the rules in trust.rs, each a lone signer unless it says otherwise.
const alone = (cert: Uint8Array) => build(rawSigner(keys.leaf, [cert], 'RSA-PKCS1-v1_5', 'sha256'), 'cades-b');
const AKI = OID.authorityKeyIdentifier;
// A signer whose certificate signature no longer verifies.
cms.leafTampered = editParts('leafOnly', (p) => {
  const cert = p.certs[0].slice();
  cert[cert.length - 1] ^= 0x01;
  p.certs = [cert];
});
// A second self-signed certificate over the same key and name.
certs.selfSignedTwin = await certificate({ subject: 'Conformance Self-Signed', key: keys.leaf, serial: 200, ...VALID, ca: true, keyUsage: [0x84, 2] });
// A certificate named as its own issuer but signed by another key, which an anchor of that name holds.
certs.twinCaAnchor = await certificate({ subject: 'Conformance Twin CA', key: keys.root, serial: 201, ...VALID, ca: true, keyUsage: [0x06, 1] });
certs.twinCaLeaf = await certificate({
  subject: 'Conformance Twin CA', issuer: { name: 'Conformance Twin CA', key: keys.root }, key: keys.leaf, serial: 202, ...VALID,
});
cms.twinCaLeaf = await alone(certs.twinCaLeaf);
// Authority key identifiers without a key identifier: empty, and naming the issuer only.
certs.leafEmptyAki = await certificate({
  subject: 'Conformance Signer With Empty AKI', issuer: interIssuer, key: keys.leaf, serial: 203, ...VALID,
  extra: [[AKI, seq().toBER(false)]],
});
cms.leafEmptyAki = await alone(certs.leafEmptyAki);
certs.leafAkiIssuerOnly = await certificate({
  subject: 'Conformance Signer With Issuer-Only AKI', issuer: interIssuer, key: keys.leaf, serial: 204, ...VALID,
  extra: [[AKI, ab(tlv(0x30, tlv(0xa1, tlv(0xa4, enc(name(interName).toSchema())))))]],
});
cms.leafAkiIssuerOnly = await alone(certs.leafAkiIssuerOnly);
// The unknown-extension CA again, its critical extension's value no longer BER.
certs.interCriticalNotBer = await certificate({
  subject: 'Conformance CA With Unknown Extension', issuer: rootIssuer, key: keys.inter, serial: 205, ...VALID, ca: true, keyUsage: [0x06, 1],
  ski: true, aki: true, extra: [['1.3.6.1.4.1.99999.1', ab(new Uint8Array([0xff])), true]],
});
// Signers whose certificates are signed with SHA-1, SHA-384 and SHA-512 over RSA, and with ECDSA.
for (const [label, hash, serial] of [['Sha1', 'SHA-1', 206], ['Sha384', 'SHA-384', 207], ['Sha512', 'SHA-512', 208]] as const) {
  certs[`leaf${label}Signed`] = await certificate({
    subject: `Conformance ${hash} Signed Signer`, issuer: interIssuer, key: keys.leaf, serial, ...VALID, ski: true, aki: true, hash,
  });
  cms[`leaf${label}Signed`] = await alone(certs[`leaf${label}Signed`]);
}
certs.leafUnderEc = await certificate({
  subject: 'Conformance Signer Under EC CA', issuer: { name: 'Conformance EC Self-Signed', key: keys.p256 }, key: keys.leaf, serial: 209, ...VALID,
});
cms.leafUnderEc = await alone(certs.leafUnderEc);
// The CA without BasicConstraints again, now with BasicConstraints that carry only a path length.
certs.interPathLengthOnly = await certificate({
  subject: 'Conformance CA Without BasicConstraints', issuer: rootIssuer, key: keys.inter, serial: 210, ...VALID, keyUsage: [0x06, 1],
  ski: true, aki: true, extra: [[OID.basicConstraints, seq(new asn1js.Integer({ value: 3 })).toBER(false), true]],
});
// Two issuers for one certificate: a second intermediate over the same key, and a self-signed one.
certs.interTwin = await certificate({
  subject: interName, issuer: rootIssuer, key: keys.inter, serial: 211, ...VALID, ca: true, keyUsage: [0x06, 1], ski: true, aki: true,
});
certs.interSelfSigned = await certificate({ subject: interName, key: keys.inter, serial: 212, ...VALID, ca: true, keyUsage: [0x06, 1], ski: true });
// Carried [signer, intermediate, leaf]: the path starts from the leaf, which the intermediate and a self-signed anchor both issue.
cms.leafLastCarried = await build(rawSigner(keys.leaf, [certs.selfSigned, certs.inter, certs.leaf], 'RSA-PKCS1-v1_5', 'sha256'), 'cades-b');
// Policy rules on a two-certificate path: the signer inhibits mapping, then maps.
certs.leafInhibitsOwnMapping = await certificate({
  subject: 'Conformance Signer Inhibiting Its Own Mapping', issuer: interIssuer, key: keys.leaf, serial: 213, ...VALID, ski: true, aki: true,
  extra: [constraintsExt(undefined, 0), mappingsExt([P1, P2])],
});
cms.leafInhibitsOwnMapping = await alone(certs.leafInhibitsOwnMapping);
// An intermediate whose name constraints exclude the signer; as the anchor it constrains nothing.
certs.interExcludingLeaf = await certificate({
  subject: interName, issuer: rootIssuer, key: keys.inter, serial: 214, ...VALID, ca: true, keyUsage: [0x06, 1], ski: true, aki: true,
  extra: [nameConstraintsExt([], [dirName('Conformance Signer')])],
});

// More policy and name-constraint chains, up to four CAs under the root. Each CMS carries
// [signer, CA 1, ..., CA n], so the path is CA n, ..., CA 1, root.
interface Level {
  ext: [string, ArrayBuffer, boolean?][];
  attributes?: pkijs.AttributeTypeAndValue[];
}
const cnAttr = (cn: string) => new pkijs.AttributeTypeAndValue({ type: OID.commonName, value: new asn1js.Utf8String({ value: cn }) });
const dirNameOf = (...attributes: pkijs.AttributeTypeAndValue[]) => {
  const rdn = new pkijs.RelativeDistinguishedNames();
  rdn.typesAndValues.push(...attributes);
  return new asn1js.Constructed({ idBlock: { tagClass: 3, tagNumber: 4 }, value: [rdn.toSchema()] });
};
const emailAttr = (email: string) => new pkijs.AttributeTypeAndValue({ type: '1.2.840.113549.1.9.1', value: new asn1js.IA5String({ value: email }) });
const ipv6 = (head: number[], tail: number[] = []) => [...head, ...new Array(16 - head.length - tail.length).fill(0), ...tail];
const levels = (...ext: [string, ArrayBuffer, boolean?][][]): Level[] => ext.map((e) => ({ ext: e }));
const deepChains: Record<string, Level[]> = {
  explicitPendingMin: levels([policiesExt(P1), constraintsExt(256)], [policiesExt(P2), constraintsExt(1)]),
  dnsLongerConstraint: levels([nameConstraintsExt([ia5(2, 'sub.example.com')], [])], [altNamesExt(ia5(2, 'example.com'))]),
  uriRefused: levels([nameConstraintsExt([ia5(6, '.example.com')], [])], [altNamesExt(ia5(6, 'https://ca.evil.org/'))]),
  uriSlashConstraint: levels([nameConstraintsExt([ia5(6, 'http:/')], [])], [altNamesExt(ia5(6, 'http:/'))]),
  uriHostPort: levels([nameConstraintsExt([ia5(6, '.example.com')], [])], [altNamesExt(ia5(6, 'ca.example.com:8080'))]),
  ipRefused: levels([nameConstraintsExt([ipName([10, 0, 0, 0, 255, 0, 0, 0])], [])], [altNamesExt(ipName([11, 1, 2, 3]))]),
  ipv6Permitted: levels(
    [nameConstraintsExt([ipName([...ipv6([0x20, 0x01, 0x0d, 0xb8]), ...ipv6([0xff, 0xff, 0xff, 0xff])])], [])],
    [altNamesExt(ipName(ipv6([0x20, 0x01, 0x0d, 0xb8], [1])))],
  ),
  directoryEmpty: levels([nameConstraintsExt([dirNameOf()], [])], []),
  directorySuffix: levels([nameConstraintsExt([dirNameOf(cnAttr('Policy Sub-CA'))], [])], []),
  directoryDecoy: [
    { ext: [nameConstraintsExt([dirNameOf(cnAttr('Policy Sub-CA'))], [])] },
    { ext: [], attributes: [org, cnAttr('Decoy'), cnAttr('Policy Sub-CA')] },
  ],
  directoryPrefix: levels([nameConstraintsExt([dirNameOf(org)], [])], []),
  explicitThenInhibit: levels([constraintsExt(0)], [constraintsExt(undefined, 0), mappingsExt([P1, P2])]),
  inhibitAnyNow: levels([inhibitAnyExt(0), policiesExt(ANY_POLICY)], [constraintsExt(0), policiesExt(ANY_POLICY)]),
  inhibitAnyPending: levels([inhibitAnyExt(1), policiesExt(ANY_POLICY)], [constraintsExt(0), policiesExt(ANY_POLICY)]),
  inhibitAnyMin: levels(
    [inhibitAnyExt(5), policiesExt(ANY_POLICY)],
    [inhibitAnyExt(1), policiesExt(ANY_POLICY)],
    [constraintsExt(0), policiesExt(ANY_POLICY)],
  ),
  requireOnlyMapping: levels([constraintsExt(5)], [mappingsExt([P1, P2])]),
  mappingPendingLate: levels([constraintsExt(undefined, 1)], [], [mappingsExt([P1, P2])]),
  mappingPendingInTime: levels([constraintsExt(undefined, 1)], [policiesExt(P1), mappingsExt([P1, P2])], [policiesExt(P2)]),
  mappingMinLate: levels([constraintsExt(undefined, 5)], [constraintsExt(undefined, 1)], [], [mappingsExt([P1, P2])]),
  mappingMinEqual: levels([constraintsExt(undefined, 2)], [constraintsExt(undefined, 2)], [], [mappingsExt([P1, P2])]),
  mappingMinInTime: levels(
    [constraintsExt(undefined, 5)],
    [constraintsExt(undefined, 1)],
    [policiesExt(P1), mappingsExt([P1, P2])],
    [policiesExt(P2)],
  ),
  explicitLateStart: levels(
    [constraintsExt(3), policiesExt(ANY_POLICY)],
    [constraintsExt(0), policiesExt(ANY_POLICY)],
    [policiesExt(ANY_POLICY)],
  ),
  mappingMappedExplicit: levels([policiesExt(P1), mappingsExt([P1, P2])], [policiesExt(P2), constraintsExt(0)]),
  emailSubjectPermitted: [
    { ext: [nameConstraintsExt([ia5(1, '.example.com')], [])] },
    { ext: [], attributes: [emailAttr('ca@mail.example.com'), cnAttr('Policy Sub-CA')] },
  ],
};
const levelName = (i: number) => (i === 0 ? 'Policy CA' : i === 1 ? 'Policy Sub-CA' : `Policy Sub-CA ${i}`);
let deepSerial = 300;
for (const [id, chainLevels] of Object.entries(deepChains)) {
  const made: Uint8Array[] = [];
  let issuer = rootIssuer;
  for (const [i, level] of chainLevels.entries()) {
    // A key per level: two levels over one key would issue each other, and pkijs never returns from the loop.
    const key = [keys.inter, keys.other, keys.corpus, keys.leaf][i];
    made.push(
      await certificate({
        subject: levelName(i), issuer, key, serial: deepSerial++, ...VALID, ca: true, keyUsage: [0x06, 1], ski: true, aki: true,
        extra: level.ext, subjectAttributes: level.attributes,
      }),
    );
    issuer = { name: levelName(i), key };
  }
  const signerCert = await certificate({
    subject: 'Policy Signer', issuer, key: keys.leaf, serial: deepSerial++, ...VALID, ski: true, aki: true,
  });
  cms[`policy-${id}`] = await build(rawSigner(keys.leaf, [signerCert, ...made], 'RSA-PKCS1-v1_5', 'sha256'), 'cades-b');
}

// Two CAs over one key under a CA between them, so each of the outer two issues the
// other's child: pkijs's buildPath never returns on it, so no vector uses it.
// crates/plugins/signature-core/tests/chain_cycle.rs checks the Rust ends it.
{
  const ca1 = await certificate({
    subject: 'Cycle CA', issuer: rootIssuer, key: keys.inter, serial: 400, ...VALID, ca: true, keyUsage: [0x06, 1], ski: true, aki: true,
  });
  const ca2 = await certificate({
    subject: 'Cycle Sub-CA', issuer: { name: 'Cycle CA', key: keys.inter }, key: keys.other, serial: 401, ...VALID, ca: true,
    keyUsage: [0x06, 1], ski: true, aki: true,
  });
  const ca3 = await certificate({
    subject: 'Cycle Sub-CA 2', issuer: { name: 'Cycle Sub-CA', key: keys.other }, key: keys.inter, serial: 402, ...VALID, ca: true,
    keyUsage: [0x06, 1], ski: true, aki: true,
  });
  const signerCert = await certificate({
    subject: 'Cycle Signer', issuer: { name: 'Cycle Sub-CA 2', key: keys.inter }, key: keys.leaf, serial: 403, ...VALID, ski: true, aki: true,
  });
  cms.cyclicIssuers = await build(rawSigner(keys.leaf, [signerCert, ca1, ca2, ca3], 'RSA-PKCS1-v1_5', 'sha256'), 'cades-b');
}

const out = {
  note: 'Generated by fixtures/signature/generate.ts. Test-only keys; never use them for anything else.',
  keys: Object.fromEntries(Object.entries(keys).map(([k, v]) => [k, { kind: v.kind, pkcs8: b64(v.pkcs8), spki: b64(v.spki) }])),
  certs: Object.fromEntries(Object.entries(certs).map(([k, v]) => [k, stored?.certs[k] ?? b64(v)])),
  cms: Object.fromEntries(Object.entries(cms).map(([k, v]) => [k, stored?.cms[k] ?? b64(v)])),
  // The build cases answer from this table; an extending run adds nothing to it.
  signatures: stored?.signatures ?? signatures,
};
await writeFile(join(here, 'fixtures.json'), `${JSON.stringify(out, null, 2)}\n`);
console.log(`wrote fixtures.json: ${Object.keys(certs).length} certificates, ${Object.keys(cms).length} CMS blobs`);
