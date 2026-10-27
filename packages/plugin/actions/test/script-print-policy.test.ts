/**
 * A script's `print()` reaches the print door with the dispatch's origin
 * (`uiContext`). The kernel's printer refuses every request that isn't the
 * user's. Without a printer, the print policy row for that origin decides
 * before the adapter is called.
 */
import { describe, expect, it, vi } from 'vitest';

import type { ActionDiagnostic, ActionOrigin, ActionPolicy } from '../src/contract';
import { ACTIONS_DEFAULTS } from '../src/contract';
import { createDocumentEvents } from '../src/lifecycle/document-events';
import type { ActionsServices } from '../src/services';

function printDoor(options: { printer: boolean; print: ActionPolicy['print'] }) {
  const adapterPrint = vi.fn();
  const kernelPrint = vi.fn(async () => {});
  const services = {
    events: { reportDiagnostic: vi.fn() },
    catalog: { readDocumentActions: async () => null, DOC_EVENT_TREES: {} },
    queue: { enqueue: <T>(run: () => T) => run(), budget: { scriptNodes: 0 } },
    ports: { slots: { uiAdapter: { openUri: vi.fn(), print: adapterPrint } } },
    authority: { allowsPrint: () => true },
    documentPrint: { available: () => options.printer, start: kernelPrint },
    printLatch: { active: false },
    settings: { get: () => ({ triggers: { document: false } }) },
    policy: { current: () => ({ ...ACTIONS_DEFAULTS.policy, print: options.print }) },
  } as unknown as ActionsServices;
  const events = createDocumentEvents(
    services,
    { ensureOpenSequenceBeforeDocEvent: async () => {} },
    { runAndEmit: vi.fn() } as never,
  );
  const fire = async (origin: ActionOrigin) => {
    const diagnostics: ActionDiagnostic[] = [];
    const outcome = await events.firePrintThroughAdapter({ origin, phase: 'user' }, (diagnostic) =>
      diagnostics.push(diagnostic),
    );
    return { outcome, diagnostics };
  };
  return { fire, adapterPrint, kernelPrint };
}

const BLOCK_ALL = { user: 'block', hover: 'block', lifecycle: 'block' } as const;

describe("a script's print() and the print policy row", () => {
  it('without a printer, a blocking row stops it before the adapter', async () => {
    const door = printDoor({ printer: false, print: BLOCK_ALL });
    const { outcome, diagnostics } = await door.fire('hover');
    expect(outcome).toEqual({ status: 'blocked', detail: "policy 'block'" });
    expect(diagnostics).toEqual([
      { code: 'blocked', message: "print: policy 'block' for origin 'hover'" },
    ]);
    expect(door.adapterPrint).not.toHaveBeenCalled();
  });

  it('without a printer, an adapter row lets it through', async () => {
    const door = printDoor({
      printer: false,
      print: { user: 'adapter', hover: 'adapter', lifecycle: 'adapter' },
    });
    const { outcome } = await door.fire('hover');
    expect(outcome).toEqual({ status: 'executed' });
    expect(door.adapterPrint).toHaveBeenCalledTimes(1);
  });

  it("with the kernel's printer, the refusal is the printer path's own", async () => {
    const door = printDoor({ printer: true, print: BLOCK_ALL });
    const { outcome, diagnostics } = await door.fire('hover');
    expect(outcome).toEqual({ status: 'blocked', detail: 'hover print request' });
    expect(diagnostics).toEqual([
      { code: 'blocked', message: 'print: a hover request never prints' },
    ]);
    expect(door.kernelPrint).not.toHaveBeenCalled();
    expect(door.adapterPrint).not.toHaveBeenCalled();
  });
});
