/**
 * A 32-bit BGRA BMP with a BITMAPINFOHEADER. PDFium's FPDFBitmap_BGRA byte
 * order is already what BMP wants, so the only work is the header and flipping
 * rows: BMP scanlines run bottom-up.
 */
export function encodeBmp({ width, height, stride, bgra }) {
  const headerSize = 14 + 40;
  const pixelBytes = width * height * 4;
  const out = Buffer.alloc(headerSize + pixelBytes);

  out.write('BM', 0, 'ascii');
  out.writeUInt32LE(out.length, 2);
  out.writeUInt32LE(headerSize, 10);

  out.writeUInt32LE(40, 14);
  out.writeInt32LE(width, 18);
  out.writeInt32LE(height, 22);
  out.writeUInt16LE(1, 26);
  out.writeUInt16LE(32, 28);
  out.writeUInt32LE(0, 30);
  out.writeUInt32LE(pixelBytes, 34);

  for (let y = 0; y < height; y += 1) {
    const source = (height - 1 - y) * stride;
    bgra.copy(out, headerSize + y * width * 4, source, source + width * 4);
  }

  return out;
}
