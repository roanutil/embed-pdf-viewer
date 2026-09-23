/**
 * PDF Standard security handler, enough of it to generate fixtures.
 *
 * G3's gate asks for permission traces, and the repository had exactly one
 * encrypted document: `encrypted-hello.pdf`, user password `hello`. One
 * document cannot distinguish a user unlock from an owner unlock, cannot cover
 * the four security handler revisions in use, and cannot produce the
 * unsupported-handler path at all. So the fixtures are generated, like every
 * other fixture here, rather than committed as opaque bytes somebody made once.
 *
 * ISO 32000-1 section 7.6.3 for revisions 2 through 4, ISO 32000-2 section
 * 7.6.4.3 for revision 6. Algorithm numbers in the comments below are theirs.
 *
 * Nothing here is a cryptography implementation anyone should reuse. RC4 is
 * broken, revision 2's 40-bit key is a rounding error, and the salts and file
 * IDs are fixed constants rather than random, because a fixture that changed
 * on every run could not be diffed in CI. It exists to produce documents
 * PDFium will open exactly one way.
 */
import { createCipheriv, createHash } from 'node:crypto';

/** ISO 32000-1 Table 20, the 32-byte password padding string. */
const PAD = Buffer.from([
  0x28, 0xbf, 0x4e, 0x5e, 0x4e, 0x75, 0x8a, 0x41, 0x64, 0x00, 0x4e, 0x56, 0xff, 0xfa, 0x01, 0x08,
  0x2e, 0x2e, 0x00, 0xb6, 0xd0, 0x68, 0x3e, 0x80, 0x2f, 0x0c, 0xa9, 0xfe, 0x64, 0x53, 0x69, 0x7a,
]);

/**
 * Fixed, because the file ID feeds the key derivation and a random one would
 * make every byte of the output change per run.
 */
export const FILE_ID = Buffer.from('0123456789abcdef0123456789abcdef', 'hex');

/** Fixed for revision 6's salts and the AES initialization vectors. */
const FIXED_SALTS = Buffer.from('a1b2c3d4e5f60718293a4b5c6d7e8f90', 'hex');
const FIXED_IV = Buffer.from('0f1e2d3c4b5a69788796a5b4c3d2e1f0', 'hex');

/**
 * RC4, because OpenSSL 3 dropped it and `createCipheriv('rc4', ...)` throws
 * ERR_OSSL_EVP_UNSUPPORTED. Revisions 2 through 4 have no other option.
 */
function rc4(key, data) {
  const s = new Uint8Array(256);
  for (let i = 0; i < 256; i++) s[i] = i;
  let j = 0;
  for (let i = 0; i < 256; i++) {
    j = (j + s[i] + key[i % key.length]) & 0xff;
    [s[i], s[j]] = [s[j], s[i]];
  }
  const out = Buffer.allocUnsafe(data.length);
  let i = 0;
  j = 0;
  for (let n = 0; n < data.length; n++) {
    i = (i + 1) & 0xff;
    j = (j + s[i]) & 0xff;
    [s[i], s[j]] = [s[j], s[i]];
    out[n] = data[n] ^ s[(s[i] + s[j]) & 0xff];
  }
  return out;
}

const md5 = (...parts) => createHash('md5').update(Buffer.concat(parts)).digest();
const sha = (bits, data) => createHash(`sha${bits}`).update(data).digest();

/** Password padded or truncated to 32 bytes, per Algorithm 2 step (a). */
function padPassword(password) {
  const bytes = Buffer.from(password, 'latin1');
  return Buffer.concat([bytes, PAD]).subarray(0, 32);
}

/** `P` as four bytes, low-order first, treated as a signed 32-bit integer. */
function permissionsBytes(p) {
  const out = Buffer.allocUnsafe(4);
  out.writeInt32LE(p | 0);
  return out;
}

/** Algorithm 3: the `/O` entry. */
function computeOwnerEntry({ revision, keyLength, ownerPassword, userPassword }) {
  // "If there is no owner password, use the user password instead."
  let digest = md5(padPassword(ownerPassword || userPassword));
  if (revision >= 3) {
    for (let i = 0; i < 50; i++) digest = md5(digest);
  }
  const key = digest.subarray(0, revision === 2 ? 5 : keyLength / 8);

  let out = rc4(key, padPassword(userPassword));
  if (revision >= 3) {
    for (let i = 1; i <= 19; i++) {
      out = rc4(Buffer.from(key.map((b) => b ^ i)), out);
    }
  }
  return out;
}

/** Algorithm 2: the file encryption key. */
function computeFileKey({ revision, keyLength, userPassword, ownerEntry, permissions }) {
  const n = revision === 2 ? 5 : keyLength / 8;
  let digest = md5(
    padPassword(userPassword),
    ownerEntry,
    permissionsBytes(permissions),
    FILE_ID,
    // Step (f) only applies at revision 4 and above, and only when metadata is
    // left unencrypted. These fixtures always encrypt it, so it never fires;
    // the branch is here so the omission is visibly deliberate.
  );
  if (revision >= 3) {
    for (let i = 0; i < 50; i++) digest = md5(digest.subarray(0, n));
  }
  return digest.subarray(0, n);
}

/** Algorithms 4 and 5: the `/U` entry. */
function computeUserEntry({ revision, fileKey }) {
  if (revision === 2) return rc4(fileKey, PAD);

  let out = rc4(fileKey, md5(PAD, FILE_ID));
  for (let i = 1; i <= 19; i++) {
    out = rc4(Buffer.from(fileKey.map((b) => b ^ i)), out);
  }
  // "Append 16 bytes of arbitrary padding." Fixed, for reproducibility.
  return Buffer.concat([out, FIXED_SALTS]);
}

/** Algorithm 1: the per-object key for revisions up to 4. */
function objectKey({ fileKey, objectNumber, generation, aes }) {
  const extra = Buffer.allocUnsafe(5);
  extra.writeUIntLE(objectNumber, 0, 3);
  extra.writeUIntLE(generation, 3, 2);
  const parts = [fileKey, extra];
  // AESV2 appends the four bytes "sAlT".
  if (aes) parts.push(Buffer.from([0x73, 0x41, 0x6c, 0x54]));
  return md5(...parts).subarray(0, Math.min(fileKey.length + 5, 16));
}

function aesCbcEncrypt(key, iv, data) {
  const cipher = createCipheriv(key.length === 32 ? 'aes-256-cbc' : 'aes-128-cbc', key, iv);
  return Buffer.concat([iv, cipher.update(data), cipher.final()]);
}

/**
 * Algorithm 2.B: revision 6's hardened hash.
 *
 * The loop runs at least 64 rounds and then keeps going until the last byte of
 * the AES output is small enough, which is what makes it expensive to brute
 * force and awkward to transcribe. `udata` is empty for the user password path
 * and the 48-byte `/U` entry for the owner path.
 */
function hash2B(password, salt, udata) {
  const pwd = Buffer.from(password, 'utf8');
  let k = sha(256, Buffer.concat([pwd, salt, udata]));
  let round = 0;
  let e;
  for (;;) {
    const block = Buffer.concat([pwd, k, udata]);
    const k1 = Buffer.concat(Array.from({ length: 64 }, () => block));
    const cipher = createCipheriv('aes-128-cbc', k.subarray(0, 16), k.subarray(16, 32));
    cipher.setAutoPadding(false);
    e = Buffer.concat([cipher.update(k1), cipher.final()]);
    let sum = 0;
    for (let i = 0; i < 16; i++) sum += e[i];
    k = sha([256, 384, 512][sum % 3], e);
    round++;
    if (round >= 64 && e[e.length - 1] <= round - 32) break;
  }
  return k.subarray(0, 32);
}

function aesNoPad(key, iv, data) {
  const cipher = createCipheriv(key.length === 32 ? 'aes-256-cbc' : 'aes-128-cbc', key, iv);
  cipher.setAutoPadding(false);
  return Buffer.concat([cipher.update(data), cipher.final()]);
}

/**
 * Build the `/Encrypt` dictionary and the object encryptor for one scheme.
 *
 * Returns `{ dictionary, encrypt }`, where `encrypt(bytes, objectNumber)`
 * produces the stored form of a stream or string in that object.
 */
export function standardSecurity({
  revision,
  keyLength = 128,
  userPassword = '',
  ownerPassword = '',
  permissions,
  aes = false,
}) {
  if (revision === 6) {
    const validationSalt = FIXED_SALTS.subarray(0, 8);
    const keySalt = FIXED_SALTS.subarray(8, 16);
    // The file key is arbitrary at revision 6: the passwords unlock it rather
    // than derive it, which is the point of the scheme.
    const fileKey = sha(256, Buffer.from('embedpdf conformance fixture key'));

    const u = Buffer.concat([
      hash2B(userPassword, validationSalt, Buffer.alloc(0)),
      validationSalt,
      keySalt,
    ]);
    const ue = aesNoPad(
      hash2B(userPassword, keySalt, Buffer.alloc(0)),
      Buffer.alloc(16),
      fileKey,
    );

    const ownerValidationSalt = FIXED_IV.subarray(0, 8);
    const ownerKeySalt = FIXED_IV.subarray(8, 16);
    const o = Buffer.concat([
      hash2B(ownerPassword, ownerValidationSalt, u),
      ownerValidationSalt,
      ownerKeySalt,
    ]);
    const oe = aesNoPad(hash2B(ownerPassword, ownerKeySalt, u), Buffer.alloc(16), fileKey);

    // Algorithm 8 step (a): P, then 0xFFFFFFFF, then the EncryptMetadata flag,
    // then "adb", then four bytes of anything. Encrypted with the file key in
    // ECB mode, which is the one place this scheme uses ECB.
    const perms = Buffer.concat([
      permissionsBytes(permissions),
      Buffer.from([0xff, 0xff, 0xff, 0xff]),
      Buffer.from('T'),
      Buffer.from('adb'),
      Buffer.from([0x00, 0x00, 0x00, 0x00]),
    ]);
    const permsCipher = createCipheriv('aes-256-ecb', fileKey, null);
    permsCipher.setAutoPadding(false);
    const permsEncrypted = Buffer.concat([permsCipher.update(perms), permsCipher.final()]);

    return {
      dictionary:
        `<< /Filter /Standard /V 5 /R 6 /Length 256 ` +
        `/CF << /StdCF << /CFM /AESV3 /AuthEvent /DocOpen /Length 32 >> >> ` +
        `/StmF /StdCF /StrF /StdCF ` +
        `/U <${u.toString('hex')}> /UE <${ue.toString('hex')}> ` +
        `/O <${o.toString('hex')}> /OE <${oe.toString('hex')}> ` +
        `/Perms <${permsEncrypted.toString('hex')}> ` +
        `/P ${permissions | 0} /EncryptMetadata true >>`,
      // Revision 6 uses the file key directly, with no per-object derivation.
      encrypt: (bytes) => aesCbcEncrypt(fileKey, FIXED_IV, bytes),
    };
  }

  const ownerEntry = computeOwnerEntry({ revision, keyLength, ownerPassword, userPassword });
  const fileKey = computeFileKey({
    revision,
    keyLength,
    userPassword,
    ownerEntry,
    permissions,
  });
  const userEntry = computeUserEntry({ revision, fileKey });

  const version = revision === 2 ? 1 : aes ? 4 : 2;
  const lengthEntry = revision === 2 ? '' : ` /Length ${keyLength}`;
  const cryptFilter = aes
    ? ` /CF << /StdCF << /CFM /AESV2 /AuthEvent /DocOpen /Length ${keyLength / 8} >> >>` +
      ` /StmF /StdCF /StrF /StdCF`
    : '';

  return {
    dictionary:
      `<< /Filter /Standard /V ${version} /R ${revision}${lengthEntry}${cryptFilter} ` +
      `/O <${ownerEntry.toString('hex')}> /U <${userEntry.toString('hex')}> ` +
      `/P ${permissions | 0} >>`,
    encrypt: (bytes, objectNumber) => {
      const key = objectKey({ fileKey, objectNumber, generation: 0, aes });
      return aes ? aesCbcEncrypt(key, FIXED_IV, bytes) : rc4(key, bytes);
    },
  };
}
