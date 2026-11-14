#!/usr/bin/env python3
"""End to end on the Android emulator, against a fake transcription server on
the host (10.0.2.2 from the emulator). Standard library only.

    e2e.py --adb ADB --serial SERIAL --apk APK --cache DIR [scenario ...]

launch runs the real app, which starts its daemon; offline and microphone run
it where it cannot (no network on a first launch, no microphone allowed). The replay scenarios run
the `libeco.so` the APK ships as root, replaying a clip of public speech: the
shipped app has no test-only path, and a process run as the app has no network,
one run as the shell no socket of its own. Each scenario starts from `pm clear`, waits on
conditions with a deadline and fails with what it saw. What the runner puts on
the device (the app, /data/local/tmp/eco-e2e) and the fake server are removed
when it ends, whether it passed or not."""

import argparse
import contextlib
import json
import math
import socket
import subprocess
import sys
import tempfile
import threading
import time
import wave
import zipfile
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
from fake_stt import FakeStt, TEXT  # noqa: E402

PACKAGE = "eco.app"
FILES = f"/data/data/{PACKAGE}/files"
DATA = f"{FILES}/eco"
RUN = f"{FILES}/run"
STAGE = "/data/local/tmp/eco-e2e"
# The daemon of the replay scenarios runs as root (the emulator's image allows
# `adb root`), from the same libeco.so the APK ships: a process run as the app
# or as the shell can have the network or a socket of its own, not both.
SANDBOX = f"{STAGE}/daemon"
HOME_DATA = f"{SANDBOX}/data"
HOME_RUN = f"{SANDBOX}/run"
HOME_SOCKET = f"{HOME_RUN}/eco.sock"
BINARY = f"{SANDBOX}/libeco.so"
MODELS = ("silero_vad.onnx", "wespeaker_campplus.onnx")
KEY = "dummy-key-for-the-fake-server"
# Replayed speech: six seconds of a public meeting recording (AMI, CC BY 4.0), the
# fixture the speaker tests use.
CLIP = Path(__file__).resolve().parents[1] / "fixtures" / "speaker-clips.s16"
SCALES = ("window_animation_scale", "transition_animation_scale", "animator_duration_scale")
DEVICE_ERRORS = ("device offline", "no devices", "device not found", "closed")

LAUNCH_DEADLINE = 90
SETUP_DEADLINE = 600
SESSION_DEADLINE = 90
OUTAGE_DEADLINE = 90
DIALOG_DEADLINE = 30
# The silence between the two copies of the clip in the outage: longer than the retries (7 s).
OUTAGE_GAP_S = 12


class Failure(Exception):
    pass


class Device:
    def __init__(self, adb, serial):
        self.adb_path = adb
        self.serial = serial

    def adb(self, *args, check=True, timeout=300, stdin=None):
        """Run adb; a lost device is waited for and the command run once more."""
        for attempt in (0, 1):
            done = subprocess.run(
                [self.adb_path, "-s", self.serial, *args], capture_output=True, text=True, timeout=timeout, input=stdin
            )
            if done.returncode != 0 and attempt == 0 and any(e in done.stderr for e in DEVICE_ERRORS):
                subprocess.run([self.adb_path, "-s", self.serial, "wait-for-device"], timeout=120)
                continue
            break
        if check and done.returncode != 0:
            raise Failure(f"adb {' '.join(args)} failed ({done.returncode}): {done.stdout}{done.stderr}")
        return done

    def restart_adbd(self, command, uid):
        """`adb root` or `adb unroot`, then wait until the shell runs as `uid`."""
        subprocess.run([self.adb_path, "-s", self.serial, command], capture_output=True, timeout=60)
        subprocess.run([self.adb_path, "-s", self.serial, "wait-for-device"], timeout=120)
        until(f"adb {command}", 60, lambda: self.shell("id", check=False).stdout,
              lambda: self.shell("id", check=False).stdout.startswith(f"uid={uid}("))

    def shell(self, command, **kwargs):
        return self.adb("shell", command, **kwargs)

    def run_as(self, command, **kwargs):
        return self.shell(f"run-as {PACKAGE} sh -c '{command}'", **kwargs)

    def library(self):
        path = self.shell(f"pm path {PACKAGE}").stdout.strip().removeprefix("package:")
        return path.rsplit("/", 1)[0] + "/lib/x86_64/libeco.so"

    def eco(self, *arguments):
        """The app's own `libeco.so` command line, run as the app."""
        return self.run_as(f"ECO_DATA_DIR={DATA} ECO_RUNTIME_DIR={RUN} {self.library()} {' '.join(arguments)}", check=False)

    def home_eco(self, *arguments):
        """The pushed `libeco.so` command line, run as root on the sandbox."""
        return self.shell(f"ECO_DATA_DIR={HOME_DATA} ECO_RUNTIME_DIR={HOME_RUN} {BINARY} {' '.join(arguments)}", check=False)

    def stage(self, source, name):
        """Put a host file at STAGE/name, for the app to copy."""
        self.shell(f"mkdir -p {STAGE}")
        self.adb("push", str(source), f"{STAGE}/{name}")

    def give(self, name, target):
        """Copy a staged file into the app's directory, as the app."""
        self.run_as(f"mkdir -p $(dirname {target}) && cp {STAGE}/{name} {target}")

    def log(self):
        """What the app logged since the log was cleared: the service under `eco-daemon`, the window host under `default`, the tag Qt gives it."""
        return self.shell("logcat -d -s eco-daemon:I default:I", check=False).stdout

    def processes(self):
        return self.shell("ps -A -o ARGS", check=False).stdout

    @contextlib.contextmanager
    def network_off(self):
        """The emulator without wifi and mobile data; the settings are put back, whether the body passed or not."""
        before = {name: self.shell(f"settings get global {name}").stdout.strip() for name in ("wifi_on", "mobile_data")}
        self.shell("svc wifi disable; svc data disable")
        try:
            yield
        finally:
            if before["wifi_on"] != "0":
                self.shell("svc wifi enable", check=False)
            if before["mobile_data"] != "0":
                self.shell("svc data enable", check=False)

    def fresh(self):
        """The app as just installed: no data, the microphone and notifications allowed."""
        self.shell(f"am force-stop {PACKAGE}")
        self.shell(f"pm clear {PACKAGE}")
        for permission in ("RECORD_AUDIO", "POST_NOTIFICATIONS"):
            self.shell(f"pm grant {PACKAGE} android.permission.{permission}")


def until(what, deadline, seen, condition):
    """Poll `condition` until it returns something true, or fail with `seen()`."""
    end = time.monotonic() + deadline
    while True:
        found = condition()
        if found:
            return found
        if time.monotonic() > end:
            raise Failure(f"{what}: not within {deadline} s; saw {seen()}")
        time.sleep(0.25)


class Events:
    """The daemon's events: every line its socket sends, read through `nc`."""

    def __init__(self, device):
        self.lines = []
        self._process = subprocess.Popen(
            [device.adb_path, "-s", device.serial, "shell", f"nc -U {HOME_SOCKET}"],
            stdin=subprocess.PIPE, stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True,
        )
        threading.Thread(target=self._read, daemon=True).start()

    def _read(self):
        for line in self._process.stdout:
            try:
                self.lines.append(json.loads(line))
            except ValueError:
                self.lines.append({"type": "unreadable", "line": line.strip()})

    def send(self, line):
        self._process.stdin.write(line + "\n")
        self._process.stdin.flush()

    def wait(self, what, deadline, condition):
        return until(what, deadline, lambda: [e["type"] + ":" + str(e.get("state", e.get("code", ""))) for e in self.lines[-12:]],
                     lambda: next((e for e in list(self.lines) if condition(e)), None))

    def close(self):
        self._process.kill()
        self._process.wait()


def free_port():
    with socket.socket() as probe:
        probe.bind(("0.0.0.0", 0))
        return probe.getsockname()[1]


def write_clip(path, repeats=1, gap_s=0):
    """The fixture's samples, `repeats` times with `gap_s` of silence between, as
    the 16 kHz mono s16le WAV the daemon replays."""
    with wave.open(str(path), "wb") as out:
        out.setnchannels(1)
        out.setsampwidth(2)
        out.setframerate(16000)
        for copy in range(repeats):
            if copy:
                out.writeframes(bytes(2 * 16000 * gap_s))
            out.writeframes(CLIP.read_bytes())


def config_for(port):
    return f"""[stt]
model = "fake"
language = "en"

[[secrets]]
name = "fake-key"
env = "ECO_E2E_KEY"

[[providers]]
name = "fake"
base_url = "http://10.0.2.2:{port}/v1"
secret = "fake-key"

[[models]]
name = "fake"
type = "transcription"
provider = "fake"
model = "fake-whisper"
"""


class Run:
    def __init__(self, device, apk, cache):
        self.device = device
        self.apk = apk
        self.cache = Path(cache)
        self.measures = {}
        self.workdir = Path(tempfile.mkdtemp(prefix="eco-e2e-"))

    def start_app(self):
        """Launch the app, as a user does; its service starts the daemon."""
        self.device.shell(f"monkey -p {PACKAGE} -c android.intent.category.LAUNCHER 1")

        def answers():
            return "daemon running" in self.device.eco("status").stdout

        until("the daemon answers at its socket", SETUP_DEADLINE,
              lambda: self.device.eco("status").stdout + self.device.shell("logcat -d -s eco-daemon:I").stdout[-600:], answers)

    def cached_models(self):
        """The models on the host: the first time on this machine there are none,
        and the app downloads them, as it does for a user, and the runner keeps them."""
        if all((self.cache / name).exists() for name in MODELS):
            return
        self.device.fresh()
        started = time.monotonic()
        self.start_app()
        self.measures["first_launch_with_download_s"] = round(time.monotonic() - started, 1)
        self.cache.mkdir(parents=True, exist_ok=True)
        for name in MODELS:
            data = subprocess.run(
                [self.device.adb_path, "-s", self.device.serial, "exec-out", f"run-as {PACKAGE} cat {DATA}/models/{name}"],
                capture_output=True, check=True,
            ).stdout
            (self.cache / name).write_bytes(data)

    def daemon(self, port, repeats=1, gap_s=0):
        """The daemon on the replayed clip, with the config for the fake server
        at `port`; returns the process running it and its output."""
        self.cached_models()
        clip = self.workdir / "replay.wav"
        write_clip(clip, repeats, gap_s)
        config = self.workdir / "config.toml"
        config.write_text(config_for(port))
        library = self.workdir / "libeco.so"
        with zipfile.ZipFile(self.apk) as apk:
            library.write_bytes(apk.read("lib/x86_64/libeco.so"))
        self.device.shell(f"rm -rf {SANDBOX}; mkdir -p {HOME_DATA}/models {HOME_RUN}")
        self.device.adb("push", str(library), BINARY)
        self.device.shell(f"chmod 755 {BINARY}")
        self.device.adb("push", str(clip), f"{SANDBOX}/replay.wav")
        self.device.adb("push", str(config), f"{HOME_DATA}/config.toml")
        for name in MODELS:
            self.device.adb("push", str(self.cache / name), f"{HOME_DATA}/models/{name}")
        command = (
            f"ECO_DATA_DIR={HOME_DATA} ECO_RUNTIME_DIR={HOME_RUN} ECO_E2E_KEY={KEY} "
            f"{BINARY} daemon --headless --replay {SANDBOX}/replay.wav"
        )
        process = subprocess.Popen(
            [self.device.adb_path, "-s", self.device.serial, "shell", command],
            stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True,
        )
        output = []
        threading.Thread(target=lambda: output.extend(process.stdout), daemon=True).start()
        return process, output

    def seen(self, server, events, output):
        """What a failed wait reports: the requests, the last events, the daemon's output and the session log."""
        return json.dumps({
            "requests": [request.status for request in server.seen()],
            "events": [e.get("code", e["type"]) for e in events.lines[-10:]],
            "daemon": "".join(output)[-400:],
            "log": self.sessions()[-400:],
        })

    def listening(self, process, output):
        """Wait until the daemon answers at its socket; fail as soon as it exits."""
        def answers():
            if process.poll() is not None:
                raise Failure(f"the daemon exited with {process.returncode}: {''.join(output)[-600:]}")
            return self.device.home_eco("status").stdout.startswith("eco: daemon running")

        until("the daemon listens", LAUNCH_DEADLINE, lambda: "".join(output)[-600:], answers)

    def stop_daemon(self, process):
        self.device.home_eco("stop")
        try:
            process.wait(30)
        except subprocess.TimeoutExpired:
            self.device.shell("pkill libeco.so", check=False)
            process.kill()
            process.wait()

    def sessions(self):
        return self.device.shell(f"cat {HOME_DATA}/sessions/*", check=False).stdout

    # Scenarios

    def launch(self):
        """The app starts its daemon, and the daemon's socket answers."""
        self.cached_models()
        self.device.fresh()
        for name in MODELS:
            self.device.stage(self.cache / name, name)
            self.device.give(name, f"{DATA}/models/{name}")
        started = time.monotonic()
        self.start_app()
        self.measures["launch_to_socket_s"] = round(time.monotonic() - started, 1)
        if not self.device.shell(f"pidof {PACKAGE}").stdout.strip():
            raise Failure("the app is not running")

    def offline(self):
        """A first launch without the network starts no daemon and says why; once the network is
        back, the models are downloaded and the daemon starts, with no relaunch."""
        self.device.fresh()
        self.device.shell("logcat -c")
        with self.device.network_off():
            self.device.shell(f"monkey -p {PACKAGE} -c android.intent.category.LAUNCHER 1")
            until("the failed setup is told", LAUNCH_DEADLINE, self.device.log, lambda: "status models.failed" in self.device.log())
            if "libeco.so daemon" in self.device.processes():
                raise Failure(f"a daemon runs without models: {self.device.processes()}")
            if "daemon running" in self.device.eco("status").stdout:
                raise Failure("the daemon answers without models")
            self.device.shell("logcat -c")
        until("the daemon starts once the network is back", SETUP_DEADLINE, self.device.log,
              lambda: "daemon running" in self.device.eco("status").stdout)
        log = self.device.log()
        if "status models.preparing" not in log or "status models.failed" in log.split("status models.preparing", 1)[0]:
            raise Failure(f"setup did not run again from the retry: {log[-600:]}")

    def microphone(self):
        """With the microphone refused the daemon is not started and the window host reports it;
        once it is allowed and the app is back in front, the daemon starts."""
        self.device.fresh()
        for name in MODELS:
            self.device.stage(self.cache / name, name)
            self.device.give(name, f"{DATA}/models/{name}")
        self.device.shell(f"pm revoke {PACKAGE} android.permission.RECORD_AUDIO")
        self.device.shell("logcat -c")
        self.device.shell(f"monkey -p {PACKAGE} -c android.intent.category.LAUNCHER 1")
        until("the system asks for the microphone", DIALOG_DEADLINE, lambda: self.device.shell("dumpsys window | grep mCurrentFocus").stdout,
              lambda: "GrantPermissionsActivity" in self.device.shell("dumpsys window | grep mCurrentFocus").stdout)
        self.device.shell("input keyevent KEYCODE_BACK")
        until("the refusal is told", DIALOG_DEADLINE, self.device.log, lambda: "status: microphone." in self.device.log())
        if "libeco.so daemon" in self.device.processes() or "daemon running" in self.device.eco("status").stdout:
            raise Failure(f"a daemon runs without the microphone: {self.device.processes()}")
        self.device.shell(f"pm grant {PACKAGE} android.permission.RECORD_AUDIO")
        self.device.shell("input keyevent KEYCODE_HOME")
        self.device.shell(f"monkey -p {PACKAGE} -c android.intent.category.LAUNCHER 1")
        until("the daemon starts once the microphone is allowed", LAUNCH_DEADLINE, self.device.log,
              lambda: "daemon running" in self.device.eco("status").stdout)

    def replay(self):
        """A replayed session reaches the fake server with the key, and its text is in the session log."""
        self.device.fresh()
        server = FakeStt(free_port())
        server.start()
        process, output = self.daemon(server.port)
        events = None
        try:
            self.listening(process, output)
            events = Events(self.device)
            events.wait("the greeting", 10, lambda e: e["type"] == "daemon")
            events.send('session.start {"language": "en"}')
            until("the fake server is sent audio", SESSION_DEADLINE, lambda: self.seen(server, events, output), server.seen)
            request = server.seen()[0]
            if request.authorization != f"Bearer {KEY}" or request.model != "fake-whisper" or not request.wav:
                raise Failure(f"the request was not as configured: {vars(request)}")
            until("the transcript is in the session log", SESSION_DEADLINE, lambda: self.seen(server, events, output),
                  lambda: TEXT in self.sessions())
        finally:
            if events:
                events.close()
            self.stop_daemon(process)
            server.stop()

    def retry(self):
        """A provider that fails twice is tried again, and the segment is not lost."""
        self.device.fresh()
        server = FakeStt(free_port())
        server.failing = 2
        server.start()
        process, output = self.daemon(server.port)
        events = None
        try:
            self.listening(process, output)
            events = Events(self.device)
            events.wait("the greeting", 10, lambda e: e["type"] == "daemon")
            events.send('session.start {"language": "en"}')
            until("the transcript is in the session log", SESSION_DEADLINE, lambda: self.seen(server, events, output),
                  lambda: TEXT in self.sessions())
            statuses = [request.status for request in server.seen()][:3]
            if statuses != [503, 503, 200]:
                raise Failure(f"the first segment was not tried again after two failures: {statuses}")
            if any(e.get("code") == "transcription.failed" for e in events.lines):
                raise Failure(f"a segment was reported lost: {events.lines[-8:]}")
        finally:
            if events:
                events.close()
            self.stop_daemon(process)
            server.stop()

    def outage(self):
        """A provider that stays down loses the segment, which is reported, and the
        speech after it is transcribed once the provider is back."""
        self.device.fresh()
        server = FakeStt(free_port())
        server.failing = math.inf
        server.start()
        process, output = self.daemon(server.port, repeats=2, gap_s=OUTAGE_GAP_S)
        events = None
        try:
            self.listening(process, output)
            events = Events(self.device)
            events.wait("the greeting", 10, lambda e: e["type"] == "daemon")
            events.send('session.start {"language": "en"}')
            lost = events.wait("the lost segment is told", OUTAGE_DEADLINE, lambda e: e.get("code") == "transcription.failed")
            if "10.0.2.2" not in lost["params"]["detail"]:
                raise Failure(f"the failure does not name the provider: {lost}")
            server.failing = 0
            until("the transcript is in the session log", SESSION_DEADLINE, lambda: self.seen(server, events, output),
                  lambda: TEXT in self.sessions())
        finally:
            if events:
                events.close()
            self.stop_daemon(process)
            server.stop()


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--adb", required=True)
    parser.add_argument("--serial", required=True)
    parser.add_argument("--apk", required=True)
    parser.add_argument("--cache", required=True)
    parser.add_argument("scenarios", nargs="*")
    args = parser.parse_args()
    device = Device(args.adb, args.serial)
    run = Run(device, args.apk, args.cache)
    available = {"launch": run.launch, "offline": run.offline, "microphone": run.microphone, "replay": run.replay, "retry": run.retry, "outage": run.outage}
    unknown = [name for name in args.scenarios if name not in available]
    if unknown:
        sys.exit(f"unknown scenarios {unknown}; they are {list(available)}")
    chosen = args.scenarios or list(available)

    scales = {name: device.shell(f"settings get global {name}").stdout.strip() for name in SCALES}
    failed = []
    started = time.monotonic()
    try:
        device.restart_adbd("root", 0)
        device.adb("install", "-r", "-t", args.apk)
        for name in SCALES:
            device.shell(f"settings put global {name} 0")
        for name in chosen:
            begun = time.monotonic()
            try:
                available[name]()
                print(f"PASS {name} ({time.monotonic() - begun:.1f} s)", flush=True)
            except Failure as failure:
                failed.append(name)
                print(f"FAIL {name} ({time.monotonic() - begun:.1f} s): {failure}", flush=True)
    finally:
        for name, value in scales.items():
            device.shell(f"settings put global {name} {value}" if value != "null" else f"settings delete global {name}", check=False)
        device.shell(f"am force-stop {PACKAGE}", check=False)
        device.adb("uninstall", PACKAGE, check=False)
        device.shell(f"rm -rf {STAGE}", check=False)
        device.restart_adbd("unroot", 2000)
        subprocess.run(["rm", "-rf", str(run.workdir)])
    run.measures["e2e_total_s"] = round(time.monotonic() - started, 1)
    print("measured", json.dumps(run.measures))
    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    main()
