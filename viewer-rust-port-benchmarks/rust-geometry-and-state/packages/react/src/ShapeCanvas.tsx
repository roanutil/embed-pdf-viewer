import { useCallback, useRef } from 'react';
import type { PointerEvent as ReactPointerEvent } from 'react';
import type { DisplayList, Quad, ViewEnv } from '@poc/core';
import { useCore } from './context';
import { useDispatch } from './hooks';
import { bumpStats } from './stats';

const quadPath = (q: Quad): string =>
  `${q.map((p, i) => `${i === 0 ? 'M' : 'L'}${p.x.toFixed(2)},${p.y.toFixed(2)}`).join(' ')} Z`;

export interface ShapeCanvasProps {
  view: ViewEnv;
  scene: DisplayList;
  page: { width: number; height: number };
}

/**
 * Unchanged from typescript-baseline and rust-geometry-only apart from where `isDragging` and the add-hit
 * check come from. The renderer never learned that the core moved languages,
 * which is the return on keeping the boundary at `scene()` and `dispatch()`.
 */
export function ShapeCanvas({ view, scene, page }: ShapeCanvasProps) {
  const store = useCore();
  const dispatch = useDispatch();
  const contentRef = useRef<SVGGElement | null>(null);

  const toContent = useCallback((e: ReactPointerEvent<SVGSVGElement>) => {
    const g = contentRef.current;
    if (!g) return null;
    const ctm = g.getScreenCTM();
    if (!ctm) return null;
    const inv = ctm.inverse();
    return {
      x: e.clientX * inv.a + e.clientY * inv.c + inv.e,
      y: e.clientX * inv.b + e.clientY * inv.d + inv.f,
    };
  }, []);

  const onPointerDown = useCallback(
    (e: ReactPointerEvent<SVGSVGElement>) => {
      const at = toContent(e);
      if (!at) return;
      e.currentTarget.setPointerCapture(e.pointerId);
      bumpStats({ hitTests: 1 });
      dispatch({ t: 'pointerDown', at, view });
    },
    [dispatch, toContent, view],
  );

  const onPointerMove = useCallback(
    (e: ReactPointerEvent<SVGSVGElement>) => {
      if (!store.isDragging()) return;
      const at = toContent(e);
      if (!at) return;
      dispatch({ t: 'pointerMove', at });
    },
    [dispatch, store, toContent],
  );

  const onPointerUp = useCallback(
    (e: ReactPointerEvent<SVGSVGElement>) => {
      if (e.currentTarget.hasPointerCapture(e.pointerId)) {
        e.currentTarget.releasePointerCapture(e.pointerId);
      }
      dispatch({ t: 'pointerUp' });
    },
    [dispatch],
  );

  const onDoubleClick = useCallback(
    (e: ReactPointerEvent<SVGSVGElement>) => {
      const at = toContent(e);
      if (!at) return;
      bumpStats({ hitTests: 1 });
      if (!store.hitTest(at, view)) {
        // seq(), not shapeCount(): typescript-baseline and rust-geometry-only cycle on `model.seq`, and a
        // delete lowers the count but not the seq, so cycling on the count
        // reissued the colour that had just been handed out.
        dispatch({ t: 'add', at, color: nextColor(store.seq()) });
      }
    },
    [dispatch, store, toContent, view],
  );

  const cx = page.width / 2;
  const cy = page.height / 2;

  return (
    <svg
      className="canvas"
      viewBox={`0 0 ${page.width} ${page.height}`}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={onPointerUp}
      onPointerCancel={onPointerUp}
      onDoubleClick={onDoubleClick as never}
    >
      <g transform={`rotate(${view.rotation} ${cx} ${cy}) scale(${view.zoom}) `}>
        <g ref={contentRef}>
          <rect
            x={0}
            y={0}
            width={page.width}
            height={page.height}
            className="page"
            vectorEffect="non-scaling-stroke"
          />
          {scene.items.map((item) => (
            <g key={item.handle}>
              <path
                d={quadPath(item.quad)}
                fill={item.color}
                fillOpacity={0.72}
                stroke={item.selected ? '#111' : 'none'}
                strokeWidth={2}
                vectorEffect="non-scaling-stroke"
              />
              {item.projected && (
                <circle
                  cx={item.quad[0].x}
                  cy={item.quad[0].y}
                  r={4}
                  fill="none"
                  stroke="#111"
                  strokeWidth={1.5}
                  vectorEffect="non-scaling-stroke"
                />
              )}
            </g>
          ))}
        </g>
      </g>
    </svg>
  );
}

const PALETTE = ['#c0392b', '#2980b9', '#27ae60', '#8e44ad', '#d35400', '#16a085'];
export const nextColor = (seq: number): string => PALETTE[seq % PALETTE.length]!;
