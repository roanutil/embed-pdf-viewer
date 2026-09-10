import assert from 'node:assert/strict';
import { spawnSync } from 'node:child_process';
import { copyFileSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { test } from 'node:test';

for (const linkExit of [0, 2, 7]) {
  test(`test-all preserves required web link status ${linkExit}`, (t) => {
    const root = mkdtempSync(join(tmpdir(), 'epdf-test-all-'));
    t.after(() => rmSync(root, { recursive: true, force: true }));
    for (const dir of ['build', 'bin', 'jdk/bin', 'sdk', 'packages/kotlin', 'apps/android']) {
      mkdirSync(join(root, dir), { recursive: true });
    }
    copyFileSync(new URL('./test-all.sh', import.meta.url), join(root, 'build/test-all.sh'));
    const script = (path, body) => writeFileSync(join(root, path), `#!/bin/bash\n${body}\n`, { mode: 0o755 });
    for (const name of ['build-node', 'build-swift', 'test-swift', 'build-kotlin']) {
      script(`build/${name}.sh`, 'exit 0');
    }
    script('build/link-wasm.sh', `exit ${linkExit}`);
    script('bin/cargo', 'exit 0');
    script('bin/node', 'echo subset >> "$TEST_LOG"');
    script('bin/npm', 'echo full >> "$TEST_LOG"');
    script('jdk/bin/java', 'exit 0');
    script('packages/kotlin/gradlew', 'exit 0');
    script('apps/android/gradlew', 'exit 0');
    const log = join(root, 'calls');
    writeFileSync(log, '');
    const result = spawnSync('bash', [join(root, 'build/test-all.sh')], {
      encoding: 'utf8',
      env: {
        ...process.env, PATH: `${join(root, 'bin')}:${process.env.PATH}`,
        JAVA_HOME: join(root, 'jdk'), ANDROID_HOME: join(root, 'sdk'), TEST_LOG: log,
      },
    });
    assert.equal(result.status, linkExit === 2 ? 1 : linkExit, result.stdout + result.stderr);
    assert.equal(readFileSync(log, 'utf8'), linkExit === 0 ? 'full\n' : linkExit === 2 ? 'subset\n' : '');
    if (linkExit === 2) assert.match(result.stdout, /Required suites skipped[\s\S]*web/);
  });
}
