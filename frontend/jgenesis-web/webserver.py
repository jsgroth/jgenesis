#!/usr/bin/env python3

"""
An extension of Python's simple HTTP server that sets the Cross-Origin-Opener-Policy and Cross-Origin-Embedder-Policy
HTTP headers to same-origin and require-corp respectively.

Listens on localhost:8080 by default but you can override this by passing a command line arg, e.g.:
  ./webserver.py localhost:9001
"""

from http.server import HTTPServer, SimpleHTTPRequestHandler
import sys


class Handler(SimpleHTTPRequestHandler):
    def __init__(self, *args, **kwargs):
        super().__init__(*args, **kwargs)

    # Overridden to set headers
    def send_response(self, code, message=None):
        super().send_response(code, message)

        self.send_header("Cross-Origin-Opener-Policy", "same-origin")
        self.send_header("Cross-Origin-Embedder-Policy", "require-corp")


def run(host, port):
    httpd = HTTPServer((host, port), Handler)
    httpd.serve_forever()


def main():
    server_address = "localhost:8080"
    if len(sys.argv) > 1:
        server_address = sys.argv[1]

    (host, port) = server_address.split(":")

    print(f"Starting webserver listening on http://{host}:{port}/")

    run(host, int(port))


if __name__ == "__main__":
    main()
