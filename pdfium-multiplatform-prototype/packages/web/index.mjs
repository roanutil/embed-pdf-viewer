/**
 * JS glue over the em++ module built by build/link-wasm.sh. Same six methods as
 * packages/node/index.mjs, so packages/web/vectors.test.mjs and
 * packages/node/vectors.test.mjs assert the same frozen numbers through the
 * same shapes.
 */
import createEpdfModule from '../../build/out/web/epdf.mjs';

const OP_PAGE_SIZE = 0;
const OP_PAGE_TEXT = 1;
const OP_SEARCH = 2;

export async function createEpdf() {
  const module = await createEpdfModule();

  const epdf_open = module.cwrap('epdf_open', 'number', ['number', 'number']);
  const epdf_page_count = module.cwrap('epdf_page_count', 'number', ['number']);
  const epdf_json = module.cwrap('epdf_json', 'number',
    ['number', 'number', 'number', 'number', 'number', 'number', 'number']);
  const epdf_render = module.cwrap('epdf_render', 'number',
    ['number', 'number', 'number', 'number', 'number', 'number', 'number']);
  const epdf_free = module.cwrap('epdf_free', null, ['number', 'number']);
  const epdf_close = module.cwrap('epdf_close', null, ['number']);
  const epdf_last_error = module.cwrap('epdf_last_error', 'number', []);

  /** Read the error string immediately: the next failing call overwrites it. */
  const fail = () => {
    throw new Error(module.UTF8ToString(epdf_last_error()));
  };

  /** Copy bytes into the module heap; the caller frees. */
  const push = (bytes) => {
    const ptr = module._malloc(bytes.length);
    module.HEAPU8.set(bytes, ptr);
    return ptr;
  };

  const readJson = (doc, op, index, argBytes, flag) => {
    const lenPtr = module._malloc(4);
    const argPtr = argBytes ? push(argBytes) : 0;
    let dataPtr = 0;
    try {
      dataPtr = epdf_json(doc, op, index, argPtr, argBytes ? argBytes.length : 0, flag, lenPtr);
      if (dataPtr === 0) fail();
      const len = module.HEAPU32[lenPtr >> 2];
      const text = new TextDecoder().decode(module.HEAPU8.subarray(dataPtr, dataPtr + len));
      return JSON.parse(text);
    } finally {
      if (dataPtr !== 0) epdf_free(dataPtr, module.HEAPU32[lenPtr >> 2]);
      if (argPtr !== 0) module._free(argPtr);
      module._free(lenPtr);
    }
  };

  class Document {
    #handle;

    constructor(handle) {
      this.#handle = handle;
    }

    pageCount() {
      const count = epdf_page_count(this.#handle);
      if (count < 0) fail();
      return count;
    }

    pageSize(index) {
      return readJson(this.#handle, OP_PAGE_SIZE, index, null, 0);
    }

    pageText(index) {
      return readJson(this.#handle, OP_PAGE_TEXT, index, null, 0);
    }

    search(index, query, caseSensitive) {
      return readJson(
        this.#handle,
        OP_SEARCH,
        index,
        new TextEncoder().encode(query),
        caseSensitive ? 1 : 0,
      );
    }

    renderPage(index, scale) {
      const meta = module._malloc(16); // w, h, stride, len
      let dataPtr = 0;
      try {
        dataPtr = epdf_render(this.#handle, index, scale, meta, meta + 4, meta + 8, meta + 12);
        if (dataPtr === 0) fail();
        const [width, height, stride, len] = [0, 1, 2, 3].map((i) => module.HEAPU32[(meta >> 2) + i]);
        // Copy out: the view would dangle after epdf_free, and would also be
        // invalidated by any heap growth.
        const bgra = new Uint8Array(module.HEAPU8.subarray(dataPtr, dataPtr + len));
        return { width, height, stride, bgra };
      } finally {
        if (dataPtr !== 0) epdf_free(dataPtr, module.HEAPU32[(meta >> 2) + 3]);
        module._free(meta);
      }
    }

    close() {
      if (this.#handle !== 0) {
        epdf_close(this.#handle);
        this.#handle = 0;
      }
    }
  }

  return {
    openDocument(bytes) {
      const ptr = push(bytes);
      try {
        const handle = epdf_open(ptr, bytes.length);
        if (handle === 0) fail();
        return new Document(handle);
      } finally {
        module._free(ptr);
      }
    },
  };
}
