"""The Android SDK pieces eco pins, and where they sit in the checkout."""

import os
from pathlib import Path

PLATFORM = "android-36"
BUILD_TOOLS = "36.0.0"
NDK = "29.0.14206865"
IMAGE = f"system-images;{PLATFORM};google_apis;x86_64"
AVD = "eco"
# The oldest Android the daemon runs on; the NDK compilers are named by it.
MIN_API = 26


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
