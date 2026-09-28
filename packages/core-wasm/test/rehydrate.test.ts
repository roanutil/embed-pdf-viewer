/**
 * `rehydrate` rebuilds a page's transform in JavaScript and throws if the
 * rebuild drifts from the kernel's own numbers.
 */
import { describe, expect, test, vi } from 'vitest';
import { pageTransform } from '@embedpdf/core-geometry';

import type { EngineBridge } from '../src/engine-bridge';
import { Wire } from '../src/proxy';
import { readBinarySources, rehydrate } from '../src/rehydrate';

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

describe('a Blob argument', () => {
  test('a File is read to a payload with its bytes, type and name', async () => {
    const file = new File([new Uint8Array([1, 2, 3])], 'mark.png', { type: 'image/png' });
    expect(await readBinarySources([file, { width: 1 }])).toEqual([
      { data: new Uint8Array([1, 2, 3]), mimeType: 'image/png', name: 'mark.png' },
      { width: 1 },
    ]);
  });

  test("a payload's or attachment's Blob data lends its type and name where they are absent", async () => {
    const file = new File([new Uint8Array([4])], 'notes.txt', { type: 'text/plain' });
    expect(await readBinarySources({ data: file, name: 'kept.txt' })).toEqual({
      data: new Uint8Array([4]),
      name: 'kept.txt',
      mimeType: 'text/plain',
    });
  });

  test('one that reaches the encoder unprepared throws', () => {
    const wire = new Wire({ host: () => undefined, hold: () => 0 } as unknown as EngineBridge);
    const error = vi.spyOn(console, 'error').mockImplementation(() => {});
    expect(() => wire.encode([new Blob(['x'])], undefined, 'importLibrary')).toThrow(
      '[core-wasm] a Blob reached the encoder unprepared: importLibrary',
    );
    expect(error).toHaveBeenCalled();
  });
});

describe('a Blob a callback answers', () => {
  // The default file picker (`filePickerProvider` in
  // `packages/framework/react/src/annotation.tsx`) answers `{ data: file }`;
  // an attachment needs the file's name from it.
  test("a picker's File reaches Rust as bytes with its name and type", async () => {
    const wire = new Wire({ host: () => undefined, hold: () => 0 } as unknown as EngineBridge);
    const file = new File([new Uint8Array([7, 8])], 'notes.txt', { type: 'text/plain' });
    const picker = wire.encode(() => Promise.resolve({ data: file })) as { $fn: string };
    const answer = await wire.callJs(picker.$fn.slice('js:'.length), '[]');
    expect(answer).toEqual({
      data: { $bytes: btoa(String.fromCharCode(7, 8)) },
      name: 'notes.txt',
      mimeType: 'text/plain',
    });
  });
});
