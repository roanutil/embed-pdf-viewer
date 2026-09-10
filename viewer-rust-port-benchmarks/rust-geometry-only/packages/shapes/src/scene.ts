import type { DisplayList, Model, ViewEnv } from './types';
import { isProjected, shapeQuad } from './anchor';

/**
 * The per-frame projection: model plus view in, a flat display list out.
 *
 * This is the ONE function a renderer needs, and its shape is the whole
 * interop lesson. One call per frame over the entire model beats one call
 * per shape, and the difference is invisible in pure TypeScript. It becomes
 * the difference between viable and unviable the moment this runs in WASM.
 */
export function scene(model: Model, view: ViewEnv): DisplayList {
  return {
    items: model.shapes.map((s) => ({
      id: s.id,
      quad: shapeQuad(s, view),
      color: s.color,
      selected: s.id === model.selected,
      projected: isProjected(s, view),
    })),
  };
}
