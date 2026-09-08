import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import {
  benchHostCalls,
  initGeometry,
  rectQuad,
  rectQuadObjects,
  resetCrossings,
  setHostLogger,
} from '@poc/geometry';
import wasmUrl from '@poc/geometry/wasm-url';
import { createStore } from '@poc/store';
import type { Effect, Rotation, ViewEnv } from '@poc/shapes';
import { markFirstFrame, markWasmReady } from './marks';
import {
  ShapeCanvas,
  StoreContext,
  nextColor,
  resetStats,
  useDispatch,
  useInteropStats,
  useModel,
  useScene,
} from '@poc/react';

const PAGE = { width: 600, height: 420 };

/**
 * The init gate. This component exists ONLY because geometry moved to wasm.
 *
 * In typescript-baseline the app rendered immediately. Here the whole tree waits on an
 * `await`, because the leaf at the bottom of the pyramid is now asynchronous.
 * Nothing above it asked for that.
 */
export function App() {
  const [ready, setReady] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [hostLines, setHostLines] = useState<string[]>([]);
  const live = useRef(true);

  // Stable identity on purpose. The bench swaps the host logger out for a
  // trivial one and has to put THIS one back, so it cannot be a fresh closure
  // per render.
  const hostLog = useCallback((m: string) => {
    if (live.current) setHostLines((l) => [m, ...l].slice(0, 6));
  }, []);

  useEffect(() => {
    live.current = true;
    initGeometry({ wasmUrl, host: { log: hostLog } })
      .then(() => {
        if (!live.current) return;
        markWasmReady();
        setReady(true);
      })
      .catch((e: unknown) => live.current && setError(String(e)));
    return () => {
      live.current = false;
    };
  }, [hostLog]);

  if (error) return <div className="app gate error">wasm init failed: {error}</div>;
  if (!ready) return <div className="app gate">initializing wasm…</div>;
  return <Ready hostLines={hostLines} hostLog={hostLog} />;
}

function Ready({ hostLines, hostLog }: { hostLines: string[]; hostLog: (m: string) => void }) {
  // Seeded at CREATION, not in an effect. StrictMode double-invokes effects, and
  // a seeding effect would put four shapes on the page instead of two.
  const store = useMemo(() => {
    const s = createStore();
    s.dispatch({ t: 'add', at: { x: 180, y: 150 }, color: nextColor(0) });
    s.dispatch({ t: 'add', at: { x: 380, y: 260 }, color: nextColor(1) });
    s.dispatch({ t: 'setAnchored', id: 's1', anchored: true });
    return s;
  }, []);
  const [log, setLog] = useState<string[]>([]);

  useEffect(() => {
    const off = store.onEffect((e: Effect) => {
      if (e.t === 'log') setLog((l) => [e.message, ...l].slice(0, 8));
      if (e.t === 'persist') setLog((l) => [`persist ${e.ids.join(', ')}`, ...l].slice(0, 8));
    });
    return off;
  }, [store]);

  return (
    <StoreContext.Provider value={store}>
      <Shell log={log} hostLines={hostLines} hostLog={hostLog} />
    </StoreContext.Provider>
  );
}

interface Bench {
  flatUs: number;
  objectUs: number;
  hostUs: number;
}

/**
 * All three arms have to measure the same thing: one boundary crossing and
 * nothing else.
 *
 * That is why the host arm swaps the logger. The app's logger appends to React
 * state, so timing `benchHostCalls(20000)` with it attached timed 20,000
 * setState enqueues plus the crossings, while the flat and object arms timed
 * only the crossings. The number that came out was quoted as evidence that
 * calling back into JS is the cheapest direction.
 */
function runBench(hostLog: (m: string) => void): Bench {
  const N = 20000;
  const rect = { x: 10, y: 20, width: 80, height: 40 };

  resetCrossings();
  let t = performance.now();
  for (let i = 0; i < N; i++) rectQuad(rect, i % 360);
  const flatUs = ((performance.now() - t) * 1000) / N;

  t = performance.now();
  for (let i = 0; i < N; i++) rectQuadObjects(rect, i % 360);
  const objectUs = ((performance.now() - t) * 1000) / N;

  let landed = 0;
  setHostLogger(() => {
    landed += 1;
  });
  t = performance.now();
  benchHostCalls(N);
  const hostUs = ((performance.now() - t) * 1000) / N;
  setHostLogger(hostLog);
  if (landed !== N) console.warn(`host arm landed ${landed} of ${N} calls`);

  resetCrossings();
  return { flatUs, objectUs, hostUs };
}

function Shell({
  log,
  hostLines,
  hostLog,
}: {
  log: string[];
  hostLines: string[];
  hostLog: (m: string) => void;
}) {
  const [zoom, setZoom] = useState(1);
  const [rotation, setRotation] = useState<Rotation>(0);
  const [bench, setBench] = useState<Bench | null>(null);
  const view: ViewEnv = useMemo(() => ({ zoom, rotation }), [zoom, rotation]);

  const model = useModel();
  const dispatch = useDispatch();
  const scene = useScene(view);
  const stats = useInteropStats();
  useEffect(markFirstFrame, []);
  const selected = model.shapes.find((s) => s.id === model.selected) ?? null;
  const anchoredCount = model.shapes.filter((s) => s.anchored).length;

  return (
    <div className="app">
      <header>
        <h1>Shape canvas — Rust geometry</h1>
        <p className="sub">
          Same brain, same store, same canvas as typescript-baseline. Only <code>@poc/geometry</code> moved to
          Rust. Watch the crossing meter as you zoom with an anchored shape selected.
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
              <dt>shapes / anchored</dt>
              <dd>
                {model.shapes.length} / {anchoredCount}
              </dd>
            </dl>
            <p className="note">
              An unanchored shape costs 1 crossing per frame. An anchored one, at a projecting view,
              costs 7.
            </p>
            <button onClick={resetStats}>reset counters</button>
          </section>

          <section>
            <h2>Cost per call</h2>
            {bench ? (
              <dl className="stats">
                <dt>flat Float64Array</dt>
                <dd>{bench.flatUs.toFixed(3)} µs</dd>
                <dt>serde objects</dt>
                <dd>{bench.objectUs.toFixed(3)} µs</dd>
                <dt>Rust → TS callback</dt>
                <dd>{bench.hostUs.toFixed(3)} µs</dd>
              </dl>
            ) : (
              <p className="dim">20,000 calls each.</p>
            )}
            <button onClick={() => setBench(runBench(hostLog))}>{bench ? 'run again' : 'measure'}</button>
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
