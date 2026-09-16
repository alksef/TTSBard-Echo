const fs = require('node:fs');
const path = require('node:path');

const version = process.argv[2];
if (!/^\d+\.\d+\.\d+$/.test(version ?? '')) {
  console.error('Usage: node scripts/set-version.cjs <major.minor.patch>');
  process.exit(1);
}

const root = path.join(__dirname, '..');

function replaceOnce(file, pattern, replacement) {
  const filePath = path.join(root, file);
  const input = fs.readFileSync(filePath, 'utf8');
  const output = input.replace(pattern, replacement);
  if (!pattern.test(input)) throw new Error(`Version field not found in ${file}`);
  fs.writeFileSync(filePath, output, 'utf8');
}

replaceOnce('package.json', /("version"\s*:\s*")[^"]+(")/, `$1${version}$2`);
replaceOnce('package-lock.json', /("version"\s*:\s*")[^"]+(")/, `$1${version}$2`);
replaceOnce('src-tauri/tauri.conf.json', /("version"\s*:\s*")[^"]+(")/, `$1${version}$2`);
replaceOnce('src-tauri/Cargo.toml', /^(version\s*=\s*")[^"]+(")/m, `$1${version}$2`);

// Cargo.lock keeps the resolved version of the local package; without this
// update every --locked cargo command fails until the lockfile is regenerated.
const cargoToml = fs.readFileSync(path.join(root, 'src-tauri/Cargo.toml'), 'utf8');
const packageName = cargoToml.match(/^name\s*=\s*"([^"]+)"/m)?.[1];
if (!packageName) throw new Error('Package name not found in src-tauri/Cargo.toml');
replaceOnce(
  path.join('src-tauri', 'Cargo.lock'),
  new RegExp(`(name = "${packageName}"\\r?\\nversion = ")[^"]+(")`),
  `$1${version}$2`,
);

const versionPath = path.join(root, 'src/version.ts');
fs.writeFileSync(versionPath, `export const APP_VERSION = '${version}'\n`, 'utf8');

console.log(`Version set to ${version}`);
