import type {
  FullConfig,
  FullResult,
  Reporter,
  TestCase,
  TestResult,
} from '@playwright/test/reporter';
import { execFileSync } from 'node:child_process';
import { createHash } from 'node:crypto';
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { summarize, type Sample } from './performance';

const here = path.dirname(fileURLToPath(import.meta.url));
function git(...args: string[]) {
  try {
    return execFileSync('git', args, { cwd: here, encoding: 'utf8' }).trim();
  } catch {
    return 'unknown';
  }
}

export default class PerformanceReporter implements Reporter {
  private samples: Sample[] = [];
  private config!: FullConfig;
  private commit = git('rev-parse', 'HEAD');
  private worktree = git('status', '--short');
  onBegin(config: FullConfig) {
    this.config = config;
  }
  onTestEnd(test: TestCase, result: TestResult) {
    const attachment = result.attachments.find((a) => a.name === 'viewer-performance');
    if (attachment?.body) {
      this.samples.push({ ...JSON.parse(attachment.body.toString()), status: result.status });
    } else {
      // Setup failures/timeouts must not silently disappear from the report.
      this.samples.push({
        project: test.parent.project()!.name,
        flow: test.title,
        repeat: test.repeatEachIndex,
        retry: result.retry,
        status: result.status,
        browserVersion: 'unavailable',
        metrics: {},
        renders: [],
      });
    }
  }
  onEnd(result: FullResult) {
    const summary = summarize(this.samples);
    const report = {
      schemaVersion: 1,
      createdAt: new Date().toISOString(),
      status: result.status,
      environment: {
        commit: this.commit,
        worktree: this.worktree,
        platform: process.platform,
        arch: process.arch,
        osRelease: os.release(),
        cpu: os.cpus()[0]?.model,
        cpuCount: os.cpus().length,
        node: process.version,
        playwright: JSON.parse(
          readFileSync(path.join(here, 'node_modules/@playwright/test/package.json'), 'utf8'),
        ).version,
        delivery: process.env.VIEWER_E2E_PACKED_DIR ? 'packed-production' : 'workspace-dev',
        fixtureSha256: createHash('sha256')
          .update(readFileSync(path.join(here, 'fixtures/baseline.pdf')))
          .digest('hex'),
        workers: this.config.workers,
        projects: this.config.projects.map((p) => ({
          name: p.name,
          repeatEach: p.repeatEach,
          viewport: p.use.viewport,
          deviceScaleFactor: p.use.deviceScaleFactor,
        })),
      },
      samples: this.samples,
      summary,
    };
    const destination = process.env.VIEWER_E2E_PERF_OUTPUT
      ? path.resolve(process.env.VIEWER_E2E_PERF_OUTPUT)
      : path.join(here, 'performance-results', report.createdAt.replace(/[:.]/g, '-') + '.json');
    mkdirSync(path.dirname(destination), { recursive: true });
    writeFileSync(destination, JSON.stringify(report, null, 2) + '\n');
    const rows = summary.map(
      (r) =>
        `| ${r.project} | ${r.flow.split(' ')[0]} | ${r.metric} | ${r.n} | ${r.median.toFixed(2)} | ${r.p95.toFixed(2)} |`,
    );
    const markdown = [
      '# Local viewer performance',
      '',
      `Status: ${result.status}. Commit: ${this.commit}. Delivery: ${report.environment.delivery}.`,
      '',
      'Times in ms. Informational only; compare matching machines, browsers, and delivery modes.',
      'Flow times include Playwright input, assertions, and polling. Render times include queueing and transport.',
      'Fresh browser contexts; browser reuse and server/OS caches affect results. Failed/retried samples excluded from summaries.',
      '',
      '| Browser/backend | Flow | Metric | n | Median | p95 |',
      '|---|---|---|---:|---:|---:|',
      ...rows,
      '',
    ].join('\n');
    writeFileSync(destination + '.md', markdown);
    console.log(`\nPerformance report: ${destination}\n${markdown}`);
  }
}
