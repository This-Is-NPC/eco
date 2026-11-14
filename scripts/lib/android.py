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
