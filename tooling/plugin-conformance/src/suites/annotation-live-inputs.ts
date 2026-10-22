/** Direct scenario inputs for the interaction and text-selection capabilities. */
import type { Step } from '../scenario';
import { pr } from './annotation-live-fixtures';
export const ix = (method: string, args: readonly unknown[] = [], doc = 'a'): Step => ({
  op: 'call',
  target: { token: 'interaction', doc },
  method,
  args,
});
export const sel = (method: string, args: readonly unknown[] = [], doc = 'a'): Step => ({
  op: 'call',
  target: { token: 'selection', doc },
  method,
  args,
});
export const sample = (
  phase: string,
  x: number,
  y: number,
  over: Record<string, unknown> = {},
  page = 1,
) => ({
  phase,
  viewport: { x, y },
  page: { ref: pr(page), point: { x, y }, scale: 1 },
  modifiers: { shift: false, alt: false, ctrl: false, meta: false },
  ...over,
});
export const tool = (id: string): Step => ix('activateTool', [id]);
import { actionsPlugin } from '../../../../packages/plugin/actions/src/actions.plugin';
import { ActionsToken } from '../../../../packages/plugin/actions/src/host-contract';
import { renderPlugin } from '../../../../packages/plugin/render/src/render.plugin';
import { RenderToken } from '../../../../packages/plugin/render/src/host-contract';
import { selectionPlugin } from '../../../../packages/plugin/selection/src/selection.plugin';
import { SelectionToken } from '../../../../packages/plugin/selection/src/host-contract';
import { registerPlugin } from '../scenario';
registerPlugin('annotation-live-actions', {
  factory: () => actionsPlugin({ openSequence: 'off', javascript: { enabled: false } }),
  tokens: [ActionsToken],
});
registerPlugin('annotation-live-render', { factory: () => renderPlugin(), tokens: [RenderToken] });
registerPlugin('annotation-live-selection', {
  factory: () => selectionPlugin(),
  tokens: [SelectionToken],
});
