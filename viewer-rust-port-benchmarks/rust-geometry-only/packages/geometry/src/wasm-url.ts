/**
 * The bundler-resolved URL of the wasm binary.
 *
 * Kept in its own entry point because `?url` is a Vite thing. A Node consumer
 * imports `initGeometryFromDisk` instead and never touches this file, which is
 * the shape of every "one core, many hosts" boundary: the core is portable and
 * the loading is not.
 */
import url from '../wasm/poc_geometry_bg.wasm?url';

export default url;
