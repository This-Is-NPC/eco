"""What scripts/package, scripts/install-windows and scripts/uninstall-windows
share on Windows: the release build, the eco folder the zip and the install
hold (docs/design.md §13), the daemon's stop, and the user's PATH."""

import ctypes
import os
import shutil
import subprocess
import time
import winreg
from pathlib import Path

ENVIRONMENT = "Environment"


def build(root):
    """Build the release eco.exe; eco-window must be in target/window already."""
    if not (root / "target/window/eco-window.exe").exists():
        raise SystemExit("no target/window/eco-window.exe; run `mise run window:build`")
    subprocess.run(["cargo", "build", "--release", "--locked", "--quiet"], cwd=root, check=True)


def stage(root, eco):
    """Lay out the eco folder at `eco` from the build, replacing what is there."""
    remove(eco)
    (eco / "bin").mkdir(parents=True)
    shutil.copy2(root / "target/release/eco.exe", eco / "bin/eco.exe")
    shutil.copytree(root / "target/window", eco / "lib/eco")
    shutil.copytree(root / "overlay", eco / "share/eco/overlay")
    shutil.copy2(root / "packaging/eco.svg", eco / "eco.svg")
    shutil.copy2(root / "LICENSE", eco / "LICENSE")
    shutil.copy2(root / "packaging/windows/README.txt", eco / "README.txt")


def install_dir():
    """Where the install puts the eco folder."""
    return Path(os.environ["LOCALAPPDATA"]) / "Programs" / "eco"


def stop(exe, eco):
    """Stop the daemon through `exe`, any eco.exe, and end the eco-window
    processes run from the eco folder `eco`. Returns None when no daemon ran,
    else whether its window was open."""
    status = subprocess.run([exe, "status"], stdout=subprocess.DEVNULL).returncode
    if status not in (0, 3):
        raise SystemExit("cannot determine whether the eco daemon is running")
    running = status == 0
    window = running and subprocess.run([exe, "status", "--window-open"], stdout=subprocess.DEVNULL).returncode == 0
    if running:
        subprocess.run([exe, "stop"], check=True)
    subprocess.run(
        ["powershell", "-NoProfile", "-Command",
         f"Get-Process | Where-Object {{ $_.Name -eq 'eco-window' -and $_.Path -like '{eco}\\*' }} | Stop-Process -Force"],
        check=True,
    )
    return window if running else None


def remove(path):
    """Delete the folder `path`, waiting up to 5 seconds for processes that
    have just exited to let go of its files."""
    for _ in range(50):
        try:
            shutil.rmtree(path)
            return
        except FileNotFoundError:
            return
        except PermissionError:
            time.sleep(0.1)
    shutil.rmtree(path)


def _same(entry, directory):
    expanded = os.path.normcase(os.path.expandvars(entry).rstrip("\\"))
    return expanded == os.path.normcase(str(directory).rstrip("\\"))


def _user_path():
    with winreg.OpenKey(winreg.HKEY_CURRENT_USER, ENVIRONMENT) as key:
        try:
            return winreg.QueryValueEx(key, "Path")
        except FileNotFoundError:
            return "", winreg.REG_EXPAND_SZ


def _set_user_path(value, kind):
    with winreg.OpenKey(winreg.HKEY_CURRENT_USER, ENVIRONMENT, 0, winreg.KEY_SET_VALUE) as key:
        winreg.SetValueEx(key, "Path", 0, kind, value)
    # WM_SETTINGCHANGE to every top-level window, so Explorer gives the new
    # PATH to the programs it starts from now on.
    ctypes.windll.user32.SendMessageTimeoutW(0xFFFF, 0x001A, 0, ENVIRONMENT, 0x0002, 5000, None)


def add_to_user_path(directory):
    """Append `directory` to the user's PATH unless an entry names it already;
    every other entry and the value's type stay. Returns whether it added."""
    value, kind = _user_path()
    if any(_same(entry, directory) for entry in value.split(";") if entry):
        return False
    _set_user_path(f"{value.rstrip(';')};{directory}" if value.strip(";") else str(directory), kind)
    return True


def remove_from_user_path(directory):
    """Remove every entry naming `directory` from the user's PATH; the others
    and the value's type stay. Returns whether it removed one."""
    value, kind = _user_path()
    entries = value.split(";")
    kept = [entry for entry in entries if not (entry and _same(entry, directory))]
    if kept == entries:
        return False
    _set_user_path(";".join(kept), kind)
    return True
