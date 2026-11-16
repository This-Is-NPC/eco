"""A fake transcription server for the Android end-to-end test: the endpoint
the daemon's OpenAI-compatible adapter posts to, `POST /v1/audio/transcriptions`,
answering one fixed text and keeping what it was sent."""

import json
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

TEXT = "eco hears the replayed session"


class Request:
    """What one POST carried: the key, the model, whether the file is a WAV, and the status it got."""

    def __init__(self, authorization, model, wav, status):
        self.authorization = authorization
        self.model = model
        self.wav = wav
        self.status = status


class FakeStt:
    """Listens on the host's `port` for as long as it is started. `failing` is how
    many of the next requests it answers 503, `math.inf` for all: a provider
    that is down."""

    def __init__(self, port):
        self.port = port
        self.failing = 0
        self.requests = []
        self._lock = threading.Lock()
        self._server = None
        self._thread = None

    def start(self):
        owner = self

        class Handler(BaseHTTPRequestHandler):
            def do_POST(self):
                body = self.rfile.read(int(self.headers.get("Content-Length", 0)))
                if self.path != "/v1/audio/transcriptions":
                    self.send_error(404)
                    return
                model = b'name="model"\r\n\r\n'
                start = body.find(model)
                found = body[start + len(model):].split(b"\r\n", 1)[0].decode() if start >= 0 else ""
                with owner._lock:
                    status = 503 if owner.failing > 0 else 200
                    if owner.failing > 0:
                        owner.failing -= 1
                    owner.requests.append(Request(self.headers.get("Authorization"), found, b"RIFF" in body, status))
                if status != 200:
                    self.send_error(status)
                    return
                answer = json.dumps({"text": TEXT}).encode()
                self.send_response(200)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(answer)))
                self.end_headers()
                self.wfile.write(answer)

            def log_message(self, *args):
                pass

        self._server = ThreadingHTTPServer(("0.0.0.0", self.port), Handler)
        self._server.daemon_threads = True
        self._thread = threading.Thread(target=self._server.serve_forever, daemon=True)
        self._thread.start()

    def stop(self):
        if self._server is None:
            return
        self._server.shutdown()
        self._server.server_close()
        self._thread.join()
        self._server = None

    def seen(self):
        with self._lock:
            return list(self.requests)
