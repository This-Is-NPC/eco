"""The Android SDK pieces eco pins, and where they sit in the checkout."""

import contextlib
import os
import subprocess
import sys
import time
from pathlib import Path

PLATFORM = "android-36"
BUILD_TOOLS = "36.0.0"
NDK = "29.0.14206865"
IMAGE = f"system-images;{PLATFORM};google_apis;x86_64"
AVD = "eco"
# The oldest Android the daemon runs on; the NDK compilers are named by it.
MIN_API = 26
TARGET = "x86_64-linux-android"
EMULATOR_STOP_DEADLINE = 60  # seconds for a killed emulator to leave `adb devices`


def root():
    return Path(os.environ.get("MISE_PROJECT_ROOT") or Path(__file__).resolve().parents[2])


def sdk():
    return root() / ".android-sdk"


def ndk():
    return sdk() / "ndk" / NDK


def llvm_bin():
    return ndk() / "toolchains/llvm/prebuilt/linux-x86_64/bin"


def adb():
    return sdk() / "platform-tools" / "adb"


def emulator():
    return sdk() / "emulator" / "emulator"


def emulator_args():
    """The emulator command line for the `AVD`: the host GPU, because
    SwiftShader paints Qt Quick rectangles wrongly, and no snapshot written."""
    return [str(emulator()), "-avd", AVD, "-gpu", "host", "-no-snapshot-save", "-no-boot-anim"]


def emulator_env(environ):
    return {**environ, "ANDROID_AVD_HOME": str(sdk() / "avd"), "ANDROID_HOME": str(sdk())}


def booted_emulators(devices_output):
    """The serials of the emulators `adb devices` lists as online."""
    serials = []
    for line in devices_output.splitlines()[1:]:
        words = line.split()
        if len(words) == 2 and words[0].startswith("emulator-") and words[1] == "device":
            serials.append(words[0])
    return serials


def boot_timeout_message(seconds, log_tail):
    return f"the emulator did not finish booting within {seconds} s; end of its log:\n{log_tail}"


def cargo_env():
    """The environment cargo builds TARGET in: crates that compile C (ring) need
    the NDK's clang and llvm-ar."""
    clang = llvm_bin() / f"{TARGET}{MIN_API}-clang"
    if not clang.exists():
        sys.exit(f"the Android NDK is not installed ({clang} is missing): run `mise run android:setup`")
    triple = TARGET.replace("-", "_")
    return {
        **os.environ,
        f"CC_{triple}": str(clang),
        f"AR_{triple}": str(llvm_bin() / "llvm-ar"),
        f"CARGO_TARGET_{triple.upper()}_LINKER": str(clang),
    }


def running_emulators():
    done = subprocess.run([str(adb()), "devices"], capture_output=True, text=True)
    return booted_emulators(done.stdout)


@contextlib.contextmanager
def emulator_session():
    """The serial of an emulator to run on: the one already running, which is
    left running, or one booted here, which is killed on the way out, whether
    the body passed or not."""
    serials = running_emulators()
    if serials:
        yield serials[0]
        return
    subprocess.run([str(root() / "scripts" / "android-emulator")], check=True)
    serial = running_emulators()[0]
    try:
        yield serial
    finally:
        subprocess.run([str(adb()), "-s", serial, "emu", "kill"], capture_output=True)
        end = time.monotonic() + EMULATOR_STOP_DEADLINE
        while serial in running_emulators():
            if time.monotonic() > end:
                sys.exit(f"{serial} is still running {EMULATOR_STOP_DEADLINE} s after it was killed")
            time.sleep(1)
