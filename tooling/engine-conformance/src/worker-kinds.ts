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
