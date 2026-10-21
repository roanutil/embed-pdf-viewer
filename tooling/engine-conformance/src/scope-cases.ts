/**
 * The `auth/scope` corpus: what the grammar accepts, what it refuses, and what
 * a scope array actually grants once `pdf.permissions` has expanded and the
 * implication rules have run.
 *
 * `multiplatform-port-order.md` G3 makes this a gate: "`auth/scope` shared
 * vectors agree with the TypeScript resolver". The module is a deliberate
 * duplicate, kept in both languages rather than ported, because web retains
 * the resolver in `engine/core` and native permission enforcement has no other
 * owner. Two copies of an authorization rule is exactly the arrangement that
 * drifts, and drift here is privilege escalation rather than a wrong pixel.
 *
 * So the cases lean on the places where a reimplementation would plausibly
 * differ rather than on breadth: narrowing versus the broad `modify` default,
 * the bit-12-without-bit-3 pair, the group-membership check that makes
 * `group=X` more than a string compare, and the filter values that contain
 * colons.
 */

export interface ParseCase {
  /** Stable slug, and what a failure names. */
  readonly name: string;
  /** Why the case exists, in one line. */
  readonly covers: string;
  readonly scope: string;
}

/**
 * A permission word, named. `null` is the unprobed column, which decodes to
 * all-false rather than to all-true, so an unprobed document grants nothing
 * through `pdf.permissions`.
 */
export interface BitsCase {
  readonly name: string;
  readonly raw: number | null;
}

export interface CollabProbe {
  readonly action: 'create' | 'update' | 'delete' | 'set-group';
  readonly target: { readonly userId?: string; readonly groupId?: string };
}

export interface SetGroupProbe {
  readonly newGroupId: string;
  readonly callerDefaultGroupId?: string;
}

/** Filling in or signing one form field, in `groupId` (`null`: in no group). */
export interface FieldProbe {
  readonly action: 'fill' | 'sign';
  readonly groupId: string | null;
}

export interface ResolveCase {
  readonly name: string;
  readonly covers: string;
  readonly scope: readonly string[];
  /** Names from BITS_CASES. Each one is resolved against this scope. */
  readonly bits: readonly string[];
  readonly identity?: {
    readonly user_id?: string;
    readonly group_id?: string;
    readonly groups?: readonly string[];
  };
  readonly collab?: readonly CollabProbe[];
  readonly setGroup?: readonly SetGroupProbe[];
  /**
   * `checkFieldAction` per probe, `checkAnyFieldAction` per action, and
   * `fieldRightsOf` with the identity's default group.
   */
  readonly fields?: readonly FieldProbe[];
  /** `checkSetGroup` for the `fields` entity; `setGroup` is `annotations`. */
  readonly fieldSetGroup?: readonly SetGroupProbe[];
}

/** Every capability the closed set contains, in declaration order. */
export const ALL_CAPABILITIES = [
  'doc.open',
  'doc.render',
  'doc.text.select',
  'doc.text.copy',
  'doc.text.search',
  'doc.content.copy',
  'doc.download',
  'doc.download.flattened',
  'doc.print',
  'doc.print.high',
  'doc.pages.modify',
  'doc.pages.assemble',
  'doc.forms.read',
  'doc.forms.fill',
  'doc.forms.modify',
  'doc.forms.import',
  'doc.forms.script',
  'doc.forms.submit',
  'doc.annotate.read',
  'doc.annotate.modify',
  'doc.annotate.import',
  'doc.metadata.modify',
  'doc.attachments.modify',
  'doc.outline.modify',
  'doc.redact',
  'doc.sign',
  'doc.sign.certify',
] as const;

const bit = (n: number) => 1 << (n - 1);

export const BITS_CASES: readonly BitsCase[] = [
  { name: 'unprobed', raw: null },
  { name: 'none', raw: 0 },
  { name: 'print', raw: bit(3) },
  { name: 'print-high-without-print', raw: bit(12) },
  { name: 'print-and-high', raw: bit(3) | bit(12) },
  { name: 'modify', raw: bit(4) },
  { name: 'copy', raw: bit(5) },
  { name: 'annotate-fill', raw: bit(6) },
  { name: 'fill-forms-only', raw: bit(9) },
  { name: 'accessibility', raw: bit(10) },
  { name: 'assemble', raw: bit(11) },
  { name: 'annotate-and-modify', raw: bit(6) | bit(4) },
  // The reserved bits the decoder does not surface, set alongside one it does.
  // A Rust decoder that masked the word differently would show up here.
  { name: 'reserved-bits-set', raw: bit(1) | bit(2) | bit(7) | bit(8) | bit(13) | bit(5) },
  { name: 'all-surfaced', raw: [3, 4, 5, 6, 9, 10, 11, 12].reduce((a, n) => a | bit(n), 0) },
  // What PDFium actually hands back for a document with no encryption dict.
  { name: 'pdfium-unencrypted', raw: 0xffffffff },
  { name: 'negative', raw: -1 },
] as const;

export const PARSE_CASES: readonly ParseCase[] = [
  { name: 'wildcard', covers: 'The global escape hatch.', scope: '*' },
  { name: 'virtual', covers: 'The one virtual scope, expanded at resolve time.', scope: 'pdf.permissions' },

  // Every member of the closed set, so a Rust set that dropped or misspelled
  // one fails here rather than at some call site months later.
  ...ALL_CAPABILITIES.map((name) => ({
    name: `capability-${name}`,
    covers: 'A member of the closed capability set.',
    scope: name,
  })),

  // Names deliberately removed. The comment in parser.ts says a JWT carrying
  // one must fail at verify time, so these must stay rejections.
  { name: 'legacy-doc-read', covers: 'A removed legacy name still has to be refused.', scope: 'doc.read' },
  { name: 'legacy-doc-edit-pages', covers: 'A removed legacy name still has to be refused.', scope: 'doc.edit-pages' },
  { name: 'legacy-doc-save', covers: 'A removed legacy name still has to be refused.', scope: 'doc.save' },

  // Shape, which is checked before membership.
  { name: 'shape-single-segment', covers: 'A dotted name needs at least two segments.', scope: 'doc' },
  { name: 'shape-trailing-dot', covers: 'A trailing dot leaves an empty segment.', scope: 'doc.' },
  { name: 'shape-leading-dot', covers: 'A leading dot leaves an empty segment.', scope: '.doc' },
  { name: 'shape-double-dot', covers: 'An empty middle segment.', scope: 'doc..open' },
  { name: 'shape-uppercase', covers: 'The shape is lowercase, so case is not normalized away.', scope: 'Doc.Open' },
  { name: 'shape-digit-leading', covers: 'A segment starts with a letter.', scope: 'doc.1open' },
  { name: 'shape-hyphen', covers: 'Hyphens are not in the segment alphabet.', scope: 'doc.open-now' },
  { name: 'shape-underscore', covers: 'Underscores are, which is why the hyphen case exists next to it.', scope: 'doc.open_now' },
  { name: 'shape-trailing-space', covers: 'No trimming: a stray space is a different string.', scope: 'doc.open ' },
  { name: 'shape-empty', covers: 'The empty string is not a scope.', scope: '' },

  // Collab grammar.
  { name: 'collab-create-all', covers: 'The plain shape.', scope: 'annotations:create:all' },
  { name: 'collab-update-self', covers: 'Resolved against identity rather than the string.', scope: 'annotations:update:self' },
  { name: 'collab-delete-created-by', covers: 'A filter carrying a value.', scope: 'annotations:delete:createdBy=u1' },
  { name: 'collab-update-group', covers: 'The filter with the membership check behind it.', scope: 'annotations:update:group=legal' },
  { name: 'collab-action-wildcard', covers: 'The action wildcard, which includes set-group.', scope: 'annotations:*:all' },
  { name: 'collab-set-group-all', covers: 'Assignment authority over any group.', scope: 'annotations:set-group:all' },
  { name: 'collab-set-group-group', covers: 'Assignment authority over one group.', scope: 'annotations:set-group:group=legal' },

  // Filter values that contain colons. The parser splits on the first two and
  // only the first two, which is the whole reason these are here.
  {
    name: 'collab-created-by-with-colons',
    covers: 'A subject id with a colon in it survives the split.',
    scope: 'annotations:update:createdBy=auth0|user:1',
  },
  {
    name: 'collab-group-urn',
    covers: 'A urn:uuid group id survives the split.',
    scope: 'annotations:update:group=urn:uuid:5f2b',
  },

  // Collab rejections.
  { name: 'collab-one-colon', covers: 'entity:action with no filter.', scope: 'annotations:create' },
  { name: 'collab-unknown-entity', covers: 'Only annotations and fields are collab-scoped.', scope: 'comments:create:all' },
  { name: 'collab-unknown-action', covers: 'The action set is closed.', scope: 'annotations:frobnicate:all' },
  { name: 'collab-unknown-filter', covers: 'The filter vocabulary is closed.', scope: 'annotations:create:bogus' },
  { name: 'collab-empty-created-by', covers: 'createdBy= with nothing after it.', scope: 'annotations:create:createdBy=' },
  { name: 'collab-empty-group', covers: 'group= with nothing after it.', scope: 'annotations:create:group=' },
  {
    name: 'collab-set-group-self',
    covers: 'set-group refuses :self at parse time rather than always-denying later.',
    scope: 'annotations:set-group:self',
  },
  {
    name: 'collab-set-group-created-by',
    covers: 'set-group refuses :createdBy for the same reason.',
    scope: 'annotations:set-group:createdBy=u1',
  },
  {
    name: 'collab-entity-wildcard',
    covers: 'The wildcard is on the action, not the entity.',
    scope: '*:create:all',
  },
  {
    name: 'collab-empty-action',
    covers: 'An empty action is not the wildcard.',
    scope: 'annotations::all',
  },

  // Field scopes (4b2cdb395): `fields:<fill|sign|set-group|*>:<all|group=X>`.
  { name: 'fields-fill-all', covers: 'Filling in every field.', scope: 'fields:fill:all' },
  { name: 'fields-sign-group', covers: 'Signing the fields of one group.', scope: 'fields:sign:group=buyer' },
  { name: 'fields-set-group-group', covers: 'Putting a field in one group.', scope: 'fields:set-group:group=seller' },
  { name: 'fields-action-wildcard', covers: 'The action wildcard over fields.', scope: 'fields:*:group=buyer' },
  { name: 'fields-group-urn', covers: 'A urn:uuid group id survives the split for fields too.', scope: 'fields:fill:group=urn:uuid:5f2b' },
  { name: 'fields-self', covers: 'A field records no creator, so :self is refused.', scope: 'fields:fill:self' },
  { name: 'fields-created-by', covers: 'And so is :createdBy.', scope: 'fields:sign:createdBy=alice' },
  { name: 'fields-annotation-action', covers: 'Fields have no create action.', scope: 'fields:create:all' },
  { name: 'fields-update', covers: 'Nor an update action.', scope: 'fields:update:group=buyer' },
  { name: 'fields-empty-group', covers: 'group= with nothing after it.', scope: 'fields:fill:group=' },
] as const;

const SELF = { user_id: 'u1', group_id: 'legal', groups: ['legal', 'eng'] } as const;

/** The probes every collab case runs, so the table is uniform. */
const COLLAB_PROBES: readonly CollabProbe[] = [
  { action: 'create', target: { userId: 'u1', groupId: 'legal' } },
  { action: 'update', target: { userId: 'u1', groupId: 'legal' } },
  { action: 'update', target: { userId: 'u2', groupId: 'legal' } },
  { action: 'update', target: { userId: 'u2', groupId: 'finance' } },
  { action: 'update', target: {} },
  { action: 'delete', target: { userId: 'u1', groupId: 'legal' } },
  { action: 'delete', target: { userId: 'u2', groupId: 'finance' } },
  { action: 'set-group', target: { userId: 'u1', groupId: 'legal' } },
] as const;

const SET_GROUP_PROBES: readonly SetGroupProbe[] = [
  { newGroupId: 'legal', callerDefaultGroupId: 'legal' },
  { newGroupId: 'finance', callerDefaultGroupId: 'legal' },
  { newGroupId: 'finance', callerDefaultGroupId: undefined },
  { newGroupId: 'eng', callerDefaultGroupId: 'legal' },
] as const;

/** The field probes every field case runs: each action, in a group, another, and none. */
const FIELD_PROBES: readonly FieldProbe[] = [
  { action: 'fill', groupId: 'buyer' },
  { action: 'fill', groupId: 'seller' },
  { action: 'fill', groupId: null },
  { action: 'sign', groupId: 'buyer' },
  { action: 'sign', groupId: 'seller' },
  { action: 'sign', groupId: null },
] as const;

const FIELD_SET_GROUP_PROBES: readonly SetGroupProbe[] = [
  { newGroupId: 'legal', callerDefaultGroupId: 'legal' },
  { newGroupId: 'buyer', callerDefaultGroupId: 'legal' },
  { newGroupId: 'seller', callerDefaultGroupId: 'legal' },
  { newGroupId: 'buyer', callerDefaultGroupId: undefined },
] as const;

const EVERY_BITS = BITS_CASES.map((b) => b.name);

export const RESOLVE_CASES: readonly ResolveCase[] = [
  {
    name: 'empty',
    covers: 'No scope grants nothing, whatever the bits say.',
    scope: [],
    bits: EVERY_BITS,
    identity: SELF,
    collab: COLLAB_PROBES,
    setGroup: SET_GROUP_PROBES,
    fields: FIELD_PROBES,
    fieldSetGroup: FIELD_SET_GROUP_PROBES,
  },
  {
    name: 'wildcard',
    covers: 'Short-circuits every check before expansion, including collab.',
    scope: ['*'],
    bits: EVERY_BITS,
    identity: SELF,
    collab: COLLAB_PROBES,
    setGroup: SET_GROUP_PROBES,
    fields: FIELD_PROBES,
    fieldSetGroup: FIELD_SET_GROUP_PROBES,
  },
  {
    name: 'pdf-permissions',
    covers: 'The whole bit expansion table, one row per permission word.',
    scope: ['pdf.permissions'],
    bits: EVERY_BITS,
    identity: SELF,
    collab: COLLAB_PROBES,
    setGroup: SET_GROUP_PROBES,
    fields: FIELD_PROBES,
    fieldSetGroup: FIELD_SET_GROUP_PROBES,
  },
  {
    name: 'annotate-modify-implies-read',
    covers: 'You cannot sensibly modify what you cannot see.',
    scope: ['doc.annotate.modify'],
    bits: ['none'],
  },
  {
    name: 'forms-modify-implies-fill-and-read',
    covers: 'The two-step implication, which a Rust port could easily do in one.',
    scope: ['doc.forms.modify'],
    bits: ['none'],
  },
  {
    name: 'forms-fill-implies-read',
    covers: 'The second step on its own.',
    scope: ['doc.forms.fill'],
    bits: ['none'],
  },
  {
    name: 'collab-implies-annotate-read',
    covers: 'A mutation route has to see the row to evaluate the filter against it.',
    scope: ['annotations:update:self'],
    bits: ['none'],
    identity: SELF,
    collab: COLLAB_PROBES,
  },
  {
    name: 'forms-submit-is-grant-only',
    covers: 'No bit expands to it, so even all-bits plus pdf.permissions withholds it.',
    scope: ['pdf.permissions'],
    bits: ['all-surfaced', 'pdfium-unencrypted'],
  },
  {
    name: 'narrowing-modify-plus-update-self',
    covers:
      'The case the narrowing model exists for: [modify, update:self] means edit own only, not edit anyone via modify-bypass.',
    scope: ['doc.annotate.modify', 'annotations:update:self'],
    bits: ['none'],
    identity: SELF,
    collab: COLLAB_PROBES,
  },
  {
    name: 'narrowing-is-per-action',
    covers: 'update is narrowed, delete still falls back to the broad modify default.',
    scope: ['doc.annotate.modify', 'annotations:update:createdBy=u9'],
    bits: ['none'],
    identity: SELF,
    collab: COLLAB_PROBES,
  },
  {
    name: 'narrowing-from-bits-not-scope',
    covers: 'The broad default can arrive through pdf.permissions, and narrowing still shadows it.',
    scope: ['pdf.permissions', 'annotations:update:self'],
    bits: ['annotate-fill'],
    identity: SELF,
    collab: COLLAB_PROBES,
  },
  {
    name: 'group-filter-needs-membership',
    covers:
      'Named for the rule before 4b2cdb395. group=X now matches a target in group X whether or not the caller belongs to X: membership is no longer part of the filter.',
    scope: ['annotations:update:group=finance'],
    bits: ['none'],
    identity: SELF,
    collab: COLLAB_PROBES,
  },
  {
    name: 'group-filter-with-membership',
    covers: 'The same scope for a caller who is in the group.',
    scope: ['annotations:update:group=finance'],
    bits: ['none'],
    identity: { user_id: 'u1', group_id: 'legal', groups: ['legal', 'finance'] },
    collab: COLLAB_PROBES,
  },
  {
    name: 'self-filter-without-identity',
    covers: 'No user_id means :self matches nothing, rather than matching a row with no owner.',
    scope: ['annotations:update:self'],
    bits: ['none'],
    identity: {},
    collab: COLLAB_PROBES,
  },
  {
    name: 'action-wildcard-covers-every-action',
    covers: 'annotations:*:all narrows every action at once.',
    scope: ['doc.annotate.modify', 'annotations:*:createdBy=u2'],
    bits: ['none'],
    identity: SELF,
    collab: COLLAB_PROBES,
  },
  {
    name: 'set-group-default-needs-no-authority',
    covers: 'Assigning your own default group is not a reassignment.',
    scope: [],
    bits: ['none'],
    identity: SELF,
    setGroup: SET_GROUP_PROBES,
  },
  {
    name: 'set-group-is-not-implied-by-modify',
    covers: 'Assignment authority is cloud-only and does not inherit from modify.',
    scope: ['doc.annotate.modify'],
    bits: ['all-surfaced'],
    identity: SELF,
    setGroup: SET_GROUP_PROBES,
  },
  {
    name: 'set-group-all',
    covers: 'Authority over any destination group.',
    scope: ['annotations:set-group:all'],
    bits: ['none'],
    identity: SELF,
    setGroup: SET_GROUP_PROBES,
  },
  {
    name: 'set-group-one-group',
    covers: 'Authority over one destination group and not another.',
    scope: ['annotations:set-group:group=finance'],
    bits: ['none'],
    identity: SELF,
    setGroup: SET_GROUP_PROBES,
  },
  {
    name: 'set-group-through-action-wildcard',
    covers: 'The action wildcard reaches set-group.',
    scope: ['annotations:*:group=finance'],
    bits: ['none'],
    identity: SELF,
    setGroup: SET_GROUP_PROBES,
    collab: COLLAB_PROBES,
  },
  {
    name: 'set-group-ignores-unusable-filters-on-wildcard',
    covers:
      'An action wildcard carrying :self is legitimate for other actions and cannot satisfy set-group.',
    scope: ['annotations:*:self'],
    bits: ['none'],
    identity: SELF,
    setGroup: SET_GROUP_PROBES,
    collab: COLLAB_PROBES,
  },
  {
    name: 'duplicate-scopes',
    covers: 'Repeats are a set, not a multiset.',
    scope: ['doc.render', 'doc.render', 'pdf.permissions', 'pdf.permissions'],
    bits: ['copy'],
  },
  {
    name: 'order-does-not-matter',
    covers: 'The same grants written the other way round.',
    scope: ['pdf.permissions', 'doc.render', 'annotations:update:self'],
    bits: ['copy'],
    identity: SELF,
    collab: COLLAB_PROBES,
  },
  {
    name: 'grant-minted-capabilities',
    covers:
      'doc.forms.import, doc.forms.script, doc.annotate.import and doc.outline.modify are in the closed set and imply nothing.',
    scope: ['doc.forms.import', 'doc.forms.script', 'doc.annotate.import', 'doc.outline.modify'],
    bits: ['none'],
  },
  {
    name: 'grant-minted-never-from-bits',
    covers: 'No bit expands to the imports or to scripts; bit 4 or 11 expands to doc.outline.modify.',
    scope: ['pdf.permissions'],
    bits: ['modify', 'assemble', 'all-surfaced'],
  },

  // Field scopes (4b2cdb395). A field's group is who fills it.
  {
    name: 'fields-group-scopes',
    covers: 'A group scope reaches its group only; a field in no group needs :all.',
    scope: ['doc.open', 'fields:fill:group=buyer', 'fields:sign:group=buyer'],
    bits: ['none'],
    identity: SELF,
    fields: FIELD_PROBES,
  },
  {
    name: 'fields-implies-forms-read',
    covers: 'Groups limit writes, never reads: any field scope lets the holder see the form.',
    scope: ['fields:fill:group=buyer'],
    bits: ['none'],
    identity: SELF,
    fields: FIELD_PROBES,
  },
  {
    name: 'fields-narrow-per-action',
    covers: 'A fill scope narrows filling and says nothing about signing.',
    scope: ['doc.forms.fill', 'doc.sign', 'fields:fill:group=buyer'],
    bits: ['none'],
    identity: SELF,
    fields: FIELD_PROBES,
  },
  {
    name: 'fields-broad-capability-decides',
    covers: 'Without a fields scope, doc.forms.fill decides for every field, and does not sign.',
    scope: ['doc.forms.fill'],
    bits: ['none'],
    identity: SELF,
    fields: FIELD_PROBES,
  },
  {
    name: 'fields-with-pdf-permissions',
    covers: 'pdf.permissions stays what the file allows, the fields scope narrows writing, and design is never narrowed.',
    scope: ['pdf.permissions', 'fields:*:group=buyer'],
    bits: ['annotate-and-modify', 'none'],
    identity: SELF,
    fields: FIELD_PROBES,
  },
  {
    name: 'fields-all-filter',
    covers: ':all reaches a field in no group as well.',
    scope: ['fields:*:all'],
    bits: ['none'],
    identity: SELF,
    fields: FIELD_PROBES,
    fieldSetGroup: FIELD_SET_GROUP_PROBES,
  },
  {
    name: 'fields-set-group',
    covers: 'Your own default group always; another with fields:set-group for it.',
    scope: ['fields:set-group:group=buyer'],
    bits: ['none'],
    identity: SELF,
    setGroup: SET_GROUP_PROBES,
    fieldSetGroup: FIELD_SET_GROUP_PROBES,
  },
  {
    name: 'set-group-is-per-entity',
    covers: "An annotation set-group scope does not put fields in groups, and the other way round.",
    scope: ['annotations:set-group:all'],
    bits: ['none'],
    identity: SELF,
    setGroup: SET_GROUP_PROBES,
    fieldSetGroup: FIELD_SET_GROUP_PROBES,
  },
  {
    name: 'fields-scope-is-not-an-annotation-scope',
    covers: 'A fields scope does not narrow annotation collab or imply doc.annotate.read.',
    scope: ['doc.annotate.modify', 'fields:*:all'],
    bits: ['none'],
    identity: SELF,
    collab: COLLAB_PROBES,
    fields: FIELD_PROBES,
  },
] as const;
