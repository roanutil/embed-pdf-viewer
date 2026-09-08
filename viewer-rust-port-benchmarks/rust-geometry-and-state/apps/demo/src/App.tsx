import { useEffect, useMemo, useState } from 'react';
import { createCoreStore, initCore, resetCrossings } from '@poc/core';
import wasmUrl from '@poc/core/wasm-url';
import type { CoreStore, Effect, Rotation, ViewEnv } from '@poc/core';
import { markFirstFrame, markWasmReady } from './marks';
import {
  CoreContext,
  ShapeCanvas,
  nextColor,
  resetStats,
  useCore,
  useDispatch,
  useInteropStats,
  useScene,
  useSelected,
  useShapeCount,
} from '@poc/react';

const PAGE = { width: 600, height: 420 };

/**
 * The init gate again, but note where it is: one await at the application root,
 * which is where an application already has a lifecycle. rust-geometry-only pushed the same
 * await down into a geometry leaf that a dozen pure functions called, and every
 * one of them became async-adjacent. Same tax, much better placement.
 */
export function App() {
  const [store, setStore] = useState<CoreStore | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [hostLines, setHostLines] = useState<string[]>([]);

  useEffect(() => {
    let live = true;
    let created: CoreStore | null = null;
    initCore({ wasmUrl })
      .then(() => {
        if (!live) return;
        markWasmReady();
        created = createCoreStore({
          log: (m) => setHostLines((l) => [m, ...l].slice(0, 6)),
        });
        created.dispatch({ t: 'add', at: { x: 180, y: 150 }, color: nextColor(0) });
        created.dispatch({ t: 'add', at: { x: 380, y: 260 }, color: nextColor(1) });
        created.dispatch({ t: 'setAnchored', id: 's1', anchored: true });
        setStore(created);
      })
      .catch((e: unknown) => live && setError(String(e)));
    return () => {
      live = false;
      created?.destroy();
    };
  }, []);

  if (error) return <div className="app gate error">wasm init failed: {error}</div>;
  if (!store) return <div className="app gate">initializing wasm…</div>;
  return (
    <CoreContext.Provider value={store}>
      <Shell hostLines={hostLines} />
    </CoreContext.Provider>
  );
}

function Shell({ hostLines }: { hostLines: string[] }) {
  const [zoom, setZoom] = useState(1);
  const [rotation, setRotation] = useState<Rotation>(0);
  const [log, setLog] = useState<string[]>([]);
  const view: ViewEnv = useMemo(() => ({ zoom, rotation }), [zoom, rotation]);

  const dispatch = useDispatch();
  const scene = useScene(view);
  const selected = useSelected();
  const shapeCount = useShapeCount();
  const stats = useInteropStats();

  const store = useCore();
  useEffect(markFirstFrame, []);

  useEffect(() => {
    const off = store.onEffect((e: Effect) => {
      if (e.t === 'log') setLog((l) => [e.message, ...l].slice(0, 8));
      if (e.t === 'persist') setLog((l) => [`persist ${e.ids.join(', ')}`, ...l].slice(0, 8));
    });
    return off;
  }, [store]);

  return (
    <div className="app">
      <header>
        <h1>Shape canvas — Rust core</h1>
        <p className="sub">
          Geometry, the brain, and the store all live in Rust. TypeScript holds a handle, a version
          number, and an id cache. Seed 200 shapes and sweep the zoom: the crossing count rises by
          one per frame, not by 1,400.
        </p>
      </header>

      <div className="layout">
        <main>
          <ShapeCanvas view={view} scene={scene} page={PAGE} />
        </main>

        <aside>
          <section>
            <h2>View</h2>
            <label>
              zoom <b>{zoom.toFixed(2)}×</b>
              <input
                type="range"
                min={0.25}
                max={4}
                step={0.05}
                value={zoom}
                onChange={(e) => setZoom(Number(e.target.value))}
              />
            </label>
            <div className="row">
              {([0, 90, 180, 270] as Rotation[]).map((r) => (
                <button key={r} className={r === rotation ? 'on' : ''} onClick={() => setRotation(r)}>
                  {r}°
                </button>
              ))}
            </div>
          </section>

          <section>
            <h2>Scene size</h2>
            <div className="row">
              {[10, 50, 200].map((n) => (
                <button key={n} onClick={() => store.seed(n, true)}>
                  {n}
                </button>
              ))}
            </div>
            <p className="note">All anchored, so every shape projects. {shapeCount} on the page.</p>
          </section>

          <section>
            <h2>Selected {selected ? <code>{selected.id}</code> : <span className="dim">none</span>}</h2>
            {selected ? (
              <>
                <label className="check">
                  <input
                    type="checkbox"
                    checked={selected.anchored}
                    onChange={(e) =>
                      dispatch({ t: 'setAnchored', id: selected.id, anchored: e.target.checked })
                    }
                  />
                  anchored (zoom-exempt)
                </label>
                <label>
                  rot <b>{selected.rot}°</b>
                  <input
                    type="range"
                    min={0}
                    max={359}
                    value={selected.rot}
                    onChange={(e) => dispatch({ t: 'setRot', id: selected.id, rot: Number(e.target.value) })}
                  />
                </label>
                <button className="danger" onClick={() => dispatch({ t: 'delete', id: selected.id })}>
                  delete
                </button>
              </>
            ) : (
              <p className="dim">Click a shape.</p>
            )}
          </section>

          <section>
            <h2>Interop</h2>
            <dl className="stats">
              <dt>implementation</dt>
              <dd>{stats.kind}</dd>
              <dt>scene() calls</dt>
              <dd>{stats.sceneCalls}</dd>
              <dt>hit tests</dt>
              <dd>{stats.hitTests}</dd>
              <dt>wasm crossings</dt>
              <dd className="hot">{stats.crossings}</dd>
              <dt>crossings / frame</dt>
              <dd>{stats.sceneCalls > 0 ? (stats.crossings / stats.sceneCalls).toFixed(2) : '—'}</dd>
            </dl>
            <p className="note">
              One per frame in the steady state, three on the first frame after the shape set
              changes, two after any other change.
            </p>
            <button
              onClick={() => {
                resetCrossings();
                resetStats();
              }}
            >
              reset counters
            </button>
          </section>

          <section>
            <h2>Rust → TypeScript</h2>
            <ul className="log">
              {hostLines.length === 0 ? (
                <li className="dim">nothing yet</li>
              ) : (
                hostLines.map((l, i) => <li key={i}>{l}</li>)
              )}
            </ul>
          </section>

          <section>
            <h2>Effects</h2>
            <ul className="log">
              {log.length === 0 ? <li className="dim">nothing yet</li> : log.map((l, i) => <li key={i}>{l}</li>)}
            </ul>
          </section>
        </aside>
      </div>
    </div>
  );
}
