"""Receive the temporary Connect IQ test app's final result over HTTP."""

import http.server
import os
import sys
import urllib.parse


class ResultHandler(http.server.BaseHTTPRequestHandler):
    result_path = ""

    def do_GET(self):
        request = urllib.parse.urlsplit(self.path)
        if request.path != "/":
            self.send_error(404)
            return

        query = urllib.parse.parse_qs(request.query, strict_parsing=True)
        reports = query.get("result", [])
        if len(reports) != 1 or not reports[0] or len(reports[0]) > 4096:
            self.send_error(400, "expected one non-empty result field")
            return

        temporary_path = self.result_path + ".tmp"
        with open(temporary_path, "w", encoding="utf-8") as result_file:
            result_file.write(reports[0])
            result_file.flush()
            os.fsync(result_file.fileno())
        os.replace(temporary_path, self.result_path)

        body = b"result received\n"
        self.send_response(200)
        self.send_header("Content-Type", "text/plain; charset=utf-8")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def log_message(self, format, *args):
        sys.stderr.write("collector: " + format % args + "\n")


def main():
    if len(sys.argv) != 3:
        raise SystemExit("usage: collector.py PORT RESULT_FILE")
    port = int(sys.argv[1])
    ResultHandler.result_path = sys.argv[2]
    server = http.server.ThreadingHTTPServer(("127.0.0.1", port), ResultHandler)
    print(f"collector: listening on 127.0.0.1:{port}", file=sys.stderr, flush=True)
    server.serve_forever()


if __name__ == "__main__":
    main()
