import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import { existsSync, readFileSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import test from 'node:test';

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const readJson = (relativePath) =>
  JSON.parse(readFileSync(path.join(repoRoot, relativePath), 'utf8'));

function resolvePackageFile(packageRoot, relativePath) {
  assert.ok(relativePath.startsWith('./'), `${relativePath} must start with ./`);
  const resolved = path.resolve(packageRoot, relativePath);
  assert.ok(
    resolved.startsWith(`${packageRoot}${path.sep}`),
    `${relativePath} must stay within the plugin package`,
  );
  assert.ok(existsSync(resolved), `${relativePath} must exist`);
  return resolved;
}

test('repo marketplace points to the Pip Dev Browser package', () => {
  const marketplace = readJson('.agents/plugins/marketplace.json');
  const entry = marketplace.plugins.find((plugin) => plugin.name === 'pip-dev-browser');

  assert.ok(entry, 'marketplace must expose pip-dev-browser');
  assert.equal(entry.source.source, 'local');
  assert.ok(entry.source.path.startsWith('./'));
  assert.ok(existsSync(path.resolve(repoRoot, entry.source.path, 'plugin.json')));
  assert.equal(entry.policy.installation, 'AVAILABLE');
});

test('portable and Codex compatibility manifests reference files in the package', () => {
  const packageRoot = path.join(repoRoot, 'pip-dev', 'plugins', 'pip-dev-browser');
  const portable = readJson('pip-dev/plugins/pip-dev-browser/plugin.json');
  const portableMcp = readJson('pip-dev/plugins/pip-dev-browser/mcp.json');
  const codex = readJson('pip-dev/plugins/pip-dev-browser/.codex-plugin/plugin.json');
  const codexMcp = readJson('pip-dev/plugins/pip-dev-browser/.mcp.json');

  assert.equal(portable.name, 'pip-dev-browser');
  assert.equal(portable.extensions['com.openai'].interface.displayName, 'Pip Dev Browser');
  resolvePackageFile(packageRoot, portable.extensions['com.openai'].onboardingSkill);
  resolvePackageFile(packageRoot, codex.skills);
  resolvePackageFile(packageRoot, codex.mcpServers);

  const portableServer = portableMcp.mcpServers.pip_dev;
  const codexServer = codexMcp.mcpServers.pip_dev;
  assert.equal(portableServer.type, 'streamable-http');
  assert.equal(codexServer.type, 'http');
  assert.equal(portableServer.url, 'http://127.0.0.1:9200/mcp');
  assert.equal(codexServer.url, portableServer.url);
});

test('GitHub install instructions include both sparse paths and the marketplace install command', () => {
  const docs = [
    readFileSync(path.join(repoRoot, 'README.md'), 'utf8'),
    readFileSync(path.join(repoRoot, 'pip-dev/docs/INSTALL_WINDOWS.md'), 'utf8'),
    readFileSync(path.join(repoRoot, 'pip-dev/docs/PIP_DEV_AGENT_SETUP.md'), 'utf8'),
  ].join('\n');

  assert.match(docs, /codex plugin marketplace add giangocquoc-art\/pip-browser --sparse \.agents\/plugins --sparse pip-dev\/plugins\/pip-dev-browser/);
  assert.match(docs, /codex plugin add pip-dev-browser --marketplace pip-dev-plugins/);
});

test('private BrowserOS pointers and local SDK/npm caches stay out of the public source candidate', () => {
  const excludedPaths = [
    'pip-dev/.gitmodules',
    'pip-dev/.internal-docs/README.md',
    'pip-dev/.github/workflows/sync-internal-docs.yml',
    'pip-dev/.claude/skills/ask-internal/SKILL.md',
    'pip-dev/.claude/skills/write-internal-docs/SKILL.md',
    'pip-dev/.agents/skills/ask-internal',
    'pip-dev/packages/browseros/.data/sdk-portable-packages/example.nupkg',
    'pip-dev/packages/browseros/.data/sdk-portable-tools/nuget.exe',
    'pip-dev/packages/browseros/.data/windows-sdk-26100-validation/Debuggers/x64/dbghelp.dll',
    'pip-dev/packages/browseros/.data/windows-sdk-nuget-stage/Lib/example.lib',
    'pip-dev/packages/browseros-agent/.data/verify-npm-cache/_cacache/example',
  ];
  const ignored = execFileSync('git', ['check-ignore', '--no-index', '--stdin'], {
    cwd: repoRoot,
    input: `${excludedPaths.join('\n')}\n`,
    encoding: 'utf8',
  })
    .trim()
    .split(/\r?\n/);

  assert.deepEqual(new Set(ignored), new Set(excludedPaths));
});
