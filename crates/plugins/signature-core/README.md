# epdf-signature-core

`packages/core/signature` in Rust: parse, verify and build detached CMS signatures, the completion
gate, certificate chain validation, `sign()` and `validateSignatures()` over a document handle, and
the signer ports. All 297 cases in the five `vectors/core/signature-*.json` files replay identically
(`cargo test -p epdf-signature-core`): 116 parse cases, 70 for verify, build and the gate, 55 for
trust, 33 for signing and validation over a document, and 23 for the signers.

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
  validity that reaches 2050.

## Deliberate differences

- Malformed input outside the vectors can refuse with different text. pkijs validates the whole
  SignedData against one schema and reports "Object's schema was not verified against input data
  for SignedData"; this port applies the same structure and reports the same text, but asn1js's
  schema matcher has corners (extra members, constructed strings) it doesn't follow member for
  member.
- An ECDSA hash shorter than half the curve's field (SHA-256 over P-521) is left-padded before
  `verify_prehash`, which is what WebCrypto's integer conversion does; the `ecdsa` crate would
  otherwise refuse it.
- RSA keys go through `rsa::RsaPublicKey::new`, which refuses moduli above 4,096 bits. WebCrypto
  in Node accepts up to 16,384. Such a signature verifies as `invalid` here.

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
`rsaEncryption` signature algorithm, and 25 chains that exercise the chain engine's policy and
name-constraint half.

Mutation testing has not been run on this crate yet.
