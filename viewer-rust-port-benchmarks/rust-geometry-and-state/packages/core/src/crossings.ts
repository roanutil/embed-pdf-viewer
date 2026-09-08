/** Same counter as rust-geometry-only, so the two demos are comparable. */
let count = 0;
const listeners = new Set<() => void>();

export const crossings = (): number => count;

export function countCrossing(n = 1): void {
  count += n;
  // This runs inside the timed region of every benchmark workload, and the
  // benchmark has no subscribers; skip the iterator setup for an empty set.
  if (listeners.size !== 0) notify();
}

function notify(): void {
  for (const l of listeners) l();
}

export function resetCrossings(): void {
  count = 0;
  notify();
}

export function subscribeCrossings(listener: () => void): () => void {
  listeners.add(listener);
  return () => void listeners.delete(listener);
}
