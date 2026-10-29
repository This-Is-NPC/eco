"""The Windows pieces of scripts/package (the release build and the eco folder
the zip holds, docs/design.md §13) and of scripts/install-windows and
scripts/uninstall-windows (install.ps1, run on that zip)."""

import os
import shutil
import subprocess


def build(root):
    """Build the release eco.exe and eco-launch.exe; eco-window must be in target/window already."""
    if not (root / "target/window/eco-window.exe").exists():
        raise SystemExit("no target/window/eco-window.exe; run `mise run window:build`")
    subprocess.run(["cargo", "build", "--release", "--locked", "--quiet"], cwd=root, check=True)


def stage(root, eco):
    """Lay out the eco folder at `eco` from the build, replacing what is there."""
    if eco.exists():
        shutil.rmtree(eco)
    (eco / "bin").mkdir(parents=True)
    shutil.copy2(root / "target/release/eco.exe", eco / "bin/eco.exe")
    shutil.copytree(root / "target/window", eco / "lib/eco")
    shutil.copy2(root / "target/release/eco-launch.exe", eco / "lib/eco/eco-launch.exe")
    shutil.copytree(root / "overlay", eco / "share/eco/overlay")
    shutil.copy2(root / "packaging/windows/eco.ico", eco / "share/eco/eco.ico")
    shutil.copy2(root / "LICENSE", eco / "LICENSE")
    shutil.copy2(root / "packaging/windows/README.txt", eco / "README.txt")


def installer(root, **env):
    """Run install.ps1, the installer a release is installed with, with `env`
    added to the environment; exit with its status when it fails."""
    status = subprocess.run(
        ["powershell", "-NoProfile", "-ExecutionPolicy", "Bypass", "-File", root / "install.ps1"],
        env=os.environ | env,
    ).returncode
    if status:
        raise SystemExit(status)
