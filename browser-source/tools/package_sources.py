"""Package redistributable source/licenses only; never include profiles or executables."""
from pathlib import Path
import zipfile

root=Path(__file__).resolve().parents[1]
output=root/'crates/installer/assets/Pip-source.zip'
output.parent.mkdir(parents=True,exist_ok=True)
files=[root/name for name in ['Cargo.toml','Cargo.lock','LICENSE','README.md','PIP_README.md','THIRD_PARTY.md','RELEASE_NOTES.md','docs/PONYTAIL_AUDIT_2026-10-16.md']]
allowed={'.py','.rs','.toml','.lock','.html','.b64','.png','.webp','.ico','.rgba','.svg','.txt','.md','.mjs','.js','.json','.ps1','.cmd','.xml'}
for folder in ['crates','licenses','scripts','tools','store']:
    for path in (root/folder).rglob('*'):
        if not path.is_file():continue
        relative=path.relative_to(root)
        if any(part in {'assets','target','node_modules','user_data','data','out','__pycache__'} for part in relative.parts) and not relative.parts[:4]==('crates','app','ui','assets'):continue
        if folder=='licenses' or path.suffix in allowed or path.name.endswith('.xml.template') or path.name in {'LICENSE','LICENSE.txt'}:
            if not path.resolve().is_relative_to(root):raise ValueError(f'External source link: {relative}')
            files.append(path)
with zipfile.ZipFile(output,'w',zipfile.ZIP_DEFLATED,compresslevel=9,strict_timestamps=False) as archive:
    for path in sorted(set(files)):archive.write(path,path.relative_to(root).as_posix())
with zipfile.ZipFile(output) as archive:
    assert archive.testzip() is None
    assert not any(name.lower().endswith(('.exe','.dll')) or '/user_data/' in name for name in archive.namelist())
    assert 'store/package-store-msix.ps1' in archive.namelist()
    assert 'store/AppxManifest.xml.template' in archive.namelist()
    assert not any(name.startswith('store/out/') for name in archive.namelist())
print(f'Packaged {len(files)} source/license files: {output.stat().st_size:,} bytes')
