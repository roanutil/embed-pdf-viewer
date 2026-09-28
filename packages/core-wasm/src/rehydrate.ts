/**
 * Where a capability's JSON is not what the UI expects, by member name.
 *
 * `REHYDRATE` fixes a decoded result. A `{"$host": n}` needs no entry: the
 * decoder swaps every one for the engine bridge's live value, which is what
 * render's `getPlan` handles need. `PREPARE` fixes arguments before they are
 * encoded. Task 9 adds entries as the UI reaches more members. Arguments
 * that still hold a `Blob` after `PREPARE` are read by `readBinarySources`
 * (`Wire.call` in `proxy.ts`).
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
 * methods run in JavaScript: a live kernel prints the transform's data
 * without them (`visible_page_json` in `crates/plugins/stage/src/harness.rs`).
 * The inputs come back out of the page: its display box over
 * `contentScale` is the page size in points (rounded to a millionth, which
 * undoes the division's last-bit error for any size a PDF writes), and
 * `deviceWidth / contentWidth` is the device pixel ratio. A rebuilt
 * transform that differs from the kernel's in any member is a real drift
 * between this rebuild and the kernel's own math, with no ref-backed
 * transform to fall back on, so it throws.
 */
const points = (n: number) => Math.round(n * 1e6) / 1e6;

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
  const differs = TRANSFORM_DATA.filter((key) => rebuilt[key] !== t[key]);
  if (differs.length > 0) {
    const error = new Error(
      `[core-wasm] a rebuilt pageTransform differs from the kernel's in ${differs.join(', ')}`,
    );
    console.error(error, { kernel: t, rebuilt });
    throw error;
  }
  return { ...page, transform: rebuilt };
}

const REHYDRATE: Partial<Record<string, (value: unknown) => unknown>> = {
  listVisiblePages: (pages) => (pages as Obj[]).map(withPageTransform),
  getPageFrame: (page) => withPageTransform(page as Obj),
};

export function rehydrate(member: string, value: unknown): unknown {
  const fix = REHYDRATE[member];
  return fix && value !== undefined && value !== null ? fix(value) : value;
}

const isBlob = (value: unknown): value is Blob =>
  typeof Blob !== 'undefined' && value instanceof Blob;

const isPlainObject = (value: unknown): value is Obj =>
  value !== null && typeof value === 'object' && Object.getPrototypeOf(value) === Object.prototype;

/** Whether a `Blob` sits anywhere in `value`'s arrays and plain objects. */
export const hasBlob = (value: unknown): boolean => {
  if (isBlob(value)) return true;
  if (Array.isArray(value)) return value.some(hasBlob);
  if (isPlainObject(value)) return Object.values(value).some(hasBlob);
  return false;
};

/** A `Blob`'s `type` as `mimeType` and a `File`'s `name`, where they are set. */
const blobFacts = (blob: Blob): Obj => {
  const name = (blob as File).name;
  return {
    ...(blob.type ? { mimeType: blob.type } : {}),
    ...(typeof name === 'string' && name.length > 0 ? { name } : {}),
  };
};

const bytesOf = async (blob: Blob) => new Uint8Array(await blob.arrayBuffer());

/**
 * `value` with every `Blob` in its arrays and plain objects read to bytes,
 * keeping what `resolveBinarySource` (`@embedpdf/engine-core`,
 * `resource/BinarySource.ts`) would take from it. A `Blob` in a member
 * named `data` (a `BinaryPayload`'s, or an `AttachmentFileSource`'s) becomes
 * its bytes, and its type and file name fill the object's `mimeType` and
 * `name` where those are absent. Any other `Blob` is a whole `BinarySource`
 * and becomes the payload `{data, mimeType?, name?}`. Every capability
 * member that takes a `Blob` takes it in one of those two places.
 */
export async function readBinarySources(value: unknown): Promise<unknown> {
  if (isBlob(value)) return { data: await bytesOf(value), ...blobFacts(value) };
  if (Array.isArray(value)) return Promise.all(value.map(readBinarySources));
  if (!isPlainObject(value)) return value;
  const out: Obj = {};
  for (const [key, member] of Object.entries(value)) {
    out[key] =
      key === 'data' && isBlob(member) ? await bytesOf(member) : await readBinarySources(member);
  }
  if (isBlob(value.data)) {
    for (const [key, fact] of Object.entries(blobFacts(value.data))) {
      if (out[key] === undefined) out[key] = fact;
    }
  }
  return out;
}

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
