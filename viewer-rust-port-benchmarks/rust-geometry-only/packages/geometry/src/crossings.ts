/**
 * Every call into wasm goes through here, so the count is the real number and
 * not an estimate. In typescript-baseline this file does not exist, because there is nothing
 * to count.
 */
let count = 0;
const listeners = new Set<() => void>();

export const crossings = (): number => count;

export function countCrossing(n = 1): void {
  count += n;
  listeners.forEach((l) => l());
}

export function resetCrossings(): void {
  count = 0;
  listeners.forEach((l) => l());
}

export function subscribeCrossings(listener: () => void): () => void {
  listeners.add(listener);
  return () => void listeners.delete(listener);
}
