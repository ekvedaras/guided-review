import json, sys, threading

script = json.load(open(sys.argv[1]))
# A second argument names the server, which then logs to a file of its own.
log = open(script["log"] + ("." + sys.argv[2] if len(sys.argv) > 2 else ""), "a")
lock = threading.Lock()
modified = set()


def read():
    headers = {}
    while True:
        line = sys.stdin.buffer.readline()
        if not line:
            return None
        line = line.strip()
        if not line:
            break
        key, value = line.decode().split(":", 1)
        headers[key.lower()] = value.strip()
    return json.loads(sys.stdin.buffer.read(int(headers["content-length"])))


def send(message):
    body = json.dumps(message).encode()
    with lock:
        sys.stdout.buffer.write(b"Content-Length: %d\r\n\r\n" % len(body) + body)
        sys.stdout.buffer.flush()


def progress(kind):
    send({"jsonrpc": "2.0", "method": "$/progress", "params": {"token": "index", "value": {"kind": kind, "title": "Indexing"}}})


while True:
    message = read()
    if message is None:
        break
    log.write(json.dumps(message) + "\n")
    log.flush()
    method = message.get("method")
    if method is None:
        continue
    if method in script.get("crash_on", []):
        sys.exit(1)
    if "id" not in message:
        if method == "initialized" and script.get("indexing_ms"):
            send({"jsonrpc": "2.0", "id": "create", "method": "window/workDoneProgress/create", "params": {"token": "index"}})
            progress("begin")
            threading.Timer(script["indexing_ms"] / 1000, lambda: progress("end")).start()
        if method == "initialized" and script.get("reports_status") and not script.get("busy_ms"):
            send({"jsonrpc": "2.0", "method": "experimental/serverStatus", "params": {"health": "ok", "quiescent": True}})
        if method == "exit":
            break
        continue
    if method in script.get("silent_on", []):
        continue
    if method in script.get("content_modified_once", []) and method not in modified:
        modified.add(method)
        send({"jsonrpc": "2.0", "id": message["id"], "error": {"code": -32801, "message": "content modified"}})
        continue
    if method == "$/guidedReviewBarrier" and script.get("busy_ms"):
        # Like rust-analyzer, whose status comes after the reply to a request sent right after initialized.
        send({"jsonrpc": "2.0", "id": message["id"], "error": {"code": -32601, "message": "unknown request"}})
        threading.Timer(0.1, lambda: send({"jsonrpc": "2.0", "method": "experimental/serverStatus", "params": {"health": "ok", "quiescent": False}})).start()
        threading.Timer(script["busy_ms"] / 1000, lambda: send({"jsonrpc": "2.0", "method": "experimental/serverStatus", "params": {"health": "ok", "quiescent": True}})).start()
        continue
    if method == "initialize":
        capabilities = {}
        if script.get("encoding"):
            capabilities["positionEncoding"] = script["encoding"]
        result = {"capabilities": capabilities}
    elif method == "shutdown":
        result = None
    else:
        result = script.get("answers", {}).get(method)
    send({"jsonrpc": "2.0", "id": message["id"], "result": result})
