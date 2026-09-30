export interface PerfArm {
  id: string;
  kernel: 'typescript' | 'rust';
  rustReads: boolean;
}

export interface PerfBridge {
  arm: PerfArm;
  ready: Promise<void>;
  opened: Promise<void>;
  pageCount(): number;
  goToPageIndex(i: number): void;
  currentPageIndex(): number;
  zoomTo(z: number): void;
  zoomLevel(): number;
  search(text: string): Promise<number>;
  clearSearch(): void;
  readSelection(): Promise<string>;
  clearSelection(): void;
  annotationCount(pageIndex: number): number;
  execute(command: string): void;
  kernelCounts(): Record<string, number> | null;
  pageBox(pageIndex: number): { x: number; y: number; width: number; height: number };
}

export interface Probe {
  readonly firstPaintAt: number | null;
  settledAt(sinceMs: number, options?: { doneAt?: number; requireChange?: boolean }): Promise<number>;
  frames(): number[];
  longTasks(): { start: number; duration: number }[];
  reset(): void;
}

declare global {
  interface Window {
    __perf: PerfBridge;
    __probe: Probe;
  }
}
