#!/usr/bin/env node
/**
 * The same EH module check, in real Safari.
 *
 * Playwright covers Chromium, Firefox and WebKit, and real Chrome through its
 * `chrome` channel. It cannot drive Safari: its `webkit` browser is a
 * Playwright build of WebKit, not the browser people have. Section 14 is
 * specific that this distinction matters ("A Nightly pass says nothing about
 * release Firefox, and Safari is untested"), so Safari gets its own runner
 * over `safaridriver`'s WebDriver endpoint rather than being folded into the
 * Playwright projects and quietly counted as the same evidence.
 *
 * Needs one manual step, once per machine, because it grants remote control of
 * a browser and is not something a script should do to somebody:
 *
 *   Safari > Settings > Advanced > Show features for web developers
 *   Safari > Settings > Developer > Allow remote automation
 *
 * Then, with the module server already running (`node serve.mjs`):
 *
 *   node safari.mjs
 */
import { spawn } from 'node:child_process';
import { setTimeout as sleep } from 'node:timers/promises';

const PORT = Number(process.env.SAFARIDRIVER_PORT ?? 4599);
const PAGE = `http://127.0.0.1:${process.env.EH_PORT ?? 5311}/`;

async function call(method, path, body) {
  const response = await fetch(`http://127.0.0.1:${PORT}${path}`, {
    method,
    headers: { 'Content-Type': 'application/json' },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const json = await response.json();
  if (json.value?.error) {
    throw new Error(`${json.value.error}: ${json.value.message}`);
  }
  return json.value;
}

const driver = spawn('safaridriver', ['-p', String(PORT)], { stdio: 'inherit' });
const stop = () => driver.kill();
process.on('exit', stop);
process.on('SIGINT', () => {
  stop();
  process.exit(130);
});

try {
  await sleep(1500);

  let session;
  try {
    session = await call('POST', '/session', {
      capabilities: { alwaysMatch: { browserName: 'safari' } },
    });
  } catch (error) {
    if (String(error).includes('Allow remote automation')) {
      console.error(
        'Safari is not accepting WebDriver sessions yet.\n\n' +
          '  Safari > Settings > Advanced > Show features for web developers\n' +
          '  Safari > Settings > Developer > Allow remote automation\n\n' +
          'Then run this again. Nothing else on this machine needs changing.',
      );
      process.exit(2);
    }
    throw error;
  }

  const id = session.sessionId;
  const version = session.capabilities?.browserVersion ?? 'unknown';
  try {
    await call('POST', `/session/${id}/url`, { url: PAGE });

    // The page sets window.__ehResult when it finishes, pass or fail.
    let report = null;
    for (let i = 0; i < 120 && report === null; i++) {
      report = await call('POST', `/session/${id}/execute/sync`, {
        script: 'return window.__ehResult ?? null;',
        args: [],
      });
      if (report === null) await sleep(500);
    }
    if (report === null) {
      console.error(`Safari ${version}: the page never finished. The module did not instantiate.`);
      process.exit(1);
    }

    const steps = Object.fromEntries(report.steps.map((s) => [s.name, s.value]));
    const failures = [];
    if (report.error) failures.push(`page threw: ${report.error}`);
    if (steps.instantiated !== true) failures.push('the module did not instantiate');
    if (steps.pageCount !== 14) failures.push(`pageCount ${steps.pageCount}, expected 14`);
    if (!(steps.charCount > 1000)) failures.push(`charCount ${steps.charCount}`);
    if (!(steps.renderedNonWhitePixels > 0)) failures.push('the render produced nothing');
    if (steps.wrongPasswordRefused !== true) failures.push('a wrong password was not refused');
    if (steps.lastError !== 4) failures.push(`FPDF_GetLastError ${steps.lastError}, expected 4`);
    if (steps.rustCoreLinked === true && steps.rustGeometryRuns !== 238) {
      failures.push(`rust geometry runs ${steps.rustGeometryRuns}, expected 238`);
    }

    if (failures.length > 0) {
      console.error(`Safari ${version} FAILED:\n  ${failures.join('\n  ')}`);
      process.exit(1);
    }
    console.log(
      `  ok      Safari ${version}: instantiated, ${steps.pageCount} pages, ` +
        `${steps.charCount} chars, ${steps.renderedNonWhitePixels} non-white pixels, ` +
        `wrong password -> ${steps.lastError}` +
        (steps.rustCoreLinked ? `, rust core ${steps.rustGeometryRuns} runs` : ', no rust core'),
    );
  } finally {
    await call('DELETE', `/session/${id}`).catch(() => {});
  }
} finally {
  stop();
}
