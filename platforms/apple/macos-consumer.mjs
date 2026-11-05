// Private validation manifests enable Mac consumers before distributed pins move.
import { cpSync, existsSync, readFileSync, writeFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

export function macosConsumerManifest(text) {
  const count = (pattern) => (text.match(pattern) ?? []).length;
  if (count(/\.iOS\(\.v17\),/g) !== 1 || count(/\.linkedFramework\("CoreGraphics"\),/g) !== 2) {
    throw new Error('unsupported private consumer manifest platform/linker shape');
  }
  if (text.includes('.macOS(') && !text.includes('.macOS(.v14)')) {
    throw new Error('private consumer manifest must target macOS14');
  }
  const foundation = count(/\.linkedFramework\("Foundation"\)/g);
  const appkit = count(/\.linkedFramework\("AppKit", \.when\(platforms: \[\.macOS\]\)\)/g);
  if (![0, 2].includes(foundation) || ![0, 2].includes(appkit)) {
    throw new Error('unsupported partial private consumer manifest linker shape');
  }
  if (!text.includes('.macOS(.v14)'))
    text = text.replace('.iOS(.v17),', '.iOS(.v17), .macOS(.v14),');
  return text.replace(
    /([ \t]*)\.linkedFramework\("CoreGraphics"\),/g,
    (line, indent) =>
      line +
      (!foundation ? `\n${indent}.linkedFramework("Foundation"),` : '') +
      (!appkit ? `\n${indent}.linkedFramework("AppKit", .when(platforms: [.macOS])),` : ''),
  );
}

export function stageMacConsumer(source, output) {
  if (existsSync(output)) throw new Error(`refusing existing private consumer: ${output}`);
  const manifest = macosConsumerManifest(readFileSync(path.join(source, 'Package.swift'), 'utf8'));
  cpSync(source, output, {
    recursive: true,
    verbatimSymlinks: true,
    filter: (entry) => !['.build', '.swiftpm'].includes(path.basename(entry)),
  });
  writeFileSync(path.join(output, 'Package.swift'), manifest);
}

if (process.argv[1] && fileURLToPath(import.meta.url) === path.resolve(process.argv[1])) {
  if (process.argv.length !== 4)
    throw new Error('usage: macos-consumer.mjs <source-package> <new-private-package>');
  stageMacConsumer(process.argv[2], process.argv[3]);
}
