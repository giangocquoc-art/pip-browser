"""Fetch pinned upstream sources; regenerate the bundled YouTube rules/resources.

No build-time network access. Review and change these commits intentionally for updates.
"""
import concurrent.futures
import hashlib
import json
from pathlib import Path
import subprocess
import urllib.request

ROOT = Path(__file__).resolve().parents[1]
DEST = ROOT / 'crates/app/filters'
PINS = {
    'brave/uBlock': '1b21281a9178b31a017ead68182e43cea58961ec',
    'brave/adblock-resources': '6da9b247c3368baa8b4f24cd02046e18702a6527',
    'uBlockOrigin/uAssets': '0942ffe5d4cc7ba6d905d64044a82123f1e7c6fd',
}

def fetch(repo, path):
    url = f'https://raw.githubusercontent.com/{repo}/{PINS[repo]}/{path}'
    data = urllib.request.urlopen(url, timeout=45).read()
    target = DEST / 'upstream' / repo / path
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_bytes(data)
    return data

def listing(repo, path):
    req = urllib.request.Request(f'https://api.github.com/repos/{repo}/contents/{path}?ref={PINS[repo]}', headers={'User-Agent':'Pip-build'})
    return json.load(urllib.request.urlopen(req, timeout=45))

if __name__ == '__main__':
    modules = listing('brave/uBlock', 'src/js/resources')
    with concurrent.futures.ThreadPoolExecutor(max_workers=6) as pool:
        list(pool.map(lambda m: fetch('brave/uBlock', m['path']), modules))
    for name in ['urlskip', 'arglist-parser', 'jsonpath']:
        fetch('brave/uBlock', f'src/js/{name}.js')
    (DEST/'upstream/brave/uBlock/package.json').write_text('{"type":"module"}\n')
    fetch('brave/uBlock', 'LICENSE.txt')
    metadata = json.loads(fetch('brave/adblock-resources', 'metadata.json'))
    with concurrent.futures.ThreadPoolExecutor(max_workers=6) as pool:
        list(pool.map(lambda m: fetch('brave/adblock-resources', 'resources/'+m['resourcePath']), metadata))
    fetch('brave/adblock-resources', 'LICENSE')
    fetch('uBlockOrigin/uAssets', 'LICENSE')
    # Only domain-specific YouTube entries: avoid importing unrelated site behavior.
    rules = ['! Pip: YouTube rules selected from pinned upstream lists; see sources.json.']
    for name in ['filters', 'quick-fixes', 'unbreak']:
        source = fetch('uBlockOrigin/uAssets', f'filters/{name}.txt').decode()
        conditions = []
        for line in source.splitlines():
            if line.startswith('!#if '):
                condition = line[5:].strip()
                # All conditions used by these pinned lists are single tokens.
                assert ' ' not in condition, condition
                value = condition.lstrip('!') in {'env_chromium'}
                conditions.append(not value if condition.startswith('!') else value)
                continue
            if line == '!#else':
                conditions[-1] = not conditions[-1]
                continue
            if line == '!#endif':
                conditions.pop()
                continue
            if not all(conditions):
                continue
            separator = '#@#' if '#@#' in line else '##'
            if line.startswith('!') or separator not in line:
                continue
            domains = line.split(separator,1)[0].split(',')
            youtube = [d for d in domains if d in ('youtube.com','www.youtube.com','m.youtube.com','music.youtube.com','www.youtube-nocookie.com','youtube-nocookie.com')]
            if youtube:
                rules.append(','.join(youtube)+separator+line.split(separator,1)[1])
    (DEST/'youtube.txt').write_text('\n'.join(dict.fromkeys(rules))+'\n', encoding='utf-8')
    subprocess.run(['node', str(ROOT/'tools/build_scriptlet_resources.mjs')],check=True)
    hashes = {str(p.relative_to(DEST)).replace('\\','/'):hashlib.sha256(p.read_bytes()).hexdigest() for p in sorted((DEST/'upstream').rglob('*')) if p.is_file()}
    (DEST/'sources.json').write_text(json.dumps({'commits':PINS,'sha256':hashes}, indent=2)+'\n')
    print(f'Bundled {len(rules)-1} YouTube rules with pinned sources.')
