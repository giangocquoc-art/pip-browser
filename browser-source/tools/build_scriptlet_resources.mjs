// Convert upstream's own registered functions to adblock-rust Resource objects.
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const filters = path.join(root, 'crates/app/filters');
const { builtinScriptlets } = await import('../crates/app/filters/upstream/brave/uBlock/src/js/resources/scriptlets.js');
const resources = builtinScriptlets.map(s => ({
  name:s.name, aliases:s.aliases || [], dependencies:s.dependencies || [],
  kind:{mime:s.name.endsWith('.fn') ? 'fn/javascript' : 'application/javascript'},
  content:Buffer.from(s.fn.toString()).toString('base64'), permission:s.requiresTrust ? 1 : 0
}));
const brave = path.join(filters, 'upstream/brave/adblock-resources');
for (const item of JSON.parse(fs.readFileSync(path.join(brave,'metadata.json')))) {
  resources.push({...item, content:fs.readFileSync(path.join(brave,'resources',item.resourcePath)).toString('base64')});
}
const byName = new Map(resources.flatMap(s => [s.name,...s.aliases].map(n=>[n,s])));
const used = new Set();
function add(name) {
  const s=byName.get(name);
  if (!s) throw new Error(`Missing upstream scriptlet: ${name}`);
  if (used.has(s)) return;
  used.add(s);
  for (const dep of s.dependencies || []) add(dep);
}
for (const match of fs.readFileSync(path.join(filters,'youtube.txt'),'utf8').matchAll(/##\+js\(([^,)]+)/g)) add(match[1].endsWith('.js')?match[1]:match[1]+'.js');
fs.writeFileSync(path.join(filters,'scriptlet-resources.json'),JSON.stringify([...used]));
console.log(`Converted ${used.size} upstream scriptlet resources used by YouTube`);
