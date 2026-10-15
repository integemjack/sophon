# -*- coding: utf-8 -*-
# e2e: sophon ask streaming + auto-append, against in-process mock
import json, socket, subprocess, threading, os, shutil, sys, time

ROOT = r"C:\Users\ultra\Desktop\works\hermes"
EXE = os.path.join(ROOT, "target", "release", "sophon.exe")
E2E = os.path.join(ROOT, "e2e_tmp")
PORT = 18743

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
            rb = json.dumps({"data": [{"id": "mock-m-1.0"}, {"id": "mock-m-2.5"}]}).encode()
            conn.sendall(b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: "
                         + str(len(rb)).encode() + b"\r\nConnection: close\r\n\r\n" + rb)
        elif path.startswith("/v1/chat/completions"):
            chunks = [
                "好的，正在找可用的 API。\n",
                "候选确定：<SOPHON_APPEND>http://127.0.0.1:%d/v1 mock-key mock-m-2.5</SOPHON_APPEND>\n" % PORT,
            ]
            sse = ""
            for c in chunks:
                sse += 'data: {"choices":[{"delta":{"content":%s}}]}\n\n' % json.dumps(c)
            sse += "data: [DONE]\n\n"
            b = sse.encode()
            conn.sendall(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nContent-Length: "
                         + str(len(b)).encode() + b"\r\nConnection: close\r\n\r\n" + b)
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

def serve():
    while True:
        c, _ = srv.accept()
        threading.Thread(target=handle, args=(c,), daemon=True).start()
threading.Thread(target=serve, daemon=True).start()
time.sleep(0.3)

# fresh env
shutil.rmtree(E2E, ignore_errors=True)
os.makedirs(E2E)
sub = os.path.join(E2E, "sophon.sub")
with open(sub, "w", encoding="utf-8") as f:
    f.write("http://127.0.0.1:%d/v1 mock-key\n" % PORT)

env = dict(os.environ, SOPHON_DIR=E2E)
# prompt asks for api discovery; mock will emit the append mark
p = subprocess.run([EXE, "ask", "请找一个可用的远程API并添加到清单"],
                   env=env, capture_output=True, text=True, encoding="utf-8", errors="replace", timeout=60)
print("exit:", p.returncode)
print("--- stdout ---")
print(p.stdout)
print("--- stderr ---")
print(p.stderr)
print("--- sophon.sub after ---")
print(open(sub, encoding="utf-8").read())

ok = True
if "候选确定" not in p.stdout:
    print("FAIL: 流式正文没有实时打到 stdout"); ok = False
if "已验证并追加" not in p.stderr:
    print("FAIL: 没有验证并追加"); ok = False
content = open(sub, encoding="utf-8").read()
if ("http://127.0.0.1:%d/v1 mock-key mock-m-2.5" % PORT) not in content:
    print("FAIL: sophon.sub 里没有新条目"); ok = False
if ("http://127.0.0.1:%d/v1 mock-key\n" % PORT) not in content:
    print("FAIL: 原有内容丢失"); ok = False
# run again -> should report 已存在
p2 = subprocess.run([EXE, "ask", "再添加一次"], env=env, capture_output=True, text=True,
                    encoding="utf-8", errors="replace", timeout=60)
if "已存在，跳过" not in p2.stderr:
    print("FAIL: 第二次应报已存在"); print(p2.stderr); ok = False
print("E2E", "PASS" if ok else "FAIL")
sys.exit(0 if ok else 1)
