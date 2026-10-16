"""Test the distributable setup silently, under disposable per-user directories.

No real profile, desktop shortcut or HKCU app registration is changed.
"""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import time
import zipfile

root=Path(__file__).resolve().parents[1]
setup=Path(sys.argv[1] if len(sys.argv)>1 else root/'dist/Pip-Setup-0.1.1-Windows-x64.exe').resolve()
stage=Path(tempfile.mkdtemp(prefix='Pip setup check '))
env=os.environ.copy()
for variable,folder in [('LOCALAPPDATA','Local AppData'),('APPDATA','Roaming AppData'),('USERPROFILE','User Profile')]:
    env[variable]=str(stage/folder);(stage/folder).mkdir()
install=Path(env['LOCALAPPDATA'])/'Programs/Pip'
data=Path(env['APPDATA'])/'evergreen-browser'
desktop=Path(env['USERPROFILE'])/'Desktop/Pip.lnk'
start_menu=Path(env['APPDATA'])/'Microsoft/Windows/Start Menu/Programs/Pip.lnk'
def run(exe,*arguments,success=True):
    result=subprocess.run([str(exe),*arguments],cwd=stage,env=env,capture_output=True,timeout=90,creationflags=subprocess.CREATE_NO_WINDOW)
    assert (result.returncode==0)==success,(result.returncode,result.stderr.decode(errors='replace'))
    return result
def wait_removed(path):
    until=time.monotonic()+15
    while path.exists() and time.monotonic()<until:time.sleep(.2)
    assert not path.exists(),f'Cleanup did not remove {path}'
def digest(path):return hashlib.sha256(path.read_bytes()).hexdigest()

run(setup,'--silent','--no-register')
binary=install/'Pip.exe';uninstaller=install/'uninstall.exe'
assert digest(binary)==digest(root/'target/release/evergreen-browser.exe')
assert digest(uninstaller)==digest(setup)
assert desktop.is_file() and start_menu.is_file()
assert 'uBlock' in (install/'THIRD_PARTY.md').read_text(encoding='utf-8')
with zipfile.ZipFile(install/'Pip-source.zip') as archive:
    assert archive.testzip() is None
    assert 'crates/app/filters/upstream/brave/uBlock/LICENSE.txt' in archive.namelist()
    assert 'crates/app/filters/youtube.txt' in archive.namelist()
    assert not any(n.lower().endswith(('.exe','.dll')) or '/user_data/' in n for n in archive.namelist())

# A suspended process locks its own image without running app code or showing UI.
holder=subprocess.Popen([str(binary)],env=env,cwd=stage,creationflags=subprocess.CREATE_NO_WINDOW|0x00000004)
try:
    original=digest(binary)
    run(setup,'--silent','--no-register',success=False)
    run(uninstaller,'--uninstall','--silent','--no-register',success=False)
    assert holder.poll() is None and digest(binary)==original
finally:
    holder.terminate();holder.wait(timeout=10) # only the process created above

data.mkdir(parents=True);(data/'keep.txt').write_text('preserve my data')
outside=stage/'Roaming AppData/unrelated.txt';outside.write_text('keep outside Pip')
run(uninstaller,'--uninstall','--silent','--no-register')
wait_removed(install)
assert (data/'keep.txt').is_file() and not desktop.exists() and not start_menu.exists()
run(setup,'--silent','--no-register')
run(install/'uninstall.exe','--uninstall','--silent','--no-register','--remove-data')
wait_removed(install)
assert not data.exists() and outside.read_text()=='keep outside Pip'
evidence={'setup_sha256':digest(setup),'bytes':setup.stat().st_size,'payload_sha256':digest(root/'target/release/evergreen-browser.exe'),'checks':['silent per-user installation','payload integrity','source/license archive','desktop/start shortcuts','running app is neither killed nor overwritten','uninstall keeps data by default','explicit removal stays within Pip data directory','deferred uninstall cleanup'],'stage':str(stage),'registry':'not modified (--no-register)'}
(root/'target/installer-verification.json').write_text(json.dumps(evidence,indent=2))
print(json.dumps(evidence,indent=2))
