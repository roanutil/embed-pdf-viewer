/**
 * Fixed-point quantization for the frozen vectors.
 *
 * The TypeScript oracle reads PDFium compiled to WASM; the Rust runner reads
 * the same fork compiled natively. Two builds of one C++ source agree on
 * almost every bit, and "almost" is not something a committed vector can be
 * written against. Every geometry number is therefore frozen at 1e-4 of a PDF
 * point (about 35 nanometres on a printed page, four orders of magnitude below
 * anything a reader can observe) and both sides compare the quantized value.
 *
 * Divergence below that threshold is deliberately invisible here. It belongs to
 * the pixel gate and the performance gates, not to the behavioural corpus.
 *
 * `crates/epdf-engine/tests/support/quantize.rs` is the Rust twin of this file.
 * Both round half away from zero, which is what `f64::round` does in Rust and
 * what `Math.round` does NOT do for negative ties, hence the sign split below.
 */

/** Units per PDF point. Changing it invalidates every committed vector. */
export const QUANTIZATION_SCALE = 10_000;

/** Rounds to the nearest 1/{@link QUANTIZATION_SCALE}, half away from zero. */
export function quantize(value: number): number {
  if (!Number.isFinite(value)) {
    throw new Error(`Cannot quantize a non-finite value: ${value}`);
  }
  const scaled = value * QUANTIZATION_SCALE;
  // Math.round breaks ties toward +Infinity, so -0.5 would land on -0 while
  // Rust's f64::round lands on -1. Round the magnitude and restore the sign.
  const units = scaled < 0 ? -Math.round(-scaled) : Math.round(scaled);
  // A quantized zero is written as 0, never -0: JSON.stringify emits "0" for
  // both, and a value that reads back differently from how it compares is the
  // kind of thing that costs an afternoon.
  return units === 0 ? 0 : units / QUANTIZATION_SCALE;
}

/**
 * Quantizes every number in a JSON-shaped value, leaving structure, strings and
 * booleans untouched.
 */
export function quantizeDeep<T>(value: T): T {
  if (typeof value === 'number') return quantize(value) as T;
  if (Array.isArray(value)) return value.map((item) => quantizeDeep(item)) as T;
  if (value && typeof value === 'object') {
    const out: Record<string, unknown> = {};
    for (const [key, child] of Object.entries(value)) out[key] = quantizeDeep(child);
    return out as T;
  }
  return value;
}
