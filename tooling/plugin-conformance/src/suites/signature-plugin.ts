/**
 * `packages/plugin/signature`: the act of signing, document-scoped. Reads
 * over the engine's signature snapshot, one-shot and two-phase signing,
 * visual fills, validation of the working copy with its invalidation edge,
 * the live document events, the sign-here target, the destination rule, and
 * the armed mark dropped on a field. Replayed by
 * `crates/plugins/signature/tests/conformance.rs`.
 *
 * Harness shims, each mirrored by `crates/plugins/signature/src/harness.rs`:
 *
 *   - `console.error`. While a signature factory is current, a call whose
 *     message starts `[signature]` logs as the callback `console.error`
 *     (the Rust port takes it as `SignatureHost::console_error`).
 *   - Ports from the vector. `config.signer` is `{kind: 'cms', answer}` or
 *     `{kind: 'raw', certificateChain, algorithm, hash, answer}`, whose
 *     `sign` logs as `signer.sign` and answers with `answer` (an
 *     `AnswerSpec` whose `resolves` may carry `$bytes`); `thunk: true` wraps
 *     it in a function that logs `signer.resolve` and resolves it.
 *     `config.trust` is `{anchors}`, an `AnswerSpec` logged as
 *     `trust.anchors`. A per-call signer rides the call's arguments as a
 *     scripted `$fn`, which the runner decodes itself.
 *   - `config.shape: 'controller'` builds the shipping controller and runs its connect.
 *
 * WebCrypto answers from libuv's pool (`suites/signature-core.ts`), so only
 * cases whose call reaches the completion gate or a CMS build set
 * `awaitCalls`, and every validation the plugin starts on its own judges a
 * snapshot whose revision chain is broken, which never reads a CMS.
 */
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

import {
  formWidget as currentFormWidget,
  type SignatureDTO,
  type SignatureCompleteResult,
} from '@embedpdf/engine-core/runtime';

import { definePlugin, type AnyPlugin } from '@embedpdf/core';

import { signaturePlugin } from '../../../../packages/plugin/signature/src/signature.plugin';
import { SIGNATURE_DEFAULTS, SignatureToken } from '../../../../packages/plugin/signature/src/contract';
import type { SignatureConfig,
  SignatureCapability,
} from '../../../../packages/plugin/signature/src/contract';
import { createSignatureController } from '../../../../packages/plugin/signature/src/controller';
import * as model from '../../../../packages/plugin/signature/src/model';
import { initialSignatureState, sameFieldRef } from '../../../../packages/plugin/signature/src/model';
import type { SignatureState } from '../../../../packages/plugin/signature/src/model';
import { blankPagePdf } from '../../../../packages/plugin/signature/src/blank-page';
import { markRoleOf } from '../../../../packages/plugin/signature/src/contract';
import { FormToken } from '../../../../packages/plugin/form/src/contract';
import { AnnotationToken } from '../../../../packages/plugin/annotation/src/contract';
import { InteractionToken } from '../../../../packages/plugin/interaction/src/host-contract';
import { StampToken } from '../../../../packages/plugin/stamp/src/contract';

import { fieldAnswer, deleteAnswer } from './form-live-fixtures';
import { signature as currentSignatureField } from './form-current-fixtures';
import {
  materialize,
  confirmation,
  created as currentCreated,
  updated as currentUpdated,
  deleted as currentDeleted,
  square as currentSquare,
} from './annotation-live-fixtures';
import { canonical } from '../canonical';
import { makeError, type FakeDocumentSpec, type ResponseSpec } from '../fake-engine';
import { onCaseEnd, registerPlugin, type PluginEnv, type ScenarioCase, type Step } from '../scenario';
import type { FunctionCase, Suite } from '../vectors';

const sameRef = sameFieldRef;
import type { AnswerSpec } from './acrojs';
import { created, hydrate, stamp as stampDto } from './annotation-fixtures';
import { annotations, list, ref, fqn, signature as sigField, text, widgetAnnot, PAGES, sample } from './form-fixtures';
import './interaction';
import { assetDoc, assetPage, opens, PDF, png, withWorkshop } from './stamp-fixtures';

/** Why these cases record no state; see `ScenarioCase` in `../scenario`. */
const UNSNAPSHOTTED = {
  snapshot: 'none',
  reason:
    'Pins authority twins, revision reads, placement by mode and the armed-mark handler, through call outcomes, settlements and engine calls. Its dispatched actions are compared; the state they build is not.',
} as const;

// ── fixtures ──────────────────────────────────────────────────────────────────

interface Fixtures {
  certs: Record<string, string>;
  cms: Record<string, string>;
}
const fixtures = JSON.parse(
  readFileSync(join(dirname(fileURLToPath(import.meta.url)), '../../fixtures/signature/fixtures.json'), 'utf8'),
) as Fixtures;
const $b = (b64: string) => ({ $bytes: b64 });
const cmsArg = (name: string) => $b(fixtures.cms[name]);
const certArg = (name: string) => $b(fixtures.certs[name]);
const digestBytes = (n: number, seed: number) => new Uint8Array(n).map((_, i) => (i * 7 + seed) & 0xff);
const GOOD_DIGEST = $b(Buffer.from(digestBytes(32, 3)).toString('base64'));

// ── the harness factory ───────────────────────────────────────────────────────

let signatureLog: PluginEnv['log'] | null = null;
onCaseEnd(() => {
  signatureLog = null;
});
const previousError = console.error;
console.error = (...args: unknown[]) => {
  if (signatureLog && typeof args[0] === 'string' && args[0].startsWith('[signature]')) {
    signatureLog('console.error', args);
    return;
  }
  previousError(...args);
};

/** `{$bytes}` anywhere in a scripted answer, as bytes. */
const decodeBytes = (value: unknown): unknown => {
  if (Array.isArray(value)) return value.map(decodeBytes);
  if (value && typeof value === 'object') {
    if ('$bytes' in value) return new Uint8Array(Buffer.from((value as { $bytes: string }).$bytes, 'base64'));
    return Object.fromEntries(Object.entries(value).map(([k, v]) => [k, decodeBytes(v)]));
  }
  return value;
};

function answering(name: string, spec: AnswerSpec, env: PluginEnv): (...args: unknown[]) => Promise<unknown> {
  return async (...args: unknown[]) => {
    env.log(name, args);
    if (spec.rejects) throw makeError(spec.rejects);
    return decodeBytes(spec.resolves);
  };
}

interface SignerSpec {
  readonly kind: 'cms' | 'raw';
  readonly answer: AnswerSpec;
  readonly certificateChain?: readonly { $bytes: string }[];
  readonly algorithm?: string;
  readonly hash?: string;
  readonly thunk?: boolean;
}

interface SignatureSpec {
  readonly mode?: SignatureConfig['mode'];
  readonly allowCertify?: boolean;
  readonly signer?: SignerSpec;
  readonly trust?: { readonly anchors: AnswerSpec };
  readonly shape?: 'controller';
}

function signerOf(spec: SignerSpec, env: PluginEnv): NonNullable<SignatureConfig['key']> {
  const sign = answering('signer.sign', spec.answer, env);
  const port =
    spec.kind === 'cms'
      ? { kind: 'cms' as const, sign: sign as never }
      : {
          kind: 'raw' as const,
          certificateChain: (spec.certificateChain ?? []).map((c) => decodeBytes(c) as Uint8Array),
          algorithm: spec.algorithm as never,
          hash: spec.hash as never,
          sign: sign as never,
        };
  if (!spec.thunk) return port;
  return async () => {
    env.log('signer.resolve', []);
    return port;
  };
}

function configOf(spec: SignatureSpec, env: PluginEnv): SignatureConfig {
  return {
    ...(spec.mode !== undefined ? { mode: spec.mode } : {}),
    ...(spec.allowCertify !== undefined ? { allowCertify: spec.allowCertify } : {}),
    ...(spec.signer ? { key: signerOf(spec.signer, env) } : {}),
    ...(spec.trust ? { trust: { anchors: answering('trust.anchors', spec.trust.anchors, env) as never } } : {}),
  };
}

registerPlugin('signature', {
  factory: (config, env) => {
    signatureLog = env.log;
    const spec = (config ?? {}) as SignatureSpec;
    if (config === undefined) return signaturePlugin() as AnyPlugin;
    if (spec.shape === undefined) return signaturePlugin(configOf(spec, env)) as AnyPlugin;
    // `signature.plugin.ts`'s definition over the unit-test shapes.
    // A definition's initial state is `state` since 4fc1a32af, and transitions
    // replace `reduce`; settings come from the definition, as in the plugin.
    return definePlugin({
      id: 'signature',
      token: SignatureToken,
      scope: 'document',
      requires: [FormToken],
      optional: [InteractionToken, StampToken, AnnotationToken],
      state: initialSignatureState,
      settings: { defaults: SIGNATURE_DEFAULTS, whole: ['key', 'trust'] },
      create: (ctx) => createSignatureController(ctx as never),
    }) as unknown as AnyPlugin;
  },
  tokens: [SignatureToken],
});

registerPlugin('signature-annotation', {
  factory: (_, env) =>
    definePlugin({
      id: 'signature-annotation',
      token: env.token('signature-annotation'),
      scope: 'document',
      requires: [AnnotationToken],
      create: (ctx) => ({
        api: {
          arm: (
            ...args: Parameters<
              import('../../../../packages/plugin/annotation/src/contract').AnnotationCapability['stamps']['arm']
            >
          ) => ctx.get(AnnotationToken).stamps.arm(...args),
          isArmed: () => ctx.get(AnnotationToken).stamps.isArmed(),
        },
      }),
    }),
});

// ── documents ─────────────────────────────────────────────────────────────────

type Dto = Record<string, unknown>;

/** A signature DTO: field `n`, widget `100 + n` on page 1. */
const dto = (index: number, n: number, name: string, over: Partial<SignatureDTO> = {}): SignatureDTO => ({
  index,
  field: ref(n),
  fieldName: name,
  widget: currentFormWidget(100 + n, { kind: 'objectNumber', objectNumber: 1 }),
  signed: false,
  kind: 'signature',
  filter: null,
  subFilter: null,
  byteRange: null,
  contentsSize: 0,
  coverage: null,
  revisionIndex: null,
  signer: { name: null, location: null, reason: null, contactInfo: null, signedAt: null },
  docMdp: null,
  catalogCertification: false,
  fieldMdp: null,
  lock: null,
  seedValue: null,
  ...over,
});
const signedDto = (index: number, n: number, name: string, over: Partial<SignatureDTO> = {}): SignatureDTO =>
  dto(index, n, name, {
    signed: true,
    filter: 'Adobe.PPKLite',
    subFilter: 'ETSI.CAdES.detached',
    byteRange: [0, 100, 8292, 50],
    contentsSize: 8192,
    coverage: 'whole-revision',
    revisionIndex: 0,
    signer: { name: 'Conformance Signer',
      location: null,
      reason: null,
      contactInfo: null,
      signedAt: null,
    },
    ...over,
  });

/** Sig1 signed, Sig2 empty. The chain is broken, so a validation never
 *  reads a CMS (see the header). */
const SIG1 = signedDto(0, 1, 'Sig1');
const SIG2 = dto(1, 2, 'Sig2');
const snap = (signatures: readonly unknown[], over: Dto = {}) => ({
  chainValid: false,
  revisions: [{ index: 0 }],
  signatures,
  protection: {
    enforced: null,
    judged: null,
    certification: null,
    fieldLocks: [],
    policyVersion: 1,
  },
  ...over,
});
const listed = (signatures: readonly unknown[], over: Dto = {}, extra: Partial<ResponseSpec> = {}): ResponseSpec => ({
  path: 'signatures.list',
  value: snap(signatures, over),
  ...extra,
});

const analysis = (verdict: string, basis = 'working-copy', primary: unknown = null): ResponseSpec => ({
  path: 'signatures.analyze',
  value: { current: { verdict, primary }, basis: { source: basis }, later: { revisionCount: 0, undoneObjectNumbers: [] } },
});
const PERMITTED = analysis('permitted');

/** Form fields: Sig1 (101), Sig2 (102), a text field (103), and Sig4 (104)
 *  whose value is set although it is not signed. */
const FIELDS = [
  sigField(1, 'Sig1'),
  sigField(2, 'Sig2'),
  text(3, 'Name'),
  sigField(4, 'Sig4', { valueEntry: { kind: 'scalar', value: 'x' } }),
];
const rect = (bottom: number) => ({ left: 100, bottom, right: 260, top: bottom + 24 });
const WIDGETS = [widgetAnnot(1, 101, rect(600)), widgetAnnot(1, 102, rect(500)), widgetAnnot(1, 103, rect(400)), widgetAnnot(1, 104, rect(300))];

interface DocOptions {
  readonly signatures?: readonly unknown[];
  readonly services?: string[];
  readonly lacks?: string[];
  readonly allows?: string[] | '*';
  /** Tried before the defaults. */
  readonly responses?: ResponseSpec[];
  readonly fields?: unknown[];
}

const docOf = (o: DocOptions = {}): FakeDocumentSpec => ({
  pages: PAGES,
  services: o.services ?? ['signatures'],
  ...(o.lacks ? { lacks: o.lacks } : {}),
  security: { allows: o.allows ?? '*' },
  responses: [
    ...(o.responses ?? []),
    hydrate({ 1: WIDGETS }),
    { path: 'annotations.list', value: { annotations: WIDGETS } },
    list(o.fields ?? FIELDS),
    annotations(1, WIDGETS),
    listed(o.signatures ?? [SIG1, SIG2]),
    PERMITTED,
    { path: 'forms.setSignatureAppearance' },
    { path: 'signatures.cancel' },
  ],
});

// ── steps ─────────────────────────────────────────────────────────────────────

const ORIGIN = { kind: 'local', sessionId: 's1', sub: null, ts: 0, serverId: null } as const;
const start: Step = { op: 'call', target: 'kernel', method: 'start' };
const open = (id = 'a'): Step => ({ op: 'call', target: 'documents', method: 'open', args: [{ kind: 'bytes', id, bytes: { $bytes: '' } }] });
const close = (id = 'a'): Step => ({ op: 'call', target: 'documents', method: 'close', args: [id] });
const boot = [start, open()];
const sig = (method: keyof SignatureCapability, ...args: unknown[]): Step => ({ op: 'call', target: { token: 'signature', doc: 'a' }, method, args });
const st = (method: string, ...args: unknown[]): Step => ({ op: 'call', target: { token: 'stamp' }, method, args });
const ix = (method: string, ...args: unknown[]): Step => ({ op: 'call', target: { token: 'interaction', doc: 'a' }, method, args });
const anno = (method: 'arm' | 'isArmed', ...args: unknown[]): Step => ({ op: 'call', target: { token: 'signature-annotation', doc: 'a' }, method, args });
const emit = (event: Dto): Step => {
  const field = currentSignatureField(2, 'Sig2');
  let value: unknown = event;
  switch (event.type) {
    case 'forms.created':
      value = fieldAnswer('forms.create', 900, field).events[0];
      break;
    case 'forms.updated':
    case 'forms.valueSet':
      value = { ...fieldAnswer('forms.update', 900, field).events[0], type: event.type };
      break;
    case 'forms.widgetAdded':
      value = fieldAnswer('forms.addWidget', 900, field).events[0];
      break;
    case 'forms.widgetRemoved':
      value = fieldAnswer('forms.removeWidget', 900, field).events[0];
      break;
    case 'forms.deleted':
      value = deleteAnswer(900, field).events[0];
      break;
    case 'forms.repaired':
      value = {
        type: event.type,
        acroformCreated: false,
        fieldsLinked: 0,
        widgetsLinked: 0,
        fieldsUnrepairable: 0,
        appearancesBaked: 0,
        needsAppearancesCleared: false,
        meta: { affectedPages: [], cacheDelta: null, opId: 'repair', undoable: false },
        origin: ORIGIN,
      };
      break;
    case 'annotations.created':
      value = confirmation(900, [{ item: currentCreated(currentSquare(900)) }]).events[0];
      break;
    case 'annotations.updated':
      value = confirmation(900, [{ item: currentUpdated(currentSquare(900)) }]).events[0];
      break;
    case 'annotations.deleted':
      value = confirmation(900, [
        { item: currentDeleted(currentSquare(900).ref), changed: [currentSquare(900).ref] },
      ]).events[0];
      break;
    case 'annotations.reordered':
      value = {
        type: event.type,
        page: currentSquare(900).page,
        order: [currentSquare(900).ref],
        meta: {
          affectedPages: [currentSquare(900).page],
          cacheDelta: null,
          opId: 'reorder',
          undoable: true,
          changed: [currentSquare(900).ref],
        },
        origin: ORIGIN,
      };
      break;
    case 'signatures.prepared':
      value = { ...prepared(), ...event, origin: ORIGIN };
      break;
    case 'signatures.cancelled':
      value = { ...event, origin: ORIGIN };
      break;
    case 'document.versioned':
      value = { ...event, version: { sha256: 'new', byteLength: 10 }, origin: ORIGIN };
      break;
  }
  return { op: 'emit', doc: 'a', event: value };
};
const advance = (ms: number): Step => ({ op: 'advance', ms });
const settle = (label: string, value?: unknown): Step =>
  value === undefined ? { op: 'settle', label } : { op: 'settle', label, value };
const drain: Step = { op: 'drain' };
const fn = (name: string, over: Dto = {}) => ({ $fn: name, ...over });
const listen = (hook: keyof SignatureCapability, name = hook.slice(2)) => sig(hook, fn(name));
const HOOKS = [
  'onSigned',
  'onFilled',
  'onCleared',
  'onValidated',
  'onProtectionChanged',
  'onInvalidationPredicted',
  'onTargetChanged',
  'onSignRequested',
  'onInspectionRequested',
] satisfies (keyof SignatureCapability)[];
const listenAll = HOOKS.map((h) => listen(h));

const PNG = png(100, 50);
const MARK = { source: PNG };
const FORM_SIG = (config?: SignatureSpec) => [{ factory: 'interaction' }, { factory: 'form' }, config === undefined ? { factory: 'signature' } : { factory: 'signature', config }];
const FULL = (config?: SignatureSpec) => [
  { factory: 'interaction' },
  { factory: 'annotation' },
  { factory: 'signature-annotation' },
  { factory: 'form' },
  { factory: 'stamp' },
  config === undefined ? { factory: 'signature' } : { factory: 'signature', config },
];

const CMS_SIGNER: SignerSpec = { kind: 'cms', answer: { resolves: cmsArg('rsaCades') } };
const prepared = (over: Dto = {}) => ({
  signingId: 'signing-1',
  digest: GOOD_DIGEST,
  algorithm: 'sha256' as const,
  byteRange: [0, 100, 8292, 50] as [number, number, number, number],
  contentsSize: 8192,
  subFilter: 'ETSI.CAdES.detached' as const,
  expectedVersion: { baseSha256: 'base', editsVersion: 0 },
  expiresAt: null,
  ...over,
});
const completed = (n = 2, name = 'Sig2'): SignatureCompleteResult => ({
  status: 'completed',
  signature: signedDto(1, n, name),
  version: { sha256: 'new', byteLength: 10 },
  previous: { baseSha256: 'base', editsVersion: 0 },
  protection: {
    enforced: null,
    judged: null,
    certification: null,
    fieldLocks: [],
    policyVersion: 1,
  },
  meta: { affectedPages: [], cacheDelta: null, opId: 'signing-1', undoable: false },
});
/** After a signing: Sig2 signed too. */
const BOTH_SIGNED = [SIG1, signedDto(1, 2, 'Sig2')];

// ── reads ─────────────────────────────────────────────────────────────────────

const SIG3_FQN = dto(2, 0, 'Sig3', { field: fqn('Sig3'), widget: null });

const reads: ScenarioCase[] = [
  {
    name: 'reads-over-the-snapshot',
    covers:
      "The initial read lands the snapshot and, since something is signed, a validation of the working copy. The lists are memoized per snapshot. getSignature finds a field by ref (sameRef, which a mixed pair never matches), by name, by a widget's object number or annotation ref (a widget the snapshot doesn't carry is looked up through the form plugin), and answers null for an unknown address or an address that is neither. getVerdict follows the last validation.",
    plugins: FORM_SIG(),
    engine: { documents: { a: docOf({ signatures: [SIG1, dto(1, 2, 'Sig2', { widget: null }), SIG3_FQN] }) } },
    steps: [
      ...boot,
      sig('getSnapshot'),
      sig('getStatus'),
      sig('listSignatures'),
      sig('listSignatures'),
      sig('listUnsignedFields'),
      sig('getSignature', ref(1)),
      sig('getSignature', fqn('Sig2')),
      sig('getSignature', fqn('Nope')),
      sig('getSignature', ref(9)),
      sig('getSignature', { annotObjectNumber: 101 }),
      sig('getSignature', { kind: 'objectNumber', page: { kind: 'objectNumber', objectNumber: 1 }, annotObjectNumber: 101 }),
      sig('getSignature', { annotObjectNumber: 102 }),
      sig('getSignature', { annotObjectNumber: 999 }),
      sig('getSignature', { kind: 'name', name: 'Sig1' }),
      sig('listVerdicts'),
      sig('getVerdict', ref(1)),
      sig('getVerdict', ref(2)),
      sig('getVerdict', ref(9)),
      sig('getProtection'),
      sig('getPending'),
      sig('isBusy'),
      sig('getMode'),
      sig('getTarget'),
      sig('canSign', ref(2)),
      sig('canFill', ref(2)),
      sig('canCertify'),
    ],
  },
  {
    name: 'signed-by-truthiness',
    covers:
      "The lists split on the truthiness of `signed`, whatever its type: null, 0 and '' are unsigned; 1, 'x', {} and [] are signed.",
    plugins: FORM_SIG(),
    engine: {
      documents: {
        a: docOf({
          signatures: [
            { ...dto(0, 1, 'Null'), signed: null },
            { ...dto(1, 2, 'Zero'), signed: 0 },
            { ...dto(2, 3, 'Empty'), signed: '' },
            { ...dto(3, 4, 'One'), signed: 1 },
            { ...dto(4, 5, 'Text'), signed: 'x' },
            { ...dto(5, 6, 'Object'), signed: {} },
            { ...dto(6, 7, 'Array'), signed: [] },
          ],
        }),
      },
    },
    ...UNSNAPSHOTTED,
    steps: [...boot, sig('listSignatures'), sig('listUnsignedFields')],
  },
  {
    name: 'get-signature-edges',
    covers:
      "A widget the snapshot carries but the form plugin doesn't know is found by the snapshot alone. An objectNumber address with neither a widget nor a field object number is not a field ref, so it answers null even against a signature whose field ref lacks the number too.",
    plugins: FORM_SIG(),
    engine: {
      documents: {
        a: docOf({
          signatures: [
            SIG1,
            dto(1, 2, 'Sig2', { widget: currentFormWidget(150, { kind: 'objectNumber', objectNumber: 1 }) }),
            { ...dto(2, 0, 'Odd'), field: { kind: 'objectNumber' }, widget: null },
          ],
        }),
      },
    },
    ...UNSNAPSHOTTED,
    steps: [...boot, sig('getSignature', { annotObjectNumber: 150 }), sig('getSignature', { kind: 'objectNumber' })],
  },
  {
    name: 'nothing-signed-reads',
    covers:
      'A snapshot with nothing signed lands without a validation, so getVerdict answers null from no verdicts at all.',
    plugins: FORM_SIG(),
    engine: { documents: { a: docOf({ signatures: [SIG2] }) } },
    snapshot: ['signature::a'],
    steps: [...boot, sig('getVerdict', ref(2)), sig('listVerdicts'), sig('listSignatures'), sig('listUnsignedFields')],
  },
  {
    name: 'signature-service-refuses-unsupported',
    covers:
      "Without the engine's signatures service the snapshot is null and stays idle: reads answer empty, refresh and validate answer null and [], analyzeChanges and readRevision refuse unsupported (readRevision synchronously), and every signing verb rejects unsupported. abortPending with nothing parked resolves.",
    plugins: FORM_SIG(),
    engine: { documents: { a: docOf({
          responses: [
            'list',
            'getContents',
            'getDigest',
            'downloadRevision',
            'analyze',
            'prepare',
            'complete',
            'cancel',
          ].map((method) => ({
            path: `signatures.${method}`,
            error: { name: 'EngineError', code: 'Unsupported', message: 'unsupported' },
          })),
        }) } },
    snapshot: ['signature::a'],
    steps: [
      ...boot,
      sig('getSnapshot'),
      sig('getStatus'),
      sig('listSignatures'),
      sig('listUnsignedFields'),
      sig('getSignature', ref(1)),
      sig('getProtection'),
      sig('refresh'),
      sig('validate'),
      sig('analyzeChanges', { since: { signatureIndex: 0 }, until: 'persisted' }),
      sig('readRevision', { revisionIndex: 0 }),
      sig('sign', { field: ref(2), mark: MARK }),
      sig('prepareSignature', { field: ref(2) }),
      sig('completeSignature', 'signing-1', $b('')),
      sig('cancelPending'),
    ],
  },
  {
    name: 'authority-twins',
    covers:
      'canSign needs doc.sign and a configured signer; canFill needs doc.forms.fill and an engine that draws into a signature field; canCertify needs allowCertify and doc.sign.certify. The mode defaults to sign with a signer.',
    plugins: FORM_SIG({ signer: CMS_SIGNER, allowCertify: true }),
    engine: {
      documents: {
        a: docOf({ allows: ['doc.sign', 'doc.forms.fill', 'doc.sign.certify'] }),
        b: docOf({ allows: ['doc.forms.fill'], lacks: ['forms.setSignatureAppearance'] }),
      },
    },
    ...UNSNAPSHOTTED,
    steps: [
      ...boot,
      open('b'),
      sig('canSign', ref(2)),
      sig('canFill', ref(2)),
      sig('canCertify'),
      sig('getMode'),
      { op: 'call', target: { token: 'signature', doc: 'b' }, method: 'canSign', args: [ref(2)] },
      { op: 'call', target: { token: 'signature', doc: 'b' }, method: 'canFill', args: [ref(2)] },
      { op: 'call', target: { token: 'signature', doc: 'b' }, method: 'canCertify', args: [] },
    ],
  },
  {
    name: 'certify-needs-the-config',
    covers: 'canCertify is false without allowCertify, whatever the grant; an explicit mode wins over the signer default.',
    plugins: FORM_SIG({ mode: 'ask' }),
    engine: { documents: { a: docOf() } },
    ...UNSNAPSHOTTED,
    steps: [...boot, sig('canCertify'), sig('canSign', ref(2)), sig('getMode')],
  },
];

// ── hydration ─────────────────────────────────────────────────────────────────

const hydration: ScenarioCase[] = [
  {
    name: 'the-initial-read-fails',
    covers: 'A snapshot read that rejects marks the status error and reports the initial read; a refresh rejects again.',
    plugins: FORM_SIG(),
    engine: { documents: { a: docOf({ responses: [{ path: 'signatures.list', error: { name: 'EngineError', message: 'io', code: 'Io' } }] }) } },
    snapshot: ['signature::a'],
    steps: [...boot, sig('getStatus'), sig('refresh')],
  },
  {
    name: 'refresh-status-and-protection',
    covers:
      "The first read is loading until it lands, then ready. A protection that differs from the last snapshot's announces itself, one that is the same doesn't; a listener that throws is reported. A later read keeps the status ready while it runs.",
    plugins: FORM_SIG(),
    engine: {
      documents: {
        a: docOf({
          signatures: [SIG2],
          responses: [
            listed([SIG2], {}, { defer: 'first', times: 1 }),
            listed([SIG2], {}, { times: 1 }),
            listed([SIG2], { protection: { docMdp: 1 } }, { defer: 'third', times: 1 }),
          ],
        }),
      },
    },
    snapshot: ['signature::a'],
    steps: [
      ...boot,
      sig('onProtectionChanged', fn('protectionChanged')),
      sig('onProtectionChanged', fn('protectionChanged.bad', { throws: { message: 'listener broke' } })),
      sig('getStatus'),
      settle('first', snap([SIG2])),
      sig('refresh'),
      sig('refresh'),
      settle('third', snap([SIG2], { protection: { docMdp: 1 } })),
      sig('getProtection'),
    ],
  },
  {
    name: 'validate-and-the-invalidation-edge',
    covers:
      "validate judges the working copy by default and records the verdicts. A verdict that turns invalid because of unsaved edits warns once, on the edge (its detail, or '' without one); staying invalid doesn't warn again, and neither does a persisted basis. `at` and `until` pass through; validateField answers one field's verdict or rejects not-found.",
    plugins: FORM_SIG({ trust: { anchors: { resolves: [] } } }),
    engine: {
      documents: {
        a: docOf({
          signatures: [SIG1, SIG2],
          responses: [
            { ...PERMITTED, times: 1 },
            { ...analysis('forbidden'), times: 1 },
            { ...analysis('forbidden'), times: 1 },
            { ...analysis('permitted'), times: 1 },
            { ...analysis('forbidden', 'working-copy', { verdict: 'forbidden', objectNumber: 7, rule: 'page-content', detail: 'replaced' }), times: 1 },
            { ...analysis('forbidden', 'persisted'), times: 1 },
          ],
        }),
      },
    },
    snapshot: ['signature::a'],
    steps: [
      ...boot,
      sig('onValidated', fn('validated')),
      sig('onInvalidationPredicted', fn('invalidating')),
      sig('validate'),
      sig('validate', { at: { kind: 'now' } }),
      sig('validateField', ref(1), { until: 'working-copy' }),
      sig('validate'),
      sig('validate', { until: 'persisted' }),
      sig('validateField', ref(2)),
    ],
  },
  {
    name: 'revisions-and-analysis',
    covers:
      "analyzeChanges asks the engine; readRevision reads a revision by index or a signed field's revision, and rejects not-found for an unsigned field or a signature sealing no whole revision.",
    plugins: FORM_SIG(),
    engine: {
      documents: {
        a: docOf({
          signatures: [SIG1, SIG2, signedDto(2, 4, 'Sig4', { revisionIndex: null })],
          responses: [
            { path: 'signatures.analyze', args: [{ since: { signatureIndex: 0 }, until: 'persisted' }], value: { current: { verdict: 'unchanged', primary: null } } },
            { path: 'signatures.downloadRevision', args: [1], value: $b('cmV2MQ==') },
            { path: 'signatures.downloadRevision', args: [0], value: $b('cmV2MA==') },
          ],
        }),
      },
    },
    ...UNSNAPSHOTTED,
    steps: [
      ...boot,
      sig('analyzeChanges', { since: { signatureIndex: 0 }, until: 'persisted' }),
      sig('readRevision', { revisionIndex: 1 }),
      sig('readRevision', ref(1)),
      sig('readRevision', ref(2)),
      sig('readRevision', ref(4)),
      sig('readRevision', ref(9)),
    ],
  },
];

// ── live document events ──────────────────────────────────────────────────────

const LIST_FAILS = { path: 'signatures.list', error: { name: 'EngineError', message: 'io', code: 'Io' } } as const;

const events: ScenarioCase[] = [
  {
    name: 'document-events',
    covers:
      'A parked, aborted or completed signing sets and clears the pending signing. A new version or a desync re-reads and re-judges; a change of the field set re-reads and re-judges soon; an edit of the working copy re-judges soon, coalesced into one validation 300 ms after the last edit. Other events are ignored.',
    plugins: FORM_SIG(),
    engine: { documents: { a: docOf() } },
    snapshot: ['signature::a'],
    steps: [
      ...boot,
      sig('onValidated', fn('validated')),
      emit({ type: 'signatures.prepared', signingId: 's-9', field: ref(2) }),
      sig('getPending'),
      emit({ type: 'signatures.cancelled', signingId: 's-9' }),
      emit({ type: 'signatures.prepared', signingId: 's-10', field: ref(2) }),
      emit({ type: 'signatures.completed', signingId: 's-10', ...completed(), origin: ORIGIN }),
      emit({ type: 'document.versioned' }),
      emit({ type: 'stream.desynced' }),
      ...[
        'forms.created',
        'forms.deleted',
        'forms.widgetAdded',
        'forms.widgetRemoved',
        'forms.repaired',
        'forms.repaired',
      ].map(
        (type) => emit({ type }),
      ),
      advance(300),
      ...[
        'annotations.created',
        'annotations.updated',
        'annotations.deleted',
        'annotations.reordered',
        'forms.valueSet',
        'forms.updated',
      ].map(
        (type) => emit({ type }),
      ),
      advance(299),
      emit({ type: 'forms.updated' }),
      advance(299),
      advance(1),
      emit({ type: 'page.rendered' }),
    ],
  },
  {
    name: 'nothing-signed-is-never-re-judged',
    covers: 'An edit with nothing signed schedules no validation.',
    plugins: FORM_SIG(),
    engine: { documents: { a: docOf({ signatures: [SIG2] }) } },
    ...UNSNAPSHOTTED,
    steps: [...boot, emit({ type: 'annotations.updated' }), advance(400)],
  },
  {
    name: 'background-failures-are-reported',
    covers:
      'A re-read after a new version or a field-set change that fails is reported, as is a scheduled validation that fails.',
    plugins: FORM_SIG(),
    engine: {
      documents: {
        a: docOf({
          responses: [listed([SIG1, SIG2], {}, { times: 2 }), { ...LIST_FAILS, times: 1 }, listed([SIG1, SIG2], {}, { times: 1 }), LIST_FAILS],
        }),
      },
    },
    ...UNSNAPSHOTTED,
    steps: [...boot, emit({ type: 'document.versioned' }), emit({ type: 'forms.created' }), emit({ type: 'annotations.updated' }), advance(300)],
  },
  {
    name: 'close-drops-the-pending-validation',
    covers: 'Closing the document with a validation scheduled clears its timer and disposes the hooks.',
    plugins: FORM_SIG(),
    engine: { documents: { a: docOf() } },
    ...UNSNAPSHOTTED,
    steps: [...boot, sig('onValidated', fn('validated')), emit({ type: 'annotations.updated' }), close(), advance(400)],
  },
];

// ── the act: fills, the target, signing ───────────────────────────────────────

const fills: ScenarioCase[] = [
  {
    name: 'fill-and-clear',
    covers:
      "fillField draws the mark's bytes into the field, re-reads the form, clears the field as the sign-here target and announces the fill, busy while it runs. clearField draws the blank page. A signed field refuses conflict; an unknown asset mark refuses not-found without the stamp plugin.",
    plugins: FORM_SIG(),
    engine: {
      documents: {
        a: docOf({ responses: [{ path: 'forms.setSignatureAppearance', times: 1, defer: 'draw' }] }),
      },
    },
    snapshot: ['signature::a'],
    steps: [
      ...boot,
      ...listenAll,
      sig('setTarget', ref(2)),
      sig('fillField', ref(2), MARK),
      sig('isBusy'),
      settle('draw'),
      sig('clearField', ref(2)),
      sig('fillField', ref(1), MARK),
      sig('clearField', { kind: 'fqn', name: 'Sig1' }),
      sig('fillField', ref(2), { assetId: 'nope' }),
      sig('fillField', ref(2), { source: $b('JVBERi0xLjc=') }),
    ],
  },
  {
    name: 'fill-without-an-engine-door',
    covers: 'An engine that cannot draw into a signature field refuses unsupported.',
    plugins: FORM_SIG(),
    engine: { documents: { a: docOf({ lacks: ['forms.setSignatureAppearance'] }) } },
    ...UNSNAPSHOTTED,
    steps: [...boot, sig('fillField', ref(2), MARK), sig('clearField', ref(2)), sig('canFill', ref(2))],
  },
  {
    name: 'the-sign-here-target',
    covers:
      'setTarget announces a new target; the same ref again, or null again, is no change; a name and an object number never match each other, two names match by name. A fill of another field keeps the target. requestInspection announces the field.',
    plugins: FORM_SIG(),
    engine: { documents: { a: docOf() } },
    snapshot: ['signature::a'],
    steps: [
      ...boot,
      listen('onTargetChanged'),
      listen('onInspectionRequested'),
      sig('setTarget', ref(2)),
      sig('getTarget'),
      sig('setTarget', ref(2)),
      sig('setTarget', fqn('Sig2')),
      sig('setTarget', fqn('Sig2')),
      sig('fillField', ref(4), MARK),
      sig('setTarget', null),
      sig('setTarget', null),
      sig('fillField', ref(2), MARK),
      sig('requestInspection', ref(1)),
      sig('getTarget'),
    ],
  },
  {
    name: 'fill-an-asset-mark',
    covers: "A mark naming a stamp asset draws the asset's bytes; an asset the stamp plugin doesn't know refuses not-found.",
    plugins: [{ factory: 'interaction' }, { factory: 'form' }, { factory: 'stamp' }, { factory: 'signature' }],
    engine: withWorkshop({ documents: { a: docOf() } }),
    ...UNSNAPSHOTTED,
    steps: [...boot, st('createAsset', { name: 'Logo', source: PNG }), sig('fillField', ref(2), { assetId: 'stamp-lib-0-0:Logo' }), sig('fillField', ref(2), { assetId: 'nope' })],
  },
];

const PREPARE: ResponseSpec = { path: 'signatures.prepare', value: prepared(),
  events: canonical([
    {
      type: 'signatures.prepared',
      ...prepared(),
      digest: digestBytes(32, 3),
      field: ref(2),
      origin: ORIGIN,
    } satisfies import('@embedpdf/engine-core/runtime').DocumentEvent,
  ]) as ResponseSpec['events'],
};
const COMPLETE: ResponseSpec = { path: 'signatures.complete', value: completed(),
  events: [
    {
      type: 'signatures.completed',
      signingId: 'signing-1',
      ...completed(),
      origin: ORIGIN,
    } satisfies import('@embedpdf/engine-core/runtime').DocumentEvent,
  ],
};

const signing: ScenarioCase[] = [
  {
    name: 'two-phase-signing',
    covers:
      'prepareSignature parks the signing (pending, with the appearance when a mark or appearance is given) and hands out the digest. completeSignature of a signing this session did not prepare rejects not-found; of the parked one it completes against the prepared version, clears the pending signing and the target, re-reads, re-judges and announces the signature.',
    plugins: FORM_SIG(),
    engine: {
      documents: {
        a: docOf({
          responses: [listed([SIG1, SIG2], {}, { times: 2 }), listed(BOTH_SIGNED), PREPARE, COMPLETE],
        }),
      },
    },
    snapshot: ['signature::a'],
    steps: [
      ...boot,
      listen('onSigned'),
      listen('onTargetChanged'),
      sig('setTarget', ref(2)),
      sig('prepareSignature', {
        field: ref(2),
        mark: MARK,
        signer: { reason: 'approve' },
        certify: { permission: 2 },
        lock: { action: 'all' },
        subFilter: 'adbe.pkcs7.detached',
        digest: 'sha384',
      }),
      sig('getPending'),
      sig('completeSignature', 'unknown', $b('AAEC')),
      sig('completeSignature', 'signing-1', $b('AAEC')),
      sig('getPending'),
      sig('prepareSignature', { field: ref(2) }),
      sig('prepareSignature', { field: ref(2), appearance: $b('JVBERi0xLjc='), mark: MARK }),
    ],
  },
  {
    name: 'abort-the-parked-signing',
    covers: 'abortPending cancels the parked signing and clears it; with nothing parked it does nothing.',
    plugins: FORM_SIG(),
    engine: {
      documents: { a: docOf({ responses: [PREPARE, { path: 'signatures.cancel', value: { status: 'cancelled' },
              events: [{ type: 'signatures.cancelled', signingId: 'signing-1', origin: ORIGIN }],
            }] }) },
    },
    snapshot: ['signature::a'],
    steps: [...boot, sig('prepareSignature', { field: ref(2), mark: MARK }), sig('cancelPending'), sig('cancelPending'), sig('completeSignature', 'signing-1', $b('AAEC'))],
  },
  {
    name: 'a-parked-signing-from-elsewhere',
    covers: "abortPending cancels a signing the engine announced, which this session can't complete.",
    plugins: FORM_SIG(),
    engine: { documents: { a: docOf({ responses: [{ path: 'signatures.cancel', value: { status: 'cancelled' },
              events: [{ type: 'signatures.cancelled', signingId: 'signing-1', origin: ORIGIN }],
            }] }) } },
    ...UNSNAPSHOTTED,
    steps: [...boot, emit({ type: 'signatures.prepared', signingId: 'remote-1', field: ref(2) }), sig('completeSignature', 'remote-1', $b('AAEC')), sig('cancelPending'), sig('getPending')],
  },
  {
    name: 'sign-one-shot',
    covers:
      "sign resolves the configured signer, prepares with the mark as the appearance and the caller's attribution, has a CMS signer answer, passes the completion gate and completes; then it clears the target, re-reads, starts a validation and announces the signature.",
    plugins: FORM_SIG({ signer: CMS_SIGNER }),
    engine: {
      documents: {
        a: docOf({ responses: [listed([SIG1, SIG2], {}, { times: 2 }), listed(BOTH_SIGNED), PREPARE, COMPLETE] }),
      },
    },
    snapshot: ['signature::a'],
    awaitCalls: true,
    steps: [...boot, listen('onSigned'), sig('setTarget', ref(2)), drain, sig('sign', { field: ref(2), mark: MARK,
        signer: { reason: 'approve' }, lock: { action: 'all' } }), drain],
  },
  {
    name: 'sign-with-a-per-call-signer',
    covers:
      'A per-call signer wins over the configured one, and a function resolves the signer first. An explicit appearance wins over the mark. A validation after signing that fails is reported.',
    plugins: FORM_SIG({ signer: { ...CMS_SIGNER, answer: { rejects: { message: 'not me' } } } }),
    engine: {
      documents: {
        a: docOf({ responses: [listed([SIG1, SIG2], {}, { times: 3 }), { ...LIST_FAILS, times: 1 }, listed(BOTH_SIGNED), PREPARE, COMPLETE] }),
      },
    },
    ...UNSNAPSHOTTED,
    awaitCalls: true,
    steps: [
      ...boot,
      drain,
      sig('sign', {
        field: ref(2),
        mark: MARK,
        appearance: $b('JVBERi0xLjc='),
        key: fn('call.resolve', { resolves: { kind: 'cms', sign: fn('call.sign', { resolves: cmsArg('rsaCades') }) } }),
      }),
      drain,
    ],
  },
  {
    name: 'sign-with-a-raw-signer',
    covers:
      "A raw signer's certificate subject is the default /Name, which the caller's attribution overrides; a signer that fails aborts the signing and rejects. A thunk signer resolves per signing. A raw signer with no certificate names no one.",
    plugins: FORM_SIG({
      signer: { kind: 'raw', certificateChain: [certArg('leaf'), certArg('inter')], algorithm: 'RSA-PKCS1-v1_5', hash: 'sha256', answer: { rejects: { message: 'token removed' } }, thunk: true },
    }),
    engine: {
      documents: { a: docOf({ responses: [PREPARE, { path: 'signatures.cancel', value: { status: 'cancelled' },
              events: [{ type: 'signatures.cancelled', signingId: 'signing-1', origin: ORIGIN }],
            }] }) },
    },
    snapshot: ['signature::a'],
    awaitCalls: true,
    steps: [
      ...boot,
      drain,
      sig('sign', { field: ref(2), mark: MARK }),
      sig('sign', { field: ref(2), mark: MARK,
        signer: { name: 'Someone Else', location: 'Oslo' } }),
      sig('sign', {
        field: ref(2),
        mark: MARK,
        key: { kind: 'raw', certificateChain: [], algorithm: 'RSA-PKCS1-v1_5', hash: 'sha256', sign: fn('raw.sign', { resolves: $b('') }) },
      }),
    ],
  },
  {
    name: 'sign-without-a-signer',
    covers: "sign with no signer configured or passed rejects invalid-input; a mark the stamp plugin can't resolve rejects not-found.",
    plugins: FORM_SIG(),
    engine: { documents: { a: docOf() } },
    ...UNSNAPSHOTTED,
    steps: [...boot, sig('sign', { field: ref(2), mark: MARK })],
  },
];

// ── the destination rule ──────────────────────────────────────────────────────

const PLACE_DOC = (o: DocOptions = {}) =>
  docOf({
    ...o,
    responses: [
      ...(o.responses ?? []),
      { path: 'page.annotations.create', pon: 1, value: created(stampDto(60)) },
    ],
  });
const at = { page: { kind: 'objectNumber', objectNumber: 1 }, at: { x: 100, y: 100 }, targetWidth: 50 };

const placement: ScenarioCase[] = [
  {
    name: 'place-by-mode-visual',
    covers: 'Mode visual fills the field; a free placement of an asset goes through the stamp plugin and of bytes through the annotation plugin.',
    plugins: FULL(),
    engine: withWorkshop({ documents: { a: PLACE_DOC() } }),
    ...UNSNAPSHOTTED,
    steps: [
      ...boot,
      listen('onFilled'),
      st('createAsset', { name: 'Logo', source: PNG }),
      sig('placeMark', MARK, { field: ref(2) }),
      sig('placeMark', { assetId: 'stamp-lib-0-0:Logo' }, at),
      sig('placeMark', MARK, at),
    ],
  },
  {
    name: 'place-by-mode-ask',
    covers: 'Mode ask hands the field and the mark to the chrome.',
    plugins: FORM_SIG({ mode: 'ask' }),
    engine: { documents: { a: docOf() } },
    ...UNSNAPSHOTTED,
    steps: [...boot, listen('onSignRequested'), sig('placeMark', { assetId: 'a-1' }, { field: ref(2) })],
  },
  {
    name: 'place-an-asset-without-the-stamp-plugin',
    covers: 'A free placement of an asset with no stamp plugin refuses unsupported.',
    plugins: [{ factory: 'interaction' }, { factory: 'annotation' }, { factory: 'signature-annotation' }, { factory: 'form' }, { factory: 'signature' }],
    engine: { documents: { a: docOf() } },
    ...UNSNAPSHOTTED,
    steps: [...boot, sig('placeMark', { assetId: 'a-1' }, at)],
  },
  {
    name: 'place-by-mode-sign-offers-certification',
    covers:
      'Mode sign with allowCertify and nothing signed yet asks the chrome (the first signature can certify) instead of sealing.',
    plugins: FORM_SIG({ signer: CMS_SIGNER, allowCertify: true }),
    engine: { documents: { a: docOf({ signatures: [SIG2] }) } },
    ...UNSNAPSHOTTED,
    steps: [...boot, listen('onSignRequested'), sig('placeMark', MARK, { field: ref(2) })],
  },
  {
    name: 'place-by-mode-sign-seals',
    covers: 'Mode sign seals the field, even with allowCertify once something is signed.',
    plugins: FORM_SIG({ signer: CMS_SIGNER, allowCertify: true }),
    engine: {
      documents: {
        a: docOf({ responses: [listed([SIG1, SIG2], {}, { times: 2 }), listed(BOTH_SIGNED), PREPARE, COMPLETE] }),
      },
    },
    ...UNSNAPSHOTTED,
    awaitCalls: true,
    steps: [...boot, drain, sig('placeMark', MARK, { field: ref(2) }), drain],
  },
];

// ── the armed mark ────────────────────────────────────────────────────────────

const LIB = assetDoc([assetPage(0, 100)]);
const down = (x: number, y: number, over: Dto = {}) => ix('dispatchPointer', sample('down', x, y, over));
const up = (x: number, y: number) => ix('dispatchPointer', sample('up', x, y));
const importSignatures = st('importLibrary', PDF, { libraryKind: 'signatures' });
const importStamps = st('importLibrary', PDF);
const arm = st('armAsset', 'a', 'stamp-lib-0-1:Stamp1');

const armed: ScenarioCase[] = [
  {
    name: 'an-armed-mark-over-a-field',
    covers:
      "With the interaction hub and the stamp plugin, an armed asset from a signatures library pressed over an empty signature field disarms and goes into the field by mode. Over a signed field, or a signature field that carries a value, the press is consumed and nothing happens. Over a text field, over no field, or without a page the handler declines and the annotation plugin's stamp placement runs.",
    plugins: FULL({ mode: 'ask' }),
    engine: { documents: { a: PLACE_DOC(), lib: LIB }, responses: opens('a', 'lib') },
    ...UNSNAPSHOTTED,
    steps: [
      ...boot,
      listen('onSignRequested'),
      importSignatures,
      arm,
      down(150, 188),
      up(150, 188),
      down(150, 488),
      up(150, 488),
      down(150, 388),
      up(150, 388),
      arm,
      down(500, 700),
      up(500, 700),
      arm,
      down(10, 10, { page: null }),
      arm,
      down(150, 288),
      up(150, 288),
      ix('getActiveToolId'),
    ],
  },
  {
    name: 'an-armed-stamp-is-not-a-mark',
    covers:
      "An asset from a library of another kind, or a stamp the annotation plugin armed on its own, is not a person's mark: the handler declines. With another tool active the handler is not asked.",
    plugins: FULL({ mode: 'ask' }),
    engine: { documents: { a: PLACE_DOC(), lib: LIB }, responses: opens('a', 'lib') },
    ...UNSNAPSHOTTED,
    steps: [
      ...boot,
      listen('onSignRequested'),
      importStamps,
      arm,
      down(150, 288),
      up(150, 288),
      anno('arm', { source: PNG }),
      down(150, 288),
      up(150, 288),
      ix('activateTool', 'pan'),
      down(150, 288),
    ],
  },
  {
    name: 'an-armed-mark-reports-its-handler',
    covers: "The press that drops a mark into a field is the handler's gesture: onGestureStarted names signature:armed-mark-on-field.",
    plugins: FULL({ mode: 'ask' }),
    engine: { documents: { a: PLACE_DOC(), lib: LIB }, responses: opens('a', 'lib') },
    ...UNSNAPSHOTTED,
    steps: [...boot, ix('onGestureStarted', fn('started')), importSignatures, arm, down(150, 288), up(150, 288)],
  },
  {
    name: 'a-legacy-stamp-tool-keeps-the-arm',
    covers:
      "A tool enabling the legacy annotation-stamp tag keeps the stamp armed, but it is not the armed-stamp tool, so the handler is not asked and the annotation plugin's stamp placement runs over the field.",
    plugins: FULL({ mode: 'ask' }),
    engine: { documents: { a: PLACE_DOC(), lib: LIB }, responses: opens('a', 'lib') },
    ...UNSNAPSHOTTED,
    steps: [
      ...boot,
      listen('onSignRequested'),
      importSignatures,
      arm,
      ix('registerTool', { id: 'legacy-stamp', cursor: 'copy', enables: { $set: ['annotation-stamp'] } }),
      ix('activateTool', 'legacy-stamp'),
      anno('isArmed'),
      down(150, 288),
      up(150, 288),
    ],
  },
  {
    name: 'a-mark-that-fails-to-land',
    covers: 'A placement the handler started that rejects is reported.',
    plugins: FULL(),
    engine: { documents: { a: PLACE_DOC({ lacks: ['forms.setSignatureAppearance'] }), lib: LIB }, responses: opens('a', 'lib') },
    ...UNSNAPSHOTTED,
    steps: [...boot, importSignatures, arm, down(150, 288), up(150, 288)],
  },
  {
    name: 'no-handler-without-the-hub-and-the-stamp-plugin',
    covers: 'Without the stamp plugin no handler is registered, so an armed stamp over a field stays a stamp placement.',
    plugins: [{ factory: 'interaction' }, { factory: 'annotation' }, { factory: 'signature-annotation' },
      { factory: 'form' }, { factory: 'signature', config: { mode: 'ask' } }],
    engine: { documents: { a: PLACE_DOC({ responses: [{ path: 'apply', ...confirmation(901, [{ item: currentCreated(materialize(3, { subtype: 'stamp', box: { x: 100, y: 263, width: 100, height: 50 } })) }]) }] }) } },
    ...UNSNAPSHOTTED,
    steps: [...boot, listen('onSignRequested'), anno('arm', { source: PNG }), down(150, 288), up(150, 288)],
  },
];

// ── the unit-test shapes ──────────────────────────────────────────────────────

const shapes: ScenarioCase[] = [
  {
    name: 'the-connected-controller-loads-its-mirror',
    covers: 'The current controller loads its mirror on connect and folds engine facts.',
    plugins: FORM_SIG({ shape: 'controller' }),
    engine: { documents: { a: docOf() } },
    snapshot: ['signature::a'],
    steps: [...boot, sig('getStatus'), emit({ type: 'signatures.prepared', signingId: 's-1', field: ref(2) }), sig('getPending'), sig('refresh')],
  },
  {
    name: 'the-controller-shape',
    covers: 'createSignatureController with no config connects the same way the plugin does, without the armed-mark handler.',
    plugins: [{ factory: 'interaction' }, { factory: 'form' }, { factory: 'stamp' }, { factory: 'signature', config: { shape: 'controller' } }],
    engine: { documents: { a: docOf({ signatures: [SIG2] }) } },
    snapshot: ['signature::a'],
    steps: [...boot, sig('getStatus'), sig('getMode')],
  },
];

// ── functions ─────────────────────────────────────────────────────────────────

const functions = {
  initialSignatureState: () => initialSignatureState(),
  setVerdicts: model.setVerdicts,
  setBusy: model.setBusy,
  blankPagePdf: () => blankPagePdf(),
  markRoleOf: (asset: { name: string }) => markRoleOf(asset),
  setTarget: (target: Parameters<typeof model.setTarget>[1], field: Parameters<typeof model.setTarget>[1],
  ) => {
    const before = { ...initialSignatureState(), target };
    const state = model.setTarget(before, field);
    return { changed: state !== before, state };
  },
  sameRef: (a: never, b: never) => sameRef(a, b),
};

const S0 = initialSignatureState();
const functionCases: FunctionCase[] = [
  { name: 'initial-state', fn: 'initialSignatureState', args: [] },
  { name: 'set-verdicts', fn: 'setVerdicts', args: [S0, []] }, { name: 'set-target', fn: 'setTarget', args: [null, ref(2)] }, { name: 'set-busy', fn: 'setBusy', args: [S0, true] },
  { name: 'blank-page', covers: 'The blank one-page PDF a clear draws.', fn: 'blankPagePdf', args: [] },
  { name: 'blank-page-cached', covers: 'A second call copies the cached bytes.', fn: 'blankPagePdf', args: [] },
  { name: 'mark-role-initials', fn: 'markRoleOf', args: [{ name: 'initials' }] },
  { name: 'mark-role-signature', fn: 'markRoleOf', args: [{ name: 'anything' }] },
  { name: 'is-target-none', fn: 'setTarget', args: [null, ref(1)] },
  { name: 'is-target-same', fn: 'setTarget', args: [ref(1), ref(1)] },
  { name: 'is-target-other', fn: 'setTarget', args: [ref(1), ref(2)] },
  { name: 'same-ref-numbers', fn: 'sameRef', args: [ref(1), ref(1)] },
  { name: 'same-ref-names', fn: 'sameRef', args: [fqn('a'), fqn('b')] },
  { name: 'same-ref-mixed', fn: 'sameRef', args: [fqn('a'), ref(1)] },
];

export const suites: Suite[] = [
  {
    kind: 'scenario',
    file: 'plugins/signature.json',
    describes:
      'packages/plugin/signature: reads, hydration and validation, document events, fills, the sign-here target, one-shot and two-phase signing, the destination rule and the armed mark.',
    cases: [...reads, ...hydration, ...events, ...fills, ...signing, ...placement, ...armed, ...shapes],
  },
  {
    kind: 'function',
    file: 'plugins/signature-functions.json',
    describes:
      'packages/plugin/signature: session transitions, the blank page, markRoleOf, setTarget and sameFieldRef.',
    functions,
    cases: functionCases,
  },
];
