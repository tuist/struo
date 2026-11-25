import hmac
import http.server
import json
import os
import re
import socketserver
import sys
import tempfile
import threading
import time


CHUNK = 64 * 1024
MIN_CHARGE = 4096
KEY = re.compile(r"v1\.(?:cas|ac)\.[0-9a-f]{64}\Z")


def charge(size):
    return max(MIN_CHARGE, ((size + MIN_CHARGE - 1) // MIN_CHARGE) * MIN_CHARGE)


class CacheServer(socketserver.ThreadingMixIn, http.server.HTTPServer):
    daemon_threads = True
    request_queue_size = 8

    def __init__(self, address, token, capacity, directory):
        self.token = ("Bearer " + token).encode("utf-8")
        self.capacity = capacity
        self.directory = os.path.abspath(directory)
        self.io_lock = threading.Lock()
        self.workers = threading.BoundedSemaphore(8)
        os.makedirs(self.directory, mode=0o700, exist_ok=True)
        self.used = 0
        self.last_access = 0
        with os.scandir(self.directory) as entries:
            for entry in entries:
                if entry.name.startswith(".upload-") and entry.is_file(follow_symlinks=False):
                    os.unlink(entry.path)
                elif KEY.fullmatch(entry.name) and entry.is_file(follow_symlinks=False):
                    stat = entry.stat(follow_symlinks=False)
                    self.used += charge(stat.st_size)
                    self.last_access = max(self.last_access, stat.st_atime_ns)
        self.make_room(0, None)
        super().__init__(address, Handler)

    def touch(self, file):
        # Preserve access order across clock ties, rollback, and server restarts.
        next_access = max(time.time_ns(), self.last_access + 1)
        os.utime(file, ns=(next_access, os.fstat(file).st_mtime_ns))
        self.last_access = next_access

    def make_room(self, required, protected):
        while self.used + required > self.capacity:
            oldest = None
            with os.scandir(self.directory) as entries:
                for entry in entries:
                    if entry.name == protected or not KEY.fullmatch(entry.name):
                        continue
                    if not entry.is_file(follow_symlinks=False):
                        continue
                    stat = entry.stat(follow_symlinks=False)
                    candidate = (stat.st_atime_ns, entry.name, stat.st_size)
                    if oldest is None or candidate < oldest:
                        oldest = candidate
            if oldest is None:
                return False
            os.unlink(os.path.join(self.directory, oldest[1]))
            self.used -= charge(oldest[2])
        return True

    def process_request(self, request, client_address):
        if not self.workers.acquire(blocking=False):
            self.shutdown_request(request)
            return
        try:
            super().process_request(request, client_address)
        except BaseException:
            self.workers.release()
            raise

    def process_request_thread(self, request, client_address):
        try:
            super().process_request_thread(request, client_address)
        finally:
            self.workers.release()

    def handle_error(self, request, client_address):
        pass


class Handler(http.server.BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def setup(self):
        self.request.settimeout(10)
        super().setup()

    def log_message(self, format, *args):
        pass

    def reply(self, status, length=0):
        self.close_connection = True
        if self.request_version == "HTTP/0.9":
            self.request_version = "HTTP/1.0"
        self.send_response(status)
        self.send_header("Content-Length", str(length))
        self.send_header("Connection", "close")
        self.end_headers()

    def send_error(self, code, message=None, explain=None):
        self.reply(code)

    def handle_expect_100(self):
        return True

    def parse_request(self):
        if not super().parse_request():
            return False
        auth = self.headers.get_all("Authorization", [])
        supplied = auth[0].encode("utf-8") if len(auth) == 1 else b""
        if not hmac.compare_digest(supplied, self.server.token):
            self.reply(401)
            return False
        # BaseHTTPRequestHandler normalizes leading double slashes, so check the wire target.
        target = self.requestline.split()[1]
        if not target.startswith("/") or not KEY.fullmatch(target[1:]):
            self.reply(400)
            return False
        self.key = target[1:]
        if self.command not in ("GET", "HEAD", "PUT"):
            self.reply(405)
            return False
        lengths = self.headers.get_all("Content-Length", [])
        if self.headers.get_all("Transfer-Encoding") or len(lengths) > 1:
            self.reply(400)
            return False
        if lengths and not re.fullmatch(r"[0-9]+", lengths[0]):
            self.reply(400)
            return False
        if self.command == "PUT" and not lengths:
            self.reply(411)
            return False
        # Compare decimal strings before conversion, including arbitrarily large headers.
        length = (lengths[0].lstrip("0") or "0") if lengths else "0"
        maximum = str(self.server.capacity)
        if len(length) > len(maximum) or (len(length) == len(maximum) and length > maximum):
            self.reply(413)
            return False
        self.length = int(length)
        if self.headers.get("Expect", "").lower() == "100-continue":
            self.send_response_only(100)
            self.end_headers()
        return True

    def do_GET(self):
        self.read_value(False)

    def do_HEAD(self):
        self.read_value(True)

    def read_value(self, head):
        with self.server.io_lock:
            path = os.path.join(self.server.directory, self.key)
            try:
                source = open(path, "rb")
            except FileNotFoundError:
                self.reply(404)
                return
            with source:
                stat = os.fstat(source.fileno())
                self.server.touch(source.fileno())
                self.reply(200, stat.st_size)
                if not head:
                    while block := source.read(CHUNK):
                        self.wfile.write(block)

    def do_PUT(self):
        with self.server.io_lock:
            server = self.server
            path = os.path.join(server.directory, self.key)
            old_charge = charge(os.stat(path).st_size) if os.path.exists(path) else 0
            required = charge(self.length)
            # The old object and staging file both occupy space until atomic replacement.
            if old_charge + required > server.capacity:
                self.reply(507)
                return
            if not server.make_room(required, self.key):
                self.reply(507)
                return
            fd, temporary = tempfile.mkstemp(prefix=".upload-", dir=server.directory)
            server.used += required
            published = False
            try:
                with os.fdopen(fd, "wb") as destination:
                    remaining = self.length
                    while remaining:
                        block = self.rfile.read(min(CHUNK, remaining))
                        if not block:
                            self.reply(400)
                            return
                        destination.write(block)
                        remaining -= len(block)
                    destination.flush()
                    server.touch(destination.fileno())
                    os.fsync(destination.fileno())
                os.replace(temporary, path)
                published = True
                server.used -= old_charge
                directory = os.open(server.directory, os.O_RDONLY)
                try:
                    os.fsync(directory)
                finally:
                    os.close(directory)
                self.reply(204)
            finally:
                if not published:
                    os.unlink(temporary)
                    server.used -= required


def main():
    script, token_file, maximum, port, directory = sys.argv
    capacity = int(maximum)
    if capacity < 0:
        raise ValueError("capacity must be nonnegative")
    with open(token_file, encoding="utf-8") as source:
        token = json.load(source)["token"]
    if not isinstance(token, str) or not token:
        raise ValueError("token must be a nonempty string")
    with CacheServer(("0.0.0.0", int(port)), token, capacity, directory) as server:
        server.serve_forever()


if __name__ == "__main__":
    main()
