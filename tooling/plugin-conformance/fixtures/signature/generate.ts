/**
 * Writes `fixtures.json`: the keys, certificates, signatures and CMS blobs the
 * `core/signature` vectors are made from.
 *
 *   npx tsx --conditions=development fixtures/signature/generate.ts
 *
 * Key generation, ECDSA and RSA-PSS signing are random, so this runs once and
 * its output is committed. The vectors only read the file, which keeps
 * `vectors:check` deterministic. Re-running it rewrites every key, and every
 * vector that embeds one changes with it.
 *
 * Two kinds of signature are recorded under `signatures`, keyed by the
 * SHA-256 of the bytes signed: the ones this script made for the build cases
 * in `src/suites/signature-core.ts` (the scripted raw signer answers from
 * this table), and none else. Certificates are built with pkijs, the same
 * library `packages/core/signature` parses them with.
 */
import { createHash, webcrypto } from 'node:crypto';
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

async function generate(kind: KeyKind): Promise<KeyPair> {
  const pair = (await subtle.generateKey(
    kind === 'rsa'
      ? { name: 'RSASSA-PKCS1-v1_5', modulusLength: 2048, publicExponent: new Uint8Array([1, 0, 1]), hash: 'SHA-256' }
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

const corpusKey = await fromPem(
  await readFile(join(repo, 'packages/engine/main/test/fixtures/signature-compat/keys/TEST-ONLY-private-key.pem'), 'utf8'),
);
const keys = {
  root: await generate('rsa'),
  inter: await generate('rsa'),
  leaf: await generate('rsa'),
  other: await generate('rsa'),
  p256: await generate('p256'),
  p384: await generate('p384'),
  p521: await generate('p521'),
  corpus: corpusKey,
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

const out = {
  note: 'Generated by fixtures/signature/generate.ts. Test-only keys; never use them for anything else.',
  keys: Object.fromEntries(Object.entries(keys).map(([k, v]) => [k, { kind: v.kind, pkcs8: b64(v.pkcs8), spki: b64(v.spki) }])),
  certs: Object.fromEntries(Object.entries(certs).map(([k, v]) => [k, b64(v)])),
  cms: Object.fromEntries(Object.entries(cms).map(([k, v]) => [k, b64(v)])),
  signatures,
};
await writeFile(join(here, 'fixtures.json'), `${JSON.stringify(out, null, 2)}\n`);
console.log(`wrote fixtures.json: ${Object.keys(certs).length} certificates, ${Object.keys(cms).length} CMS blobs`);
