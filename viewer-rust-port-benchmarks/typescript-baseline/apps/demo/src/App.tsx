import { useEffect, useMemo, useState } from 'react';
import { createStore } from '@poc/store';
import type { Effect } from '@poc/shapes';
import { ShapeCanvas, StoreContext, nextColor, resetStats, useDispatch, useInteropStats, useModel, useScene } from '@poc/react';
import type { Rotation, ViewEnv } from '@poc/shapes';
import { markFirstFrame, markWasmReady } from './marks';

const PAGE = { width: 600, height: 420 };

export function App() {
  // Nothing to instantiate. Recording it anyway keeps the four legs comparable
  // across the three builds.
  useEffect(markWasmReady, []);

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
      <Shell log={log} />
    </StoreContext.Provider>
  );
}

function Shell({ log }: { log: string[] }) {
  const [zoom, setZoom] = useState(1);
  const [rotation, setRotation] = useState<Rotation>(0);
  const view: ViewEnv = useMemo(() => ({ zoom, rotation }), [zoom, rotation]);

  const model = useModel();
  const dispatch = useDispatch();
  const scene = useScene(view);
  const stats = useInteropStats();
  useEffect(markFirstFrame, []);
  const selected = model.shapes.find((s) => s.id === model.selected) ?? null;

  return (
    <div className="app">
      <header>
        <h1>Shape canvas</h1>
        <p className="sub">
          Double-click empty space to add. Drag to move. The ringed corner marks a shape the current
          view actually projects.
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
              <dt>language crossings</dt>
              <dd>{stats.crossings}</dd>
            </dl>
            <button onClick={resetStats}>reset counters</button>
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
