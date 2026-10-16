#!/usr/bin/env python3
"""Cliente RCON mínimo para o Minecraft (só stdlib).

Uso: rcon.py "comando"        ex.: rcon.py "list"
Lê a senha de /opt/minecraft/rcon.secret e conecta em 127.0.0.1:25575.
Variáveis opcionais: RCON_HOST, RCON_PORT, RCON_SECRET_FILE.
"""
import os
import re
import socket
import struct
import sys

HOST = os.environ.get("RCON_HOST", "127.0.0.1")
PORT = int(os.environ.get("RCON_PORT", "25575"))
SECRET_FILE = os.environ.get("RCON_SECRET_FILE", "/opt/minecraft/rcon.secret")

TYPE_AUTH = 3
TYPE_COMMAND = 2
MAX_PAYLOAD = 1446    # maior payload que o servidor aceita
FRAGMENT_SIZE = 4096  # respostas maiores chegam em vários pacotes


class AuthError(Exception):
    pass


def _recv_exact(sock, n):
    buf = b""
    while len(buf) < n:
        chunk = sock.recv(n - len(buf))
        if not chunk:
            raise ConnectionError("conexão RCON fechada pelo servidor")
        buf += chunk
    return buf


def send_packet(sock, req_id, ptype, payload):
    body = payload.encode("utf-8")
    if len(body) > MAX_PAYLOAD:
        raise ValueError("comando longo demais para RCON")
    data = struct.pack("<ii", req_id, ptype) + body + b"\x00\x00"
    sock.sendall(struct.pack("<i", len(data)) + data)


def read_packet(sock):
    (length,) = struct.unpack("<i", _recv_exact(sock, 4))
    if not 10 <= length <= 1 << 20:
        raise ConnectionError("pacote RCON inválido")
    data = _recv_exact(sock, length)
    req_id, ptype = struct.unpack("<ii", data[:8])
    return req_id, ptype, data[8:-2]


def run(command, timeout=10.0):
    with open(SECRET_FILE, encoding="utf-8") as f:
        password = f.read().strip()

    with socket.create_connection((HOST, PORT), timeout=timeout) as sock:
        send_packet(sock, 1, TYPE_AUTH, password)
        req_id, _, _ = read_packet(sock)
        if req_id == -1:
            raise AuthError("senha RCON recusada")

        # O servidor só processa um pacote por leitura, então nada de pipelining.
        send_packet(sock, 2, TYPE_COMMAND, command)
        parts = []
        try:
            _, _, raw = read_packet(sock)
            parts.append(raw)
            sock.settimeout(0.5)
            while len(raw) >= FRAGMENT_SIZE:
                _, _, raw = read_packet(sock)
                parts.append(raw)
        except socket.timeout:
            pass
        except ConnectionError:
            # "stop" pode fechar a conexão antes de responder
            if (command.split() or [""])[0].lower() != "stop":
                raise

    text = b"".join(parts).decode("utf-8", "replace")
    return re.sub("§.", "", text)  # remove códigos de cor (§x)


def main():
    if len(sys.argv) < 2:
        print('uso: rcon.py "comando"', file=sys.stderr)
        return 2
    try:
        out = run(" ".join(sys.argv[1:]))
    except FileNotFoundError:
        print(f"senha não encontrada em {SECRET_FILE}", file=sys.stderr)
        return 1
    except PermissionError:
        print(f"sem permissão para ler {SECRET_FILE} (use sudo)", file=sys.stderr)
        return 1
    except AuthError as e:
        print(str(e), file=sys.stderr)
        return 1
    except (ConnectionRefusedError, socket.timeout):
        print(f"RCON indisponível em {HOST}:{PORT} (servidor offline ou iniciando)", file=sys.stderr)
        return 1
    except (OSError, ValueError) as e:
        print(f"erro RCON: {e}", file=sys.stderr)
        return 1
    if out:
        print(out.rstrip("\n"))
    return 0


if __name__ == "__main__":
    sys.exit(main())
