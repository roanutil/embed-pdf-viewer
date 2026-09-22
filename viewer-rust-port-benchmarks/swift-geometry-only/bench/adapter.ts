import { initGeometry, normalizeDeg, pointInQuad, quadBounds, quadRotateAbout, quadScaleAbout, rectQuad } from '../geometry';

export const name = 'swift-geometry-only';
export async function init() { await initGeometry(); }

const shapeQuad = (shape: any, view: any) => {
  let quad = rectQuad(shape.rect, shape.rot);
  if (!shape.anchored) return quad;
  const scale = Math.max(view.zoom || 1, 1);
  const rotation = normalizeDeg(view.rotation);
  if (scale === 1 && rotation === 0) return quad;
  const bounds = quadBounds(quad);
  const anchor = { x: bounds.x, y: bounds.y };
  if (scale !== 1) quad = quadScaleAbout(quad, anchor, 1 / scale);
  if (rotation !== 0) quad = quadRotateAbout(quad, anchor, normalizeDeg(-rotation));
  return quad;
};

const projected = (shape: any, view: any) =>
  shape.anchored && (Math.max(view.zoom || 1, 1) !== 1 || normalizeDeg(view.rotation) !== 0);

export function build(layout: any[]) {
  let shapes = layout.map((item, index) => ({
    id: `s${index + 1}`,
    rect: { x: item.x - 45, y: item.y - 32, width: 90, height: 64 },
    rot: 0,
    anchored: item.anchored,
    color: item.color,
  }));
  let selected: string | null = null;
  let drag: any = null;

  const displayList = (view: any) => shapes.map((shape) => ({
    id: shape.id,
    quad: shapeQuad(shape, view),
    color: shape.color,
    selected: shape.id === selected,
    projected: projected(shape, view),
  }));
  const hit = (point: any, view: any) => {
    for (let index = shapes.length - 1; index >= 0; index--) {
      if (pointInQuad(point, shapeQuad(shapes[index], view))) return shapes[index].id;
    }
    return null;
  };

  return {
    frame(view: any) {
      const items = displayList(view);
      const last = items[items.length - 1];
      return items.length + items[0].quad[0].x + last.quad[2].y;
    },
    displayList(view: any) { return displayList(view); },
    hit(point: any, view: any) { return hit(point, view) === null ? 0 : 1; },
    gesture(down: any, view: any, moves: any[]) {
      const id = hit(down, view);
      if (id) {
        const shape = shapes.find((candidate) => candidate.id === id)!;
        selected = id;
        drag = { id, from: down, origin: shape.rect };
      } else { selected = null; }
      for (const at of moves) {
        if (!drag) continue;
        const dx = at.x - drag.from.x, dy = at.y - drag.from.y;
        shapes = shapes.map((shape) => shape.id === drag.id ? {
          ...shape,
          rect: { ...drag.origin, x: drag.origin.x + dx, y: drag.origin.y + dy },
        } : shape);
      }
      drag = null;
      return shapes[0].rect.x;
    },
    dispose() {},
  };
}
