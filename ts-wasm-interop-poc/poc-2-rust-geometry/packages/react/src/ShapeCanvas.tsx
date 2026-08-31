import { useCallback, useRef } from 'react';
import type { PointerEvent as ReactPointerEvent } from 'react';
import { hitTest } from '@poc/shapes';
import type { DisplayList, Quad, ViewEnv } from '@poc/shapes';
import { useDispatch, useModel } from './hooks';
import { bumpStats } from './stats';

const quadPath = (q: Quad): string =>
  `${q.map((p, i) => `${i === 0 ? 'M' : 'L'}${p.x.toFixed(2)},${p.y.toFixed(2)}`).join(' ')} Z`;

export interface ShapeCanvasProps {
  view: ViewEnv;
  scene: DisplayList;
  /** Content-space extent of the page. */
  page: { width: number; height: number };
}

/**
 * The one surface that knows about pointers and SVG.
 *
 * Content space is the SVG's own user space, and the view transform lives on
 * one `<g>`. That means pointer positions convert through the browser's own
 * inverse CTM rather than through hand-rolled trigonometry, so the canvas
 * cannot disagree with the projection about where a shape is.
 */
export function ShapeCanvas({ view, scene, page }: ShapeCanvasProps) {
  const dispatch = useDispatch();
  const model = useModel();
  const contentRef = useRef<SVGGElement | null>(null);

  const toContent = useCallback((e: ReactPointerEvent<SVGSVGElement>) => {
    const g = contentRef.current;
    if (!g) return null;
    const ctm = g.getScreenCTM();
    if (!ctm) return null;
    const inv = ctm.inverse();
    const x = e.clientX * inv.a + e.clientY * inv.c + inv.e;
    const y = e.clientX * inv.b + e.clientY * inv.d + inv.f;
    return { x, y };
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
      if (!model.drag) return;
      const at = toContent(e);
      if (!at) return;
      dispatch({ t: 'pointerMove', at });
    },
    [dispatch, model.drag, toContent],
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
      const at = toContent(e as unknown as ReactPointerEvent<SVGSVGElement>);
      if (!at) return;
      const hit = hitTest(model, at, view);
      bumpStats({ hitTests: 1 });
      if (!hit) dispatch({ t: 'add', at, color: nextColor(model.seq) });
    },
    [dispatch, model, toContent, view],
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
            <g key={item.id}>
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
