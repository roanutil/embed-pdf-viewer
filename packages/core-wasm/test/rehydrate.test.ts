/**
 * `rehydrate` rebuilds a page's transform in JavaScript and throws if the
 * rebuild drifts from the kernel's own numbers.
 */
import { describe, expect, test, vi } from 'vitest';
import { pageTransform } from '@embedpdf/core-geometry';

import { rehydrate } from '../src/rehydrate';

const transform = pageTransform({
  pageSize: { width: 612, height: 792 },
  rotation: 0,
  scale: 1,
  baseScale: 1,
  dpr: 1,
});
const page = { width: 612, height: 792, contentScale: 1, transform: { ...transform } };

describe('rehydrating a page transform', () => {
  // The fixture is consistent, so the throw below is the drift's alone.
  test('a transform the rebuild matches passes', () => {
    expect(() => rehydrate('getPageFrame', page)).not.toThrow();
  });

  test('a transform that drifts from the rebuild throws', () => {
    vi.spyOn(console, 'error').mockImplementation(() => {});
    const drifted = { ...page, transform: { ...transform, zoom: transform.zoom + 0.5 } };
    expect(() => rehydrate('getPageFrame', drifted)).toThrow(/differs from the kernel's in zoom/);
  });
});
