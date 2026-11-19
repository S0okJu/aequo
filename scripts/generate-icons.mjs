import { spawnSync } from 'node:child_process';
import { copyFileSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath, URL } from 'node:url';

const root = fileURLToPath(new URL('../', import.meta.url));
const source = join(root, 'img/logo.png');
const output = join(root, 'src-tauri/icons');

function generate(input, destination) {
  const result = spawnSync('pnpm', ['exec', 'tauri', 'icon', input, '--output', destination], {
    cwd: root,
    stdio: 'inherit',
  });
  if (result.error) throw result.error;
  if (result.status !== 0) throw new Error(`Icon generation failed (${result.status})`);
}

generate(source, output);

// Keep the artwork smaller inside the macOS canvas so it sits comfortably in the Dock.
// Generate this separately to preserve the other platforms' existing icon sizes.
const temporary = mkdtempSync(join(tmpdir(), 'aequo-macos-icon-'));
try {
  const artwork = readFileSync(source).toString('base64');
  const macSource = join(temporary, 'macos.svg');
  writeFileSync(
    macSource,
    `<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="1024" height="1024" viewBox="0 0 1024 1024"><image x="51.2" y="51.2" width="921.6" height="921.6" xlink:href="data:image/png;base64,${artwork}"/></svg>`,
  );
  const macOutput = join(temporary, 'icons');
  generate(macSource, macOutput);
  copyFileSync(join(macOutput, 'icon.icns'), join(output, 'icon.icns'));
} finally {
  rmSync(temporary, { recursive: true, force: true });
}
