import fs from 'node:fs';
import crypto from 'node:crypto';

const [mapPath, sourcePath] = process.argv.slice(2);
const map = JSON.parse(fs.readFileSync(mapPath));
const indices = map.sources.flatMap((name, index) =>
  name.endsWith('/pdf-functions.generated.ts') ? [index] : [],
);
if (indices.length !== 1 || typeof map.sourcesContent?.[indices[0]] !== 'string')
  throw new Error('Packed runtime source map must contain exactly one binding source');
const sha = (bytes) => crypto.createHash('sha256').update(bytes).digest('hex');
const artifactSourceSha256 = sha(map.sourcesContent[indices[0]]);
const expectedSourceSha256 = sha(fs.readFileSync(sourcePath));
if (artifactSourceSha256 !== expectedSourceSha256)
  throw new Error(
    'Packed runtime binding source differs: artifact=' +
      artifactSourceSha256 +
      ' current=' +
      expectedSourceSha256,
  );
console.log(
  JSON.stringify({
    status: 'passed',
    map: mapPath,
    source: sourcePath,
    bindingSourceSha256: expectedSourceSha256,
  }),
);
