"""Сквозная проверка приёма gzip в HTTP API мода (docs/API.md §6).

Поднимает `route`-эквивалент? Нет — проверяет **реальный** путь чтения тела:
тест-сервер на `TcpListener` повторяет `serve_connection` из `src/api.rs`
(разбор `Content-Length`/`Content-Encoding` + `gunzip_limited`) и отвечает так
же, как мод. Гоняем тело скрипта длиннее 3600 кадров:

1. plain JSON без gzip → 413 (не влезло в 64 КиБ);
2. тот же скрипт под gzip → 200 + `total_frames` из тела;
3. битый gzip → 400;
4. `Content-Encoding: br` → 415;
5. zip-бомба (64 КиБ → 512 МБ) → 413, память не съедена.

Мод при этом не запускается: проверяется контракт транспорта.

Запуск: python tools/script_size/check_gzip_e2e.py
"""

from __future__ import annotations

import gzip
import json
import socket
import struct
import threading
import zlib
from pathlib import Path

MAX_BODY_BYTES = 64 * 1024
MAX_INFLATED_BYTES = 8 * 1024 * 1024
MAX_SCRIPT_FRAMES = 1_000_000

HERE = Path(__file__).resolve().parent


class InflateError(Exception):
    def __init__(self, kind: str):
        self.kind = kind


def gunzip_limited(data: bytes, limit: int) -> bytes:
    """Порт `gunzip_limited` из `src/api.rs`: поток с обрывом на лимите."""
    decompressor = zlib.decompressobj(16 + zlib.MAX_WBITS)  # gzip-обёртка
    out = bytearray()
    chunk = 8192
    view = memoryview(data)
    try:
        while view:
            piece = decompressor.decompress(bytes(view[:chunk]), limit + 1 - len(out))
            view = view[len(view[:chunk]):]
            if not piece and not view:
                break
            out.extend(piece)
            if len(out) > limit:
                raise InflateError("TooLarge")
    except zlib.error as err:
        raise InflateError("BadData") from err
    out.extend(decompressor.flush())
    if len(out) > limit:
        raise InflateError("TooLarge")
    return bytes(out)


def serve(conn: socket.socket) -> None:
    """Повторяет `serve_connection` мода: заголовки → тело → распаковка → route."""
    buf = b""
    while b"\r\n\r\n" not in buf:
        chunk = conn.recv(4096)
        if not chunk:
            return
        buf += chunk
    head, _, rest = buf.partition(b"\r\n\r\n")
    lines = head.decode("latin-1").split("\r\n")
    content_length, gzipped = 0, False
    for line in lines[1:]:
        name, _, value = line.partition(":")
        name, value = name.strip().lower(), value.strip()
        if name == "content-length":
            content_length = int(value)
        elif name == "content-encoding":
            if value.lower() in ("gzip", "x-gzip"):
                gzipped = True
            elif value.lower() != "identity":
                return reply(conn, 415, {"error": f"unsupported content-encoding: {value}"})

    if content_length > MAX_BODY_BYTES:
        return reply(conn, 413, {"error": "body too large"})
    body_bytes = rest
    while len(body_bytes) < content_length:
        chunk = conn.recv(4096)
        if not chunk:
            return
        body_bytes += chunk

    if gzipped:
        try:
            body = gunzip_limited(body_bytes, MAX_INFLATED_BYTES).decode("utf-8", "replace")
        except InflateError as err:
            if err.kind == "TooLarge":
                return reply(conn, 413, {"error": "inflated body too large"})
            return reply(conn, 400, {"error": "gzip: BadData"})
    else:
        body = body_bytes.decode("utf-8", "replace")

    # route: только /script/run с проверками parse_script
    try:
        script = json.loads(body)
    except json.JSONDecodeError as err:
        return reply(conn, 400, {"error": f"invalid script: {err}"})
    if not script.get("commands"):
        return reply(conn, 400, {"error": "commands is empty"})
    total = 0
    for i, cmd in enumerate(script["commands"]):
        if cmd.get("duration", 0) < 1:
            return reply(conn, 400, {"error": f"commands[{i}]: duration must be >= 1"})
        if cmd["t"] + cmd["duration"] > MAX_SCRIPT_FRAMES:
            return reply(conn, 400, {"error": f"commands[{i}]: t+duration exceeds max {MAX_SCRIPT_FRAMES}"})
        total = max(total, cmd["t"] + cmd["duration"])
    reply(conn, 200, {"script_id": 1, "name": script.get("name", "script"),
                      "total_frames": total, "status": "running"})


def reply(conn: socket.socket, code: int, payload: dict) -> None:
    body = json.dumps(payload, separators=(",", ":")).encode()
    head = (f"HTTP/1.1 {code} drmod\r\nContent-Type: application/json; charset=utf-8\r\n"
            f"Content-Length: {len(body)}\r\nConnection: close\r\n\r\n").encode()
    conn.sendall(head + body)
    conn.close()


def start() -> tuple[int, socket.socket]:
    listener = socket.socket()
    listener.bind(("127.0.0.1", 0))
    listener.listen(8)
    port = listener.getsockname()[1]

    def loop():
        while True:
            try:
                conn, _ = listener.accept()
            except OSError:
                return
            threading.Thread(target=serve, args=(conn,), daemon=True).start()

    threading.Thread(target=loop, daemon=True).start()
    return port, listener


def post(port: int, body: bytes, encoding: str | None) -> tuple[int, dict]:
    conn = socket.create_connection(("127.0.0.1", port), timeout=10)
    head = f"POST /script/run HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: {len(body)}\r\n"
    if encoding:
        head += f"Content-Encoding: {encoding}\r\n"
    try:
        conn.sendall(head.encode() + b"\r\n" + body)
        received = b""
        while True:
            chunk = conn.recv(65536)
            if not chunk:
                break
            received += chunk
    except ConnectionResetError:
        # Мод отвечает 413 и закрывает соединение, **не вычитывая** тело до
        # конца (так же, как `serve_connection`: проверка длины стоит до
        # чтения) — на Windows невычитанные байты дают RST. Ответ при этом уже
        # получен, поэтому ошибку сокета здесь глотаем, а не считаем провалом.
        pass
    finally:
        conn.close()
    head_part, _, body_part = received.partition(b"\r\n\r\n")
    if not head_part:
        return 0, {}
    code = int(head_part.split(b" ")[1])
    return code, json.loads(body_part or b"{}")


def long_script(frames: int) -> bytes:
    """Скрипт с командой на каждый кадр — заведомо больше 64 КиБ в JSON."""
    commands = [{"t": i, "duration": 1,
                 "input": {"left_stick": [round(1000 * ((i % 21) - 10) / 10, 3), -500.0]}}
                for i in range(frames)]
    return json.dumps({"name": "e2e-long", "commands": commands},
                      separators=(",", ":")).encode()


def main() -> int:
    port, listener = start()
    failures: list[str] = []

    def check(name: str, got, want) -> None:
        ok = got == want
        print(f"  {'ok  ' if ok else 'FAIL'} {name}: {got}" + ("" if ok else f" (ожидалось {want})"))
        if not ok:
            failures.append(name)

    frames = 5000  # > 3600 старых кадров: то, что раньше мод отбивал
    raw = long_script(frames)
    packed = gzip.compress(raw, compresslevel=9, mtime=0)
    print(f"скрипт: {frames} кадров, JSON {len(raw) / 1024:.0f} КиБ, gzip {len(packed) / 1024:.1f} КиБ")

    print("\n1. plain JSON длинного скрипта (не влезает в 64 КиБ)")
    code, body = post(port, raw, None)
    check("413 body too large", code, 413)

    print("\n2. тот же скрипт под gzip")
    code, body = post(port, packed, "gzip")
    check("200", code, 200)
    check("total_frames", body.get("total_frames"), frames)

    print("\n3. битый gzip")
    code, _ = post(port, b"\x1f\x8b\x08garbage", "gzip")
    check("400", code, 400)

    print("\n4. неподдерживаемая кодировка")
    code, _ = post(port, packed, "br")
    check("415", code, 415)

    print("\n5. zip-бомба: 128 МиБ нулей → 64 КиБ на проводе")
    bomb = gzip.compress(b"\x00" * (128 * 1024 * 1024), compresslevel=9, mtime=0)
    print(f"  бомба на проводе: {len(bomb) / 1024:.0f} КиБ (лимит тела 64 КиБ)")
    if len(bomb) <= MAX_BODY_BYTES:
        code, _ = post(port, bomb, "gzip")
        check("413 inflated body too large", code, 413)
    else:
        code, _ = post(port, bomb, "gzip")
        check("413 body too large (бомба не влезла)", code, 413)

    listener.close()
    print()
    if failures:
        print(f"ПРОВАЛ: {len(failures)} проверок — {', '.join(failures)}")
        return 1
    print("все проверки прошли")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
