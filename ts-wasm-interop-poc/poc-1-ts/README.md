# poc-1-ts

The baseline. Four packages and a demo, all TypeScript, no wasm anywhere. Everything the other two
POCs do is measured against this.

```
packages/geometry    99 lines. Vec, Rect, Quad, rotate/scale, point-in-quad. Zero deps.
packages/shapes     276 lines. The brain: update(msg) -> [model, effects], scene, hitTest, anchor.
packages/store       97 lines. Reducer store with two channels, after core/main/src/store.ts.
packages/react      255 lines. The useSyncExternalStore binding and the ShapeCanvas surface.
apps/demo           Vite + React.
```

Those are the real counts, and they are two orders of magnitude below the packages they stand in
for. The point is the SHAPE of the dependency pyramid, not its size: a zero-dependency leaf, a pure
brain above it, a store above that, and the only DOM in the React package.

Run it:

```
pnpm install
pnpm dev            # http://localhost:5171
pnpm test           # 81 tests
pnpm gen:vectors    # regenerates ../vectors/anchor-vectors.json. Deliberate only.
```

## The two things worth reading

`packages/shapes/src/anchor.ts` is the function the whole exercise is about. An anchored shape is
exempt from the view's zoom, so its effective geometry is the stored geometry scaled by `1/max(zoom, 1)`
and counter-rotated, both about its bounds top-left. `anchorFactors` returns `null` when the
projection is the identity, and `projectQuad` then returns the caller's own object:

```ts
export function projectQuad(q: Quad, view: ViewEnv, anchored: boolean): Quad {
  const f = anchorFactors(anchored, view);
  if (!f) return q;   // same object, not a copy
  ...
}
```

That `return q` is free here and is asserted with `toBe`, not `toEqual`, in
`packages/shapes/src/shapes.test.ts`. Watch what happens to it in poc-2.

`packages/shapes/src/scene.ts` is `scene(model, view) -> DisplayList`: one call for the whole frame
rather than one per shape. In pure TypeScript that choice is invisible, which is exactly why it is
worth making now. poc-3 turns it into the difference between 1 crossing per frame and 1,400.

## The clamp

`anchorFactors` clamps to `max(zoom, 1)`, so below 100% an anchored shape scales with the page
instead of staying screen-constant. That is Adobe's rule and it is load-bearing: a screen-constant
shape at 25% zoom would dwarf the thing it annotates. `packages/shapes/src/shapes.test.ts` pins it.
