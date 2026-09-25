/**
 * Every `kind` the worker protocol accepts, read out of the TypeScript type
 * rather than off a regular expression.
 *
 * `WorkerRequest` is a union of 63 members, one of which (`OpenWorkerRequest`)
 * is itself a union alias over three `open.*` kinds, so the flat set has 65
 * entries. Counting them by grepping for `kind: '` also picks up the four
 * `LayerOpenSource` variants nested inside the open requests and the four
 * response and lifecycle kinds, which is how the number 64 gets quoted: that
 * is the `case`-arm count in `WorkerHost.ts`, which handles `abort` before the
 * switch. Asking the compiler avoids the whole argument.
 */
import { dirname, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

import ts from 'typescript';

const here = dirname(fileURLToPath(import.meta.url));
const repoRoot = resolve(here, '../../..');

export const WORKER_PROTOCOL = resolve(
  repoRoot,
  'packages/engine/core/src/wire/worker-protocol.ts',
);

/** The four frames that steer the worker rather than asking it for a result. */
export const CONTROL_FRAMES = ['abort', 'close', 'layer.close', 'shutdown'] as const;

/**
 * Reads the flat set of request kinds from the `WorkerRequest` union.
 *
 * Throws rather than returning an empty set when the alias moves or the union
 * stops being one: a checker that silently finds nothing passes.
 */
export function readWorkerRequestKinds(): string[] {
  const program = ts.createProgram([WORKER_PROTOCOL], {
    target: ts.ScriptTarget.ES2022,
    module: ts.ModuleKind.ESNext,
    moduleResolution: ts.ModuleResolutionKind.Bundler,
    strict: true,
    noEmit: true,
    skipLibCheck: true,
  });
  const checker = program.getTypeChecker();
  const source = program.getSourceFile(WORKER_PROTOCOL);
  if (!source) throw new Error(`could not load ${WORKER_PROTOCOL}`);

  let alias: ts.TypeAliasDeclaration | undefined;
  source.forEachChild((node) => {
    if (ts.isTypeAliasDeclaration(node) && node.name.text === 'WorkerRequest') alias = node;
  });
  if (!alias) throw new Error(`no WorkerRequest type alias in ${WORKER_PROTOCOL}`);

  const type = checker.getTypeAtLocation(alias.name);
  const members = type.isUnion() ? type.types : [type];

  const kinds = new Set<string>();
  for (const member of members) {
    const property = member.getProperty('kind');
    if (!property) {
      throw new Error(`a WorkerRequest member has no 'kind': ${checker.typeToString(member)}`);
    }
    const kindType = checker.getTypeOfSymbolAtLocation(property, alias.name);
    for (const literal of kindType.isUnion() ? kindType.types : [kindType]) {
      if (!literal.isStringLiteral()) {
        throw new Error(
          `a WorkerRequest 'kind' is not a string literal: ${checker.typeToString(literal)}`,
        );
      }
      kinds.add(literal.value);
    }
  }
  return [...kinds].sort();
}

/** One argument a request carries, besides the envelope's own four fields. */
export interface WorkerRequestField {
  readonly name: string;
  readonly optional: boolean;
  /** The declared type, as the checker prints it. Prose, not a schema. */
  readonly type: string;
}

/** One request kind and everything it takes. */
export interface WorkerRequestShape {
  readonly kind: string;
  readonly fields: readonly WorkerRequestField[];
}

/**
 * The four fields every request carries, which say nothing about what the
 * operation does. `layerName` is here because it selects the SESSION rather
 * than being an argument: every session-scoped kind accepts it and none of
 * them means anything different by it.
 */
const ENVELOPE = new Set(['kind', 'jobId', 'docId', 'layerName']);

/**
 * Every request kind with its arguments, read off the union.
 *
 * Asked of the compiler rather than matched out of the source, for the same
 * reason {@link readWorkerRequestKinds} is: a member whose fields come from a
 * shared base interface or an alias is invisible to a regular expression and
 * fully visible to the checker.
 */
export function readWorkerRequestShapes(): WorkerRequestShape[] {
  const program = ts.createProgram([WORKER_PROTOCOL], {
    target: ts.ScriptTarget.ES2022,
    module: ts.ModuleKind.ESNext,
    moduleResolution: ts.ModuleResolutionKind.Bundler,
    strict: true,
    noEmit: true,
    skipLibCheck: true,
  });
  const checker = program.getTypeChecker();
  const source = program.getSourceFile(WORKER_PROTOCOL);
  if (!source) throw new Error(`could not load ${WORKER_PROTOCOL}`);

  let alias: ts.TypeAliasDeclaration | undefined;
  source.forEachChild((node) => {
    if (ts.isTypeAliasDeclaration(node) && node.name.text === 'WorkerRequest') alias = node;
  });
  if (!alias) throw new Error(`no WorkerRequest type alias in ${WORKER_PROTOCOL}`);

  const type = checker.getTypeAtLocation(alias.name);
  const members = type.isUnion() ? type.types : [type];

  const shapes = new Map<string, WorkerRequestShape>();
  for (const member of members) {
    const kindProperty = member.getProperty('kind');
    if (!kindProperty) continue;
    const kindType = checker.getTypeOfSymbolAtLocation(kindProperty, alias.name);

    const fields: WorkerRequestField[] = [];
    for (const property of member.getProperties()) {
      if (ENVELOPE.has(property.name)) continue;
      const declared = checker.getTypeOfSymbolAtLocation(property, alias.name);
      fields.push({
        name: property.name,
        optional: (property.flags & ts.SymbolFlags.Optional) !== 0,
        // `undefined` is stripped: it is what `optional` already says, and
        // leaving it in would make every optional field's type read as a
        // union of itself.
        type: checker
          .typeToString(checker.getNonNullableType(declared))
          .replace(/\s+/g, ' ')
          .trim(),
      });
    }
    fields.sort((a, b) => a.name.localeCompare(b.name));

    for (const literal of kindType.isUnion() ? kindType.types : [kindType]) {
      if (!literal.isStringLiteral()) continue;
      shapes.set(literal.value, { kind: literal.value, fields });
    }
  }
  return [...shapes.values()].sort((a, b) => a.kind.localeCompare(b.kind));
}
