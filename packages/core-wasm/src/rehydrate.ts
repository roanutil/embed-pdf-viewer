/**
 * Where a capability's JSON is not what the UI expects, by member name.
 *
 * `REHYDRATE` fixes a decoded result. A `{"$host": n}` needs no entry: the
 * decoder swaps every one for the engine bridge's live value, which is what
 * render's `getPlan` handles need. `PREPARE` fixes arguments before they are
 * encoded. Task 9 adds entries as the UI reaches more members.
 */
import { pageTransform, type PageRotation, type PageTransform } from '@embedpdf/core-geometry';
import { readBlobs } from './engine-bridge';

type Obj = Record<string, unknown>;

/** The transform members `pageTransform` computes, compared before swapping. */
const TRANSFORM_DATA = [
  'rotation',
  'viewWidth',
  'viewHeight',
  'contentWidth',
  'contentHeight',
  'deviceWidth',
  'deviceHeight',
  'renderScale',
  'viewScale',
  'baseScale',
  'zoom',
  'cssMatrix',
] as const;

/**
 * A laid-out page's transform, rebuilt with `pageTransform` so its six
 * methods run in JavaScript instead of calling into the kernel. The inputs
 * come back out of the page: its display box over `contentScale` is the
 * page size in points (rounded to a millionth, which undoes the division's
 * last-bit error for any size a PDF writes), and `deviceWidth / contentWidth`
 * is the device pixel ratio. If the rebuilt transform differs from the
 * kernel's in any member, the kernel's (whose methods are refs) stays; that
 * fallback warns once, since a silent one would hide a real drift between
 * this rebuild and the kernel's own math.
 */
const points = (n: number) => Math.round(n * 1e6) / 1e6;

let warnedFallback = false;

function withPageTransform(page: Obj): Obj {
  const t = page.transform as Obj & PageTransform;
  const rotation = t.rotation as PageRotation;
  const quarter = rotation === 90 || rotation === 270;
  const scale = page.contentScale as number;
  const width = (quarter ? page.height : page.width) as number;
  const height = (quarter ? page.width : page.height) as number;
  const rebuilt = pageTransform({
    pageSize: { width: points(width / scale), height: points(height / scale) },
    rotation,
    scale: t.viewScale,
    baseScale: t.baseScale,
    dpr: t.deviceWidth / t.contentWidth,
  });
  const same = TRANSFORM_DATA.every((key) => rebuilt[key] === t[key]);
  if (!same && !warnedFallback) {
    warnedFallback = true;
    console.warn(
      "[core-wasm] listVisiblePages: a rebuilt pageTransform differs from the kernel's; falling back to the ref-backed transform",
    );
  }
  return same ? { ...page, transform: rebuilt } : page;
}

const REHYDRATE: Partial<Record<string, (value: unknown) => unknown>> = {
  listVisiblePages: (pages) => (pages as Obj[]).map(withPageTransform),
};

export function rehydrate(member: string, value: unknown): unknown {
  const fix = REHYDRATE[member];
  return fix && value !== undefined && value !== null ? fix(value) : value;
}

const hasBlob = (value: unknown): boolean => {
  if (typeof Blob !== 'undefined' && value instanceof Blob) return true;
  if (Array.isArray(value)) return value.some(hasBlob);
  if (
    value !== null &&
    typeof value === 'object' &&
    Object.getPrototypeOf(value) === Object.prototype
  ) {
    return Object.values(value).some(hasBlob);
  }
  return false;
};

/** An open source holding a `Blob` (a picked `File`) as a thunk that reads
 *  it: the kernel takes a source thunk's promise, and a `Blob` cannot cross
 *  synchronously. */
const readable = (source: unknown): unknown => (hasBlob(source) ? () => readBlobs(source) : source);

/** A `KeyStroke`'s members, read off whatever carries them: the chrome's
 *  `useCommandShortcuts` passes the `KeyboardEvent` itself, whose members
 *  are prototype getters the encoder's own-property walk never sees. */
const keyStroke = (event: unknown): unknown => {
  if (event === null || typeof event !== 'object') return event;
  const e = event as Obj;
  return {
    key: e.key,
    ...(typeof e.code === 'string' ? { code: e.code } : {}),
    ctrlKey: e.ctrlKey,
    metaKey: e.metaKey,
    altKey: e.altKey,
    shiftKey: e.shiftKey,
  };
};

const PREPARE: Partial<Record<string, (args: unknown[]) => unknown[]>> = {
  matchStroke: ([stroke, ...rest]) => [keyStroke(stroke), ...rest],
  open: ([source, ...rest]) => [readable(source), ...rest],
  openAll: ([docs, ...rest]) => [
    Array.isArray(docs)
      ? docs.map((doc: Obj) =>
          doc && typeof doc === 'object' ? { ...doc, source: readable(doc.source) } : doc,
        )
      : docs,
    ...rest,
  ],
};

export function prepareArgs(member: string, args: unknown[]): unknown[] {
  const fix = PREPARE[member];
  return fix ? fix(args) : args;
}
