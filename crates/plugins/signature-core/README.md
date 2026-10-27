# epdf-signature-core

`packages/core/signature` in Rust: parse, verify and build detached CMS signatures, the completion
gate, certificate chain validation, `sign()` and `validateSignatures()` over a document handle, and
the signer ports. Document timestamps (`sign` with `kind: 'timestamp'`, `cms/timestamp.ts`, the
`timestamp` gate profile and `createTestTimestampAuthority`) port too. No vector signs a timestamp,
so `tests/timestamp.rs` checks them against the refusal texts the TypeScript gives for the same
inputs. All 460 cases in the five `vectors/core/signature-*.json` files replay identically
(`cargo test -p epdf-signature-core`): 204 parse cases, 97 for verify, build and the gate, 96 for
trust, 35 for signing and validation over a document, and 28 for the signers.

## What ports and what the host supplies

The TypeScript is thin over two libraries, pkijs 3.4.0 and asn1js, and over WebCrypto. The port
takes the libraries' behaviour with it, because a pkijs refusal text or a pkijs quirk is part of
what `packages/core/signature` returns. `der.rs` is the slice of asn1js the package leans on (BER
in, DER out, a SET never sorted), `cert.rs` reads certificates as `pkijs.Certificate` does,
`crypto.rs` is `CryptoEngine.verifyWithPublicKey`, and `trust.rs` with `policy.rs` is
`CertificateChainValidationEngine`.

WebCrypto itself is the host's. `signers::WebCrypto` is the four calls the package and pkijs make:
`generateKey`, `sign`, `exportKey('jwk')` and `getRandomValues`. Everything around them ports: the
sign parameters, the self-signed certificate's DER, pkijs's JWK-to-SPKI conversion. Verification
does not go through the host; it uses the RustCrypto `rsa`, `p256`, `p384` and `p521` crates.
Trust anchors (`trust::TrustPort`) and key storage (`signers::PersonalKeyStore`) are host traits
as they are ports in TypeScript. `indexedDbKeyStore` does not port: it is a browser adapter over
the same store protocol, and a native host writes its own over the keychain it has.

The vectors drive the WebCrypto adapters with a scripted WebCrypto on both sides (`harness.rs`,
twin of `withWebCrypto` in the suite). `generateKey` hands out fixture keys, `sign` signs
RSASSA-PKCS1-v1_5 for real and answers RSA-PSS and ECDSA with fixed bytes, `getRandomValues` fills
with `0x5a`, and every call is logged with its parameters and the exact bytes signed. That pins the
certificate TBS byte for byte even where the signature over it is fixed.

## pkijs behaviour the vectors pin

These are recorded rather than fixed; the TypeScript is the oracle.

- The chain engine starts from the LAST certificate the CMS carries. `parseCmsInternal` orders the
  signer first, so any CMS carrying two or more certificates validates the chain from the last of
  the others, and the signer is never on the path. `trust-expired-leaf-not-checked` is an expired
  signer with a valid intermediate, reported trusted.
- A carried certificate that is itself an anchor is a path of itself twice
  (`trust-intermediate-anchor-carried`, `trust-self-signed-anchor`).
- Verification hashes with the CMS digest algorithm and never reads the signature algorithm's own
  hash for RSA or ECDSA, so an MD5-with-RSA signature algorithm over a SHA-256 digest verifies
  (`verify-unknownSignatureAlgorithm`).
- An attribute with an empty value SET throws `TypeError: Cannot read properties of undefined
  (reading '0')` out of `parseDetachedCms`, which the gate reports as `malformed` with that text
  (`gate-malformed-type-error`).
- A critical extension pkijs does not know passes the CA check as long as its value is BER
  (`trust-ca-unknown-critical`).
- A policy mapping whose issuer-domain policy no certificate lists, or a mapping on a CA whose
  child lists no policies, fails with a `TypeError` text as the chain's reason
  (`trust-policy-mappingUnknownIssuer`, `trust-policy-mappingNoPolicies`).
- `selfSignedCertificate` writes both validity times as UTCTime, which reads back as 19xx for a
  validity that reaches 2050. From 2100 on, and before 1890, asn1js's `padNumber` drops the year
  altogether (`self-signed-validity-past-2100`, `build-signing-times`).
- A GeneralizedTime year from 0000 to 0099 goes through `Date.UTC` and reads as 1900 to 1999
  (`parse-signingTimeYear0050`).
- An OBJECT IDENTIFIER whose last byte carries the continuation bit fails the whole BER parse
  (`parse-signatureOidTruncated`).
- asn1js 3.0.10's `Integer.convertFromDER` drops one leading zero whatever follows it, so an ECDSA
  `r` padded to one byte past the curve size still verifies (`verify-ecdsaRPadded`).
- WebCrypto verifies RSA-PSS with the hash the key was imported with. For a CMS signer that is the
  digest algorithm; the parameters' hash is only looked up. A SHA-1 PSS signature under a SHA-256
  digest is `invalid`, and SHA-1 parameters over a SHA-256 signature are `valid`
  (`verify-pssSha1`, `verify-pssSha1ParamsOverSha256`).
- RSA moduli up to 16,384 bits verify, and a longer one imports but never verifies, as Node's
  WebCrypto behaves (`crypto.rs:126`, `tests/adversarial.rs:108,118`). The `rsa` crate's own
  default stops at 4,096 bits, so the limit is set explicitly.

## Deliberate deviations from the TypeScript

- Every BER read the TypeScript makes with `asn1js.fromBER` (the CMS, a TSTInfo, a certificate,
  a subject key identifier) first asks `asn1js::from_ber` whether asn1js 3.0.10 would accept it.
  That walk follows asn1js's `localFromBER` byte for byte: its 100-level and 10,000-element
  limits, the elements it counts while speculatively decoding each primitive OCTET STRING (and
  each BIT STRING whose unused-bits octet is 0), its identifier, length and value-block refusals,
  and the readers that throw (a GeneralizedTime `fromString` cannot read, a BMPString of odd
  length, a UniversalString whose length is not a multiple of 4). `tests/asn1js_parity.rs`
  carries the TypeScript's verdicts: re-encodings of `rsaCades`, the node limit over a whole CMS
  (9,817 extra NULLs read, 9,818 do not) and a TSTInfo (9,989 NULL parameters read, 9,990 do
  not), and the depth limit. Against 47,413 fuzzed and limit-straddling inputs the walk and
  `asn1js.fromBER` gave the same answer every time. The signature, message-digest, ESS certHash
  and certificate `extnValue` read as pkijs reads them: a constructed signature or `extnValue`
  fails pkijs's schema, and a constructed message-digest or certHash reads as no bytes, which is
  `valueHexView` of a constructed OCTET STRING. `der` still builds the tree. It keeps a constructed string's content as bytes, as asn1js's
  `HexBlock` does (an indefinite one's runs to the end of its input). Where its tree and
  asn1js's still differ it is the stricter one: it refuses a constructed element whose last
  member runs past the declared length, which asn1js reads on while the buffer lasts. One throw
  is not modelled:
  `String.fromCharCode.apply` over a string of more than about 120,000 octets throws a
  RangeError at a threshold set by the host's stack.
- `timestamp::read_timestamp_info` reads a TSTInfo as `new pkijs.TSTInfo({ schema })` does. The
  schema match follows asn1js's `compareSchema`, including its rule that a trailing member no
  optional slot can place fails the whole schema, and genTime goes through a port of asn1js's
  `GeneralizedTime.fromString`. `tests/timestamp.rs` replays 66 TSTInfo inputs with the verdicts
  pkijs 3.4.0 and asn1js 3.0.10 gave for them. A GeneralizedTime without `Z` is local time to
  asn1js, so its value depends on the host's time zone; this reads it as a host on UTC does.
  Accepting or refusing it doesn't depend on the zone, and the gate never reads the time.
- Three reads still differ from pkijs, none reached by a vector. An extension's value is read
  lazily by pkijs (`Extension.parsedValue`, pkijs 3.4.0 `build/index.js:7441-7447`); here
  `Extension::parsed` and the chain engine's key identifiers (`cert.rs:117`, `trust.rs:214`) go
  through `der` without asking `asn1js::from_ber`, so an extension asn1js refuses or throws on
  can still be read. The timestamp token is the unsigned attribute's bytes as they arrived,
  where TS re-encodes them with `toBER(false)`, so a token with non-DER lengths reaches the
  timestamp check unchanged here and normalised there. And `signingTime` takes its value from
  `Node::time`, not asn1js's `toDate()`: accept and refuse match through the walk, but a
  GeneralizedTime asn1js reads oddly (no `Z`, a comma fraction) can give a different instant.
- A constructed eContent joins its primitive pieces only, as asn1js's `OctetString.getValue()`
  does: a nested constructed piece adds no bytes.
- An ECDSA hash shorter than half the curve's field (SHA-256 over P-521) is left-padded before
  `verify_prehash`, which is what WebCrypto's integer conversion does; the `ecdsa` crate would
  otherwise refuse it.
- Two CA certificates that issue each other send pkijs's `buildPath` into a recursion that never
  returns: `validateChain` hangs. A straight port overflows the stack instead, which aborts the
  process, and a hostile PDF can carry such a chain. `ChainEngine::build_path` refuses an issuer
  it was reached from, and the chain is `untrusted` with "Certificate issuers form a cycle". No
  vector can pin a hang, so `tests/chain_cycle.rs` checks this over the `cyclicIssuers` fixture;
  before the guard, that test aborted with a stack overflow. The cost: a chain pkijs would never
  finish gets a verdict here that the TypeScript cannot confirm.

## Coverage

The generators reach 1,264 of the package's 1,314 statements and 295 of its 301 branches. What
they can't reach:

- `cms/engine.ts:13-19`: registering WebCrypto with pkijs when it has no engine. pkijs registers
  `globalThis.crypto` itself when it loads under Node, so the branch never runs there.
- `cms/engine.ts:36-40`: `toHex`, which nothing in the package calls.
- `cms/verify.ts:12`, the `?? []` for a signer without signed attributes: the parse refuses one
  first. And `verify.ts:52`, an error without a `message`: every error the verification path
  throws has one.
- `signers/personal.ts:82-119`: `indexedDbKeyStore`, which needs a browser.
- `trust.ts:64`, `68` and `70-72`: the chain engine's empty `resultMessage` fallback, a missing
  `certificatePath`, and a throwing `verify()`. The engine catches everything itself, and every
  failure it returns carries a message.

The fixture set is `tooling/plugin-conformance/fixtures/signature/fixtures.json`, generated once
by `generate.ts` beside it (the README there says why). It includes three CMS blobs from third-party
signers in `packages/engine/main/test/fixtures`, one of them with the OpenSSL-style
`rsaEncryption` signature algorithm, and 49 chains, up to four CAs deep, that exercise the chain
engine's policy and name-constraint half.

## Mutation testing

These counts predate the Task 13.9 catch-up (document timestamps, `CompletionProfile`, the
harness's `pkcs7` fallback for an unknown gate profile) and are stale until Task 14 reruns them.

`scripts/mutants.sh -p epdf-signature-core --test-package epdf-signature-core --file <file>`, file by
file over every module but `harness.rs`, catches 1,781 of 1,824 viable mutants (1,777 caught, 4
timeouts; 91 unviable). `verdict.rs` was run again after `validate_signatures` moved to JavaScript
truthiness for `signed`: 57 of 57 caught, 11 more than before the fix. 169 of the caught were killed by cases added for this count, each confirmed
by applying the mutant by hand. Six more sat in code since replaced and are not counted: three in
`integer_from_der`, rewritten to match asn1js (above), and three in `number_text`, which now calls
`epdf_geometry::js::number_to_string`. The same pass fixed four other divergences the new cases
exposed: the OID termination check in `der::read`, the two-digit GeneralizedTime year, the UTCTime
year `padNumber` drops, and the RSA-PSS hash. None of the 43 survivors lets a bad signature or chain
pass.

`cert.rs`

- `AlgId::encode` (3) and `Certificate::subject_key_identifier` (4): nothing in the workspace calls
  either.

`cms.rs`

- The `"cades-b"` arm of `Profile::from_name`: its only caller is the harness, whose fallback is
  `Profile::CadesB`.

`crypto.rs`

- Deleting `!` in `curve_of`: with parameters missing, both versions throw, only with different
  text, and neither text marks the signature `unsupported`.

`der.rs`

- `|` to `^` in `decode_oid`, `context` and `oid` (3): the bits OR'd in never overlap the ones
  already set.
- `-` to `+` and `/` in `decode_oid`'s truncation check (2): `read` now refuses such an OID before
  anything decodes it.
- `||` to `&&` in `integer`: every caller passes a value from 1 to 127.
- `-` to `+` and `/` in `days_from_civil`'s negative-era branch (2): parsed years are 1900 or later.
- `generalized_time` returning `vec![]`, `vec![0]` or `vec![1]` (3): nothing calls it.

`policy.rs`

- `explicit_start` starting at `1` instead of `-1`: it is only read when `explicit` is set, which
  always sets it.
- The `!inhibit_any` guard as `true`: once set, `inhibit_any` never clears, and the pending count
  it would update is never read again.
- `>` to `>=` in the three minimum updates (lines 270, 278 and 290), and `+` to `*` in line 278's
  comparison: they differ only where the value assigned equals the value held.
- `&&` to `||` in the mapping and any-policy countdowns (2): the extra decrements start from `0`
  and go negative until a constraint resets them, or run after the flag is already set.
- `<` to `<=` in the anyPolicy clearing loop: `explicit_start` is always below `slots`.
- The six mutants of the anyPolicy early exit on line 370: slots before `explicit_start` are
  already cleared for anyPolicy, so it is never found there either way, and a found anyPolicy
  implies every other policy is found, so `authConstrPolicies` is empty in both or neither.
- `==` to `!=` on line 387: `userConstrPolicies` is only tested for emptiness, and a one-element
  `authConstrPolicies` is non-empty either way.

`signers.rs`

- `||` to `&&` in `spki_from_jwk`'s RSA check, and the five mutants of the EC coordinate padding:
  WebCrypto always exports `n`, `e` and full-length coordinates, on both sides of the harness.

`trust.rs`

- The self-signed guard in `verify_signature` as `true`: its only call without an issuer is
  already behind the same test.
