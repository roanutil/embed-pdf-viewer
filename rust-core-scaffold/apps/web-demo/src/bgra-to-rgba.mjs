/**
 * PDFium gives BGRA; ImageData wants RGBA. Returns a fresh copy with R and B
 * exchanged, one pass; the input array is never mutated.
 */
export function bgraToRgba(bgra) {
  const rgba = new Uint8ClampedArray(bgra);
  for (let i = 0; i < rgba.length; i += 4) {
    const b = rgba[i];
    rgba[i] = rgba[i + 2];
    rgba[i + 2] = b;
  }
  return rgba;
}
