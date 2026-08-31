import { pointInQuad } from '@poc/geometry';
import type { Vec } from '@poc/geometry';
import type { Effect, Id, Model, Msg, Shape, ViewEnv } from './types';
import { shapeQuad } from './anchor';

const DEFAULT_W = 90;
const DEFAULT_H = 64;

/** Topmost shape under `at`, or null. Reverse order: last drawn wins. */
export function hitTest(model: Model, at: Vec, view: ViewEnv): Id | null {
  for (let i = model.shapes.length - 1; i >= 0; i--) {
    const s = model.shapes[i]!;
    if (pointInQuad(at, shapeQuad(s, view))) return s.id;
  }
  return null;
}

const replace = (shapes: readonly Shape[], id: Id, fn: (s: Shape) => Shape): readonly Shape[] =>
  shapes.map((s) => (s.id === id ? fn(s) : s));

/**
 * The one entry point. Pure: same model and msg give the same result, and
 * nothing outside is touched. Effects are DESCRIPTIONS; the shell performs
 * them.
 */
export function update(model: Model, msg: Msg): [Model, Effect[]] {
  switch (msg.t) {
    case 'add': {
      const id = `s${model.seq + 1}`;
      const shape: Shape = {
        id,
        rect: {
          x: msg.at.x - DEFAULT_W / 2,
          y: msg.at.y - DEFAULT_H / 2,
          width: DEFAULT_W,
          height: DEFAULT_H,
        },
        rot: 0,
        anchored: false,
        color: msg.color,
      };
      return [
        { ...model, shapes: [...model.shapes, shape], selected: id, seq: model.seq + 1 },
        [{ t: 'log', message: `added ${id}` }, { t: 'persist', ids: [id] }],
      ];
    }

    case 'select':
      return [{ ...model, selected: msg.id }, []];

    case 'pointerDown': {
      const hit = hitTest(model, msg.at, msg.view);
      if (!hit) return [{ ...model, selected: null }, []];
      const shape = model.shapes.find((s) => s.id === hit)!;
      return [
        {
          ...model,
          selected: hit,
          drag: { id: hit, from: msg.at, origin: shape.rect, view: msg.view },
        },
        [],
      ];
    }

    case 'pointerMove': {
      const d = model.drag;
      if (!d) return [model, []];
      // An anchored shape moves with the POINTER, not with the page, so the
      // delta is divided by the same factor its geometry is projected through.
      const shape = model.shapes.find((s) => s.id === d.id);
      if (!shape) return [model, []];
      const s = shape.anchored ? Math.max(d.view.zoom || 1, 1) : 1;
      const dx = (msg.at.x - d.from.x) * s;
      const dy = (msg.at.y - d.from.y) * s;
      return [
        {
          ...model,
          shapes: replace(model.shapes, d.id, (sh) => ({
            ...sh,
            rect: { ...d.origin, x: d.origin.x + dx, y: d.origin.y + dy },
          })),
        },
        [],
      ];
    }

    case 'pointerUp': {
      const d = model.drag;
      if (!d) return [model, []];
      return [{ ...model, drag: null }, [{ t: 'persist', ids: [d.id] }]];
    }

    case 'setAnchored':
      return [
        { ...model, shapes: replace(model.shapes, msg.id, (s) => ({ ...s, anchored: msg.anchored })) },
        [{ t: 'log', message: `${msg.id} anchored=${msg.anchored}` }, { t: 'persist', ids: [msg.id] }],
      ];

    case 'setRot':
      return [
        { ...model, shapes: replace(model.shapes, msg.id, (s) => ({ ...s, rot: msg.rot })) },
        [{ t: 'persist', ids: [msg.id] }],
      ];

    case 'delete': {
      const shapes = model.shapes.filter((s) => s.id !== msg.id);
      if (shapes.length === model.shapes.length) return [model, []];
      return [
        {
          ...model,
          shapes,
          selected: model.selected === msg.id ? null : model.selected,
          drag: model.drag?.id === msg.id ? null : model.drag,
        },
        [{ t: 'log', message: `deleted ${msg.id}` }],
      ];
    }
  }
}
