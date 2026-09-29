import { createReadStream, statSync } from 'node:fs';
import { createServer } from 'node:http';
import path from 'node:path';

// Every arm gets exactly these. Cross-origin isolation is what lets
// performance.measureUserAgentSpecificMemory() run.
export const HEADERS = {
  'Cross-Origin-Opener-Policy': 'same-origin',
  'Cross-Origin-Embedder-Policy': 'require-corp',
  'Cross-Origin-Resource-Policy': 'same-origin',
  'Cache-Control': 'no-store',
};

const TYPES = {
  '.html': 'text/html; charset=utf-8',
  '.js': 'text/javascript; charset=utf-8',
  '.mjs': 'text/javascript; charset=utf-8',
  '.css': 'text/css; charset=utf-8',
  '.wasm': 'application/wasm',
  '.pdf': 'application/pdf',
  '.ttf': 'font/ttf',
  '.json': 'application/json; charset=utf-8',
};

function serveDir(root) {
  return createServer((req, res) => {
    const url = new URL(req.url, 'http://localhost');
    const rel = decodeURIComponent(url.pathname === '/' ? '/index.html' : url.pathname);
    const file = path.join(root, rel);
    let stat = null;
    try {
      if (file.startsWith(root + path.sep)) stat = statSync(file);
    } catch {}
    // Unknown paths are a 404, never index.html, so a missing asset fails loudly.
    const type = TYPES[path.extname(file)];
    if (!stat?.isFile() || !type) {
      res.writeHead(404, HEADERS);
      res.end('not found');
      return;
    }
    res.writeHead(200, { ...HEADERS, 'Content-Type': type, 'Content-Length': stat.size });
    if (req.method === 'HEAD') return res.end();
    createReadStream(file).pipe(res);
  });
}

// One listener per arm, ports from `basePort` upward in the order given.
export async function serveArms(dists, { basePort = 5301 } = {}) {
  const servers = [];
  const urls = {};
  let port = basePort;
  for (const [arm, dir] of Object.entries(dists)) {
    const server = serveDir(path.resolve(dir));
    await new Promise((resolve, reject) => {
      server.once('error', reject);
      server.listen(port, '127.0.0.1', resolve);
    });
    servers.push(server);
    urls[arm] = `http://127.0.0.1:${port}`;
    port++;
  }
  return {
    urls,
    close: () => Promise.all(servers.map((s) => new Promise((r) => s.close(r)))),
  };
}

if (import.meta.url === `file://${process.argv[1]}`) {
  // node lib/serve.mjs <dir> [port]: a manual look at one arm.
  const { urls } = await serveArms({ arm: process.argv[2] }, { basePort: Number(process.argv[3] ?? 5301) });
  console.log(urls.arm);
}
