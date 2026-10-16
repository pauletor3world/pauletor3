"""Jogador robô para os testes de ponta a ponta (protocolo 774, Minecraft 1.21.11, offline).

Entra no mundo pelo proxy local, responde o que o servidor pede (keep-alive, teleporte,
chunks) e obedece ao /transfer, como o jogo de verdade. Também responde às consultas da
sala de espera do proxy. Escreve no stdout uma linha por acontecimento, com o tempo em ms:

    python scripts/robo-jogador.py --porta 25570 --nick Bia --segundos 600
"""
import argparse
import io
import socket
import struct
import sys
import time
import uuid
import zlib

T0 = time.time()


def log(msg):
    print(f"t={int((time.time() - T0) * 1000)} {msg}", flush=True)


def varint(v):
    v &= 0xFFFFFFFF
    out = bytearray()
    while True:
        b = v & 0x7F
        v >>= 7
        if v:
            out.append(b | 0x80)
        else:
            out.append(b)
            return bytes(out)


def read_varint_sock(s):
    val = 0
    for i in range(5):
        b = s.recv(1)
        if not b:
            raise EOFError("conexão fechada")
        val |= (b[0] & 0x7F) << (7 * i)
        if not b[0] & 0x80:
            return val
    raise ValueError("varint longo")


def read_varint(buf):
    val = 0
    for i in range(5):
        b = buf.read(1)[0]
        val |= (b & 0x7F) << (7 * i)
        if not b & 0x80:
            return val
    raise ValueError("varint longo")


def string(s):
    b = s.encode()
    return varint(len(b)) + b


def read_exact(s, n):
    out = bytearray()
    while len(out) < n:
        c = s.recv(n - len(out))
        if not c:
            raise EOFError("conexão fechada")
        out += c
    return bytes(out)


class Conn:
    def __init__(self, host, port):
        self.s = socket.create_connection((host, port), timeout=60)
        self.threshold = -1

    def send(self, pid, body=b""):
        data = varint(pid) + body
        if self.threshold >= 0:
            if len(data) >= self.threshold:
                comp = zlib.compress(data)
                data = varint(len(data)) + comp
            else:
                data = varint(0) + data
        self.s.sendall(varint(len(data)) + data)

    def recv(self):
        n = read_varint_sock(self.s)
        data = read_exact(self.s, n)
        buf = io.BytesIO(data)
        if self.threshold >= 0:
            dlen = read_varint(buf)
            rest = buf.read()
            buf = io.BytesIO(zlib.decompress(rest) if dlen else rest)
        pid = read_varint(buf)
        return pid, buf


def session(host, port, nick, intent, deadline):
    """Uma conexão. Devolve (host, porta) de um /transfer, ou None quando acabar."""
    c = Conn(host, port)
    c.send(0x00, varint(774) + string(host) + struct.pack(">H", port) + varint(intent))
    c.send(0x00, string(nick) + uuid.uuid3(uuid.NAMESPACE_OID, nick).bytes)
    state = "login"
    holds = 0
    played = False
    while time.time() < deadline:
        try:
            pid, b = c.recv()
        except socket.timeout:
            log("ERRO: 60 s sem nada do servidor")
            return None
        if state == "login":
            if pid == 0x04:  # consulta (sala de espera do proxy)
                qid = read_varint(b)
                holds += 1
                if holds == 1:
                    log("sala de espera: Entrando...")
                c.send(0x02, varint(qid) + b"\x00")
            elif pid == 0x03:
                c.threshold = read_varint(b)
            elif pid == 0x02:
                if holds:
                    log(f"saiu da sala de espera depois de {holds} consultas")
                c.send(0x03)
                state = "config"
            elif pid == 0x00:
                log(f"recusado no login: {b.read().decode(errors='replace')}")
                return None
            elif pid == 0x01:
                log("ERRO: servidor pediu criptografia (online-mode)")
                return None
        elif state == "config":
            if pid == 0x0E:
                c.send(0x07, varint(0))
            elif pid == 0x04:
                c.send(0x04, b.read(8))
            elif pid == 0x05:
                c.send(0x05, b.read(4))
            elif pid == 0x03:
                c.send(0x03)
                state = "play"
            elif pid == 0x0B:
                h = b.read(read_varint(b)).decode()
                p = read_varint(b)
                log(f"transfer para {h}:{p}")
                return (h, p)
            elif pid == 0x02:
                log("desconectado na configuração")
                return None
        else:
            if pid == 43:
                c.send(27, b.read(8))
            elif pid == 59:
                c.send(44, b.read(4))
            elif pid == 70:
                tid = read_varint(b)
                c.send(0, varint(tid))
                if not played:
                    played = True
                    c.send(43)
                    log(f"JOGANDO como {nick}")
            elif pid == 11:
                c.send(10, struct.pack(">f", 20.0))
            elif pid == 116:
                c.send(15)
                state = "config"
            elif pid == 127:
                h = b.read(read_varint(b)).decode()
                p = read_varint(b)
                log(f"transfer para {h}:{p}")
                return (h, p)
            elif pid == 32:
                log("DESCONECTADO pelo servidor")
                return None
    return None


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--porta", type=int, default=25565)
    ap.add_argument("--nick", default="Bia")
    ap.add_argument("--segundos", type=int, default=600)
    a = ap.parse_args()
    deadline = time.time() + a.segundos
    target, intent = ("127.0.0.1", a.porta), 2
    while time.time() < deadline:
        try:
            nxt = session(target[0], target[1], a.nick, intent, deadline)
        except (EOFError, ConnectionError, OSError) as e:
            log(f"conexão caiu: {e}")
            nxt = None
        if nxt is None:
            # como um jogador: espera um pouco e entra de novo
            time.sleep(3)
            target, intent = ("127.0.0.1", a.porta), 2
        else:
            target, intent = nxt, 3
    log("fim")


if __name__ == "__main__":
    sys.exit(main())
