import concurrent.futures
import http.client
import json
import os
from pathlib import Path
import socket
import subprocess
import sys
import tempfile
import time
import unittest


TOKEN = "test-secret"
AUTH = "Bearer " + TOKEN


def key(number=1, kind="cas"):
    return f"/v1.{kind}.{number:064x}"


class ServerTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.data = self.root / "data"
        self.token_file = self.root / "token.json"
        self.token_file.write_text(json.dumps({"token": TOKEN}))
        self.token_file.chmod(0o600)
        self.process = None
        self.addCleanup(self.stop)
        self.start()

    def start(self, capacity=1024 * 1024):
        with socket.socket() as listener:
            listener.bind(("127.0.0.1", 0))
            self.port = listener.getsockname()[1]
        self.process = subprocess.Popen(
            [sys.executable, str(Path(__file__).with_name("server.py")),
             str(self.token_file), str(capacity), str(self.port), str(self.data)],
            stdout=subprocess.PIPE, stderr=subprocess.PIPE,
        )
        deadline = time.monotonic() + 5
        while time.monotonic() < deadline:
            if self.process.poll() is not None:
                self.fail(f"server exited: {self.process.communicate()!r}")
            try:
                with socket.create_connection(("127.0.0.1", self.port), timeout=0.1):
                    return
            except OSError:
                time.sleep(0.01)
        self.fail("server did not start")

    def stop(self):
        if self.process is not None:
            self.process.terminate()
            output = self.process.communicate(timeout=5)
            self.process = None
            self.assertEqual(output, (b"", b""))

    def request(self, method="GET", path=None, body=None, auth=AUTH):
        connection = http.client.HTTPConnection("127.0.0.1", self.port, timeout=5)
        try:
            headers = {} if auth is None else {"Authorization": auth}
            connection.request(method, path or key(), body=body, headers=headers)
            response = connection.getresponse()
            result = response.status, response.read(), response.getheader("Content-Length")
            self.assertIsNotNone(result[2])
            self.assertEqual(response.getheader("Connection"), "close")
            return result
        finally:
            connection.close()

    def raw(self, headers, body=b"", method="PUT", path=None, auth=AUTH):
        with socket.create_connection(("127.0.0.1", self.port), timeout=5) as client:
            request = f"{method} {path or key()} HTTP/1.1\r\nHost: localhost\r\n"
            if auth is not None:
                request += f"Authorization: {auth}\r\n"
            request += "".join(f"{name}: {value}\r\n" for name, value in headers)
            client.sendall(request.encode() + b"\r\n" + body)
            client.shutdown(socket.SHUT_WR)
            response = http.client.HTTPResponse(client)
            response.begin()
            result = response.status, response.read(), response.getheader("Content-Length")
            self.assertIsNotNone(result[2])
            self.assertEqual(response.getheader("Connection"), "close")
            return result

    def test_auth_for_hits_misses_and_every_method(self):
        self.assertEqual(self.request("PUT", body=b"secret")[0], 204)
        for method in ("GET", "HEAD", "PUT", "DELETE"):
            for path in (key(), key(2), "/invalid"):
                for auth in (None, "Bearer wrong", "bearer " + TOKEN):
                    with self.subTest(method=method, path=path, auth=auth):
                        self.assertEqual(self.request(method, path, auth=auth)[0], 401)
        self.assertEqual(self.raw([("Authorization", AUTH)], auth=AUTH)[0], 401)
        self.assertEqual(self.request()[1], b"secret")

    def test_key_and_path_rejection(self):
        invalid = ["/", key().upper(), key() + "?x=1", key() + "#x", key() + "/",
                   key()[:-1], key() + "0", "/v1.other." + "0" * 64,
                   "//" + key()[1:], "/../" + key()[1:],
                   "/v1.cas." + "g" * 64, "/%76" + key()[2:],
                   "http://localhost" + key()]
        for path in invalid:
            with self.subTest(path=path):
                self.assertEqual(self.raw([], method="GET", path=path)[0], 400)
        self.assertEqual(self.request("DELETE")[0], 405)
        for kind in ("cas", "ac"):
            self.assertEqual(self.request("PUT", key(kind=kind), b"ok")[0], 204)
            self.assertEqual(self.request(path=key(kind=kind))[1], b"ok")

    def test_content_length_and_transfer_encoding(self):
        cases = [([], 411), ([("Content-Length", "-1")], 400),
                 ([("Content-Length", "+1")], 400),
                 ([("Content-Length", "1, 1")], 400),
                 ([("Content-Length", "")], 400),
                 ([("Content-Length", "1"), ("Content-Length", "1")], 400),
                 ([("Content-Length", "0"), ("Transfer-Encoding", "chunked")], 400),
                 ([("Transfer-Encoding", "identity")], 400),
                 ([("Content-Length", str(1024 * 1024 + 1))], 413),
                 ([("Content-Length", "9" * 5000)], 413)]
        for headers, status in cases:
            with self.subTest(headers=headers):
                self.assertEqual(self.raw(headers)[0], status)
        self.assertEqual(self.raw([("Content-Length", "0000")])[0], 204)
        self.assertEqual(self.request()[1], b"")

    def test_exact_capacity_and_unequal_size_eviction(self):
        self.stop()
        self.start(20480)
        self.assertEqual(self.request("PUT", key(1), b"a" * 6000)[0], 204)
        self.assertEqual(self.request("PUT", key(2), b"b" * 10288)[0], 204)
        self.stop()
        tied = time.time_ns() + 3_600_000_000_000
        for number in (1, 2):
            path = self.data / key(number)[1:]
            os.utime(path, ns=(tied, path.stat().st_mtime_ns))
        self.start(20480)
        self.assertEqual(self.request("HEAD", key(1))[2], "6000")
        self.assertEqual(self.request("PUT", key(3), b"")[0], 204)
        self.assertEqual(self.request(path=key(2))[0], 404)
        self.assertEqual(self.request(path=key(1))[1], b"a" * 6000)
        self.assertEqual(self.request(path=key(3))[1], b"")
        self.stop()
        self.start(4096)
        self.assertEqual(self.request(path=key(1))[0], 404)
        self.assertEqual(self.request(path=key(3))[0], 200)

    def test_minimum_charge_and_too_small_capacity(self):
        self.stop()
        self.start(4096)
        self.assertEqual(self.request("PUT", key(1), b"")[0], 204)
        self.assertEqual(self.request("PUT", key(2), b"")[0], 204)
        self.assertEqual(self.request(path=key(1))[0], 404)
        self.assertEqual(self.request(path=key(2))[0], 200)
        self.stop()
        self.start(4095)
        self.assertEqual(self.request("PUT", body=b"")[0], 507)

    def test_abandoned_upload_preserves_old_value(self):
        self.assertEqual(self.request("PUT", body=b"previous")[0], 204)
        self.assertEqual(self.raw([("Content-Length", "80000")], b"partial")[0], 400)
        self.assertEqual(self.request()[1], b"previous")
        self.assertEqual(self.raw([("Content-Length", "80000")], b"partial", path=key(2))[0], 400)
        self.assertEqual(self.request(path=key(2))[0], 404)
        self.assertEqual(list(self.data.glob(".upload-*")), [])

    def test_replacement_accounts_for_old_and_staging_files(self):
        self.stop()
        self.start(8192)
        self.assertEqual(self.request("PUT", body=b"previous")[0], 204)
        self.assertEqual(self.request("PUT", body=b"x" * 4097)[0], 507)
        self.assertEqual(self.request()[1], b"previous")
        self.assertEqual(self.request("PUT", body=b"new")[0], 204)
        self.assertEqual(self.request()[1], b"new")
        self.assertEqual(self.request("PUT", key(2), b"")[0], 204)
        self.assertEqual(self.request()[1], b"new")

    def test_restart_persistence_cleanup_and_quota_reconstruction(self):
        self.assertEqual(self.request("PUT", key(1), b"a" * 6000)[0], 204)
        self.assertEqual(self.request("PUT", key(2), b"b" * 10288)[0], 204)
        self.stop()
        (self.data / ".upload-abandoned").write_bytes(b"incomplete")
        self.start(20480)
        self.assertEqual(list(self.data.glob(".upload-*")), [])
        self.assertEqual(self.request(path=key(1))[1], b"a" * 6000)
        self.assertEqual(self.request("HEAD", key(2))[2], "10288")
        self.assertEqual(self.request("PUT", key(3), b"")[0], 204)
        self.assertEqual(self.request(path=key(1))[0], 404)
        self.assertEqual(self.request(path=key(2))[1], b"b" * 10288)
        self.assertEqual(self.request(path=key(3))[0], 200)
        self.stop()
        self.start(4096)
        self.assertEqual(self.request(path=key(2))[0], 404)
        self.assertEqual(self.request(path=key(3))[:2], (200, b""))

    def test_process_restart_during_upload(self):
        self.assertEqual(self.request("PUT", body=b"previous")[0], 204)
        with socket.create_connection(("127.0.0.1", self.port), timeout=5) as client:
            client.sendall(
                f"PUT {key()} HTTP/1.1\r\nHost: localhost\r\nAuthorization: {AUTH}\r\n"
                "Content-Length: 100000\r\n\r\n".encode() + b"x" * 70000
            )
            deadline = time.monotonic() + 3
            while time.monotonic() < deadline:
                if any(path.stat().st_size for path in self.data.glob(".upload-*")):
                    break
                time.sleep(0.01)
            else:
                self.fail("upload was not staged")
            self.stop()
        self.start()
        self.assertEqual(self.request()[1], b"previous")
        self.assertEqual(list(self.data.glob(".upload-*")), [])

    def test_worker_limit_closes_excess_connections(self):
        clients = []
        try:
            for _ in range(8):
                client = socket.create_connection(("127.0.0.1", self.port), timeout=5)
                clients.append(client)
                client.sendall(b"GET /")
            time.sleep(0.1)
            with socket.create_connection(("127.0.0.1", self.port), timeout=2) as excess:
                self.assertEqual(excess.recv(1), b"")
        finally:
            for client in clients:
                client.close()

    def test_invalid_requests_close_without_waiting_for_body(self):
        for auth, length, status in (("wrong", "1000000", 401), (AUTH, "999999999", 413)):
            with socket.create_connection(("127.0.0.1", self.port), timeout=2) as client:
                client.sendall(
                    f"PUT {key()} HTTP/1.1\r\nHost: localhost\r\nAuthorization: {auth}\r\n"
                    f"Content-Length: {length}\r\n\r\n".encode()
                )
                response = http.client.HTTPResponse(client)
                response.begin()
                self.assertEqual(response.status, status)
                self.assertEqual(response.read(), b"")
                self.assertEqual(client.recv(1), b"")

    def test_streaming_larger_than_chunk_and_duplicate_requests(self):
        value = bytes(range(256)) * 1000
        self.assertEqual(self.request("PUT", body=value)[0], 204)
        for _ in range(2):
            self.assertEqual(self.request()[1], value)
            self.assertEqual(self.request("HEAD")[2], str(len(value)))
            self.assertEqual(self.request("PUT", body=value)[0], 204)
        with concurrent.futures.ThreadPoolExecutor(max_workers=4) as workers:
            results = list(workers.map(lambda _: self.request("PUT", body=value)[0], range(8)))
        self.assertEqual(results, [204] * 8)
        self.assertEqual(self.request()[1], value)

    def test_expect_does_not_bypass_auth_or_size_rejection(self):
        headers = [("Expect", "100-continue"), ("Content-Length", "999999999")]
        self.assertEqual(self.raw(headers, auth=None)[0], 401)
        self.assertEqual(self.raw(headers)[0], 413)


if __name__ == "__main__":
    unittest.main()
