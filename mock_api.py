# -*- coding: utf-8 -*-
# mock: OpenAI-compatible SSE chat + /models, for e2e test of sophon ask
import json, socket, threading, sys

PORT = 18742

def handle(conn):
    try:
        buf = b""
        while b"\r\n\r\n" not in buf:
            d = conn.recv(65536)
            if not d:
                return
            buf += d
        head, _, body = buf.partition(b"\r\n\r\n")
        path = head.split(b" ")[1].decode()
        # drain remaining body by content-length
        cl = 0
        for line in head.decode(errors="replace").lower().split("\r\n"):
            if line.startswith("content-length:"):
                cl = int(line.split(":")[1].strip())
        have = len(body)
        while have < cl:
            d = conn.recv(65536)
            if not d:
                break
            have += len(d)
        if path.startswith("/v1/models"):
            resp_body = json.dumps({"data": [{"id": "mock-m-1.0"}, {"id": "mock-m-2.5"}]}).encode()
            conn.sendall(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: " +
                         str(len(resp_body)).encode() + b"\r\nConnection: close\r\n\r\n" + resp_body)
        elif path.startswith("/v1/chat/completions"):
            sse = (
                'data: {"choices":[{"delta":{"role":"assistant"}}]}\n\n'
                'data: {"choices":[{"delta":{"content":"好的，我来找一个可用的 API 并添加。\\n"}}]}\n\n'
                'data: {"choices":[{"delta":{"content":"候选已确定：<SOPHON_APPEND>http://127.0.0.1:%d/v1 mock-key mock-m-2.5</SOPHON_APPEND>\\n"}}]}\n\n'
                'data: [DONE]\n\n'
            ) % PORT
            conn.sendall(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: " +
                         str(len(sse.encode())).encode() + b"\r\nConnection: close\r\n\r\n" + sse.encode())
        else:
            conn.sendall(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
    finally:
        try:
            conn.close()
        except Exception:
            pass

srv = socket.socket()
srv.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)
srv.bind(("127.0.0.1", PORT))
srv.listen(8)
print("mock listening on", PORT, flush=True)
while True:
    c, _ = srv.accept()
    threading.Thread(target=handle, args=(c,), daemon=True).start()
