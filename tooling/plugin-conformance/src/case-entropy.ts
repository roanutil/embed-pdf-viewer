/** Shared host input for current fixtures: one UUID sequence across factories and documents. */
let restore: (() => void) | null = null;

export function restoreCaseEntropy(): void {
  const release = restore;
  restore = null;
  release?.();
}

export function installCaseEntropy(): void {
  if (restore) return;
  // Resolve the possibly throwing getter before installing anything.
  const descriptor = Object.getOwnPropertyDescriptor(globalThis.crypto, 'getRandomValues');
  const original = globalThis.crypto.getRandomValues;
  let sequence = 0;
  const deterministic = <T extends ArrayBufferView | null>(array: T): T => {
    if (!(array instanceof Uint8Array) || array.byteLength !== 16)
      return Reflect.apply(original, globalThis.crypto, [array]) as T;
    const bytes = new Uint8Array(array.buffer, array.byteOffset, array.byteLength);
    bytes.fill(0);
    new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength).setUint32(
      bytes.length - 4,
      ++sequence,
    );
    return array;
  };
  restore = () => {
    if (descriptor) Object.defineProperty(globalThis.crypto, 'getRandomValues', descriptor);
    else Reflect.deleteProperty(globalThis.crypto, 'getRandomValues');
  };
  try {
    Object.defineProperty(globalThis.crypto, 'getRandomValues', {
      configurable: true,
      writable: true,
      value: deterministic,
    });
  } catch (error) {
    restoreCaseEntropy();
    throw error;
  }
}
