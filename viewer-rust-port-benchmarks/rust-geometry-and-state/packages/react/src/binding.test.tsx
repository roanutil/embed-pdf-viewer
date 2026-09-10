import { act, cleanup, render, screen } from '@testing-library/react';
import { afterEach, beforeAll, describe, expect, it } from 'vitest';
import { createCoreStore, crossings, initCore, resetCrossings } from '@poc/core';
import { initCoreFromDisk } from '@poc/core/node';
import type { CoreStore, Rotation, ViewEnv } from '@poc/core';
import { CoreContext, ShapeCanvas, useScene, useSelected, useVersion } from './index';

const V = (zoom: number, rotation: Rotation = 0): ViewEnv => ({ zoom, rotation });

beforeAll(async () => {
  await initCoreFromDisk();
  await initCore(); // idempotent second call must not re-instantiate
});

// Testing Library only auto-cleans when vitest globals are on. They are not, so
// without this every test's container stays in the document and getByTestId
// finds several.
afterEach(cleanup);

function Harness({ store, view }: { store: CoreStore; view: ViewEnv }) {
  return (
    <CoreContext.Provider value={store}>
      <Probe view={view} />
    </CoreContext.Provider>
  );
}

function Probe({ view }: { view: ViewEnv }) {
  const version = useVersion();
  const scene = useScene(view);
  const selected = useSelected();
  return (
    <div>
      <span data-testid="version">{version}</span>
      <span data-testid="count">{scene.items.length}</span>
      <span data-testid="selected">{selected?.id ?? 'none'}</span>
      <span data-testid="projected">{scene.items.filter((i) => i.projected).length}</span>
      <ShapeCanvas view={view} scene={scene} page={{ width: 600, height: 420 }} />
    </div>
  );
}

describe('the React binding over a WASM-owned store', () => {
  it('renders the display list and re-renders when Rust changes', () => {
    const store = createCoreStore();
    const { rerender } = render(<Harness store={store} view={V(1)} />);

    expect(screen.getByTestId('count').textContent).toBe('0');
    const v0 = Number(screen.getByTestId('version').textContent);

    act(() => {
      store.dispatch({ t: 'add', at: { x: 180, y: 150 }, color: '#c0392b' });
    });

    // useSyncExternalStore saw Rust's version move and pulled a new frame.
    expect(Number(screen.getByTestId('version').textContent)).toBeGreaterThan(v0);
    expect(screen.getByTestId('count').textContent).toBe('1');
    expect(screen.getByTestId('selected').textContent).toBe('s1');

    // One <path> per shape, drawn from the decoded quad.
    expect(document.querySelectorAll('.canvas path')).toHaveLength(1);

    act(() => {
      store.dispatch({ t: 'setAnchored', id: 's1', anchored: true });
    });
    expect(screen.getByTestId('projected').textContent).toBe('0');

    // A pure view change, no dispatch. The projection flag flips.
    rerender(<Harness store={store} view={V(3)} />);
    expect(screen.getByTestId('projected').textContent).toBe('1');

    store.destroy();
  });

  it('does not loop: a re-render with the same version reuses the frame', () => {
    const store = createCoreStore();
    store.seed(25, true);
    const view = V(2, 90);
    const { rerender } = render(<Harness store={store} view={view} />);
    expect(screen.getByTestId('count').textContent).toBe('25');

    resetCrossings();
    // Same store, same view object, same version: every memo holds and the
    // mirrored version costs nothing, so a re-render is FREE. If getVersion()
    // read across the boundary this would be six crossings per re-render.
    for (let i = 0; i < 5; i++) rerender(<Harness store={store} view={view} />);
    expect(crossings()).toBe(0);

    store.destroy();
  });

  it('renders 200 shapes for one crossing per frame', () => {
    const store = createCoreStore();
    store.seed(200, true);
    const { rerender } = render(<Harness store={store} view={V(1)} />);
    expect(screen.getByTestId('count').textContent).toBe('200');
    expect(document.querySelectorAll('.canvas path')).toHaveLength(200);

    resetCrossings();
    rerender(<Harness store={store} view={V(2, 90)} />);
    // A new view on 200 shapes: exactly one scene() hop. The shape count does
    // not enter into it, which is the entire claim of this POC.
    expect(crossings()).toBe(1);
    expect(document.querySelectorAll('.canvas path')).toHaveLength(200);

    store.destroy();
  });
});
