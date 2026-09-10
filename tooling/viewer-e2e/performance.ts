/** Local measurements only. No engine results or user actions are replaced. */
export interface ViewerPerformance {
  firstPageFrameMs: number | null;
  renders: { kind: string; durationMs: number; outcome: string }[];
}

declare global {
  interface Window {
    viewerPerformance?: ViewerPerformance;
  }
}

// Serialized by Playwright: keep all browser dependencies inside this function.
export function installPerformanceProbe() {
  const probe: ViewerPerformance = { firstPageFrameMs: null, renders: [] };
  window.viewerPerformance = probe;
  const observe = () => {
    const root = document.querySelector('embedpdf-viewer')?.shadowRoot;
    const image = root?.querySelector<HTMLImageElement>('img[draggable="false"]');
    const rect = image?.getBoundingClientRect();
    if (
      image?.complete &&
      image.naturalWidth > 0 &&
      rect &&
      rect.width > 0 &&
      rect.height > 0 &&
      rect.bottom > 0 &&
      rect.top < innerHeight &&
      rect.right > 0 &&
      rect.left < innerWidth
    ) {
      // A decoded page had a frame opportunity. This is not a compositor or
      // pixel-diff timestamp, and does not wait for every sharp tile to arrive.
      requestAnimationFrame(() => {
        probe.firstPageFrameMs = performance.now();
      });
    } else {
      requestAnimationFrame(observe);
    }
  };
  requestAnimationFrame(observe);
}

export interface Sample {
  project: string;
  flow: string;
  repeat: number;
  retry: number;
  status: string;
  browserVersion: string;
  metrics: Record<string, number>;
  renders: ViewerPerformance['renders'];
}

export function distribution(values: number[]) {
  if (!values.length || values.some((n) => !Number.isFinite(n) || n < 0)) {
    throw new Error('A performance distribution requires finite, nonnegative samples');
  }
  const sorted = [...values].sort((a, b) => a - b);
  const mid = Math.floor(sorted.length / 2);
  return {
    n: sorted.length,
    min: sorted[0],
    median: sorted.length % 2 ? sorted[mid] : (sorted[mid - 1] + sorted[mid]) / 2,
    // Nearest-rank p95: with small samples this will be the maximum.
    p95: sorted[Math.ceil(sorted.length * 0.95) - 1],
    max: sorted.at(-1)!,
  };
}

export function summarize(samples: Sample[]) {
  const groups = new Map<
    string,
    { project: string; flow: string; metric: string; values: number[] }
  >();
  for (const sample of samples) {
    // Failed and retried runs remain in raw data, never in the baseline.
    if (sample.status !== 'passed' || sample.retry !== 0) continue;
    for (const [metric, value] of Object.entries(sample.metrics)) {
      const key = JSON.stringify([sample.project, sample.flow, metric]);
      const group = groups.get(key) ?? {
        project: sample.project,
        flow: sample.flow,
        metric,
        values: [],
      };
      group.values.push(value);
      groups.set(key, group);
    }
  }
  return [...groups.values()].map(({ values, ...group }) => ({
    ...group,
    ...distribution(values),
  }));
}
