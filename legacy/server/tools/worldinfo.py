#!/usr/bin/env python3
"""Lê o level.dat de um mundo e imprime: DataVersion<TAB>versão<TAB>nome do mundo.

Uso: worldinfo.py <pasta-do-mundo | level.dat>
Só stdlib (leitor NBT mínimo). Usado pelo "mc import-world".
"""
import gzip
import os
import struct
import sys


class Reader:
    def __init__(self, data):
        self.data, self.pos = data, 0

    def take(self, n):
        chunk = self.data[self.pos:self.pos + n]
        if len(chunk) != n:
            raise ValueError("level.dat truncado")
        self.pos += n
        return chunk

    def unpack(self, fmt):
        return struct.unpack(">" + fmt, self.take(struct.calcsize(">" + fmt)))[0]

    def string(self):
        return self.take(self.unpack("H")).decode("utf-8", "replace")

    def payload(self, tag):
        if tag == 1: return self.unpack("b")
        if tag == 2: return self.unpack("h")
        if tag == 3: return self.unpack("i")
        if tag == 4: return self.unpack("q")
        if tag == 5: return self.unpack("f")
        if tag == 6: return self.unpack("d")
        if tag == 7: return self.take(self.unpack("i"))
        if tag == 8: return self.string()
        if tag == 9:
            inner, size = self.unpack("b"), self.unpack("i")
            return [self.payload(inner) for _ in range(size)]
        if tag == 10:
            out = {}
            while True:
                t = self.unpack("b")
                if t == 0:
                    return out
                name = self.string()
                out[name] = self.payload(t)
        if tag == 11: return [self.unpack("i") for _ in range(self.unpack("i"))]
        if tag == 12: return [self.unpack("q") for _ in range(self.unpack("i"))]
        raise ValueError(f"tag NBT desconhecida: {tag}")


def read_level(path):
    if os.path.isdir(path):
        path = os.path.join(path, "level.dat")
    with gzip.open(path, "rb") as f:
        r = Reader(f.read())
    if r.unpack("b") != 10:
        raise ValueError("level.dat inválido")
    r.string()
    return r.payload(10).get("Data", {})


def main():
    if len(sys.argv) != 2:
        print(__doc__.strip(), file=sys.stderr)
        return 2
    try:
        data = read_level(sys.argv[1])
    except (OSError, ValueError, struct.error) as e:
        print(f"não consegui ler o level.dat: {e}", file=sys.stderr)
        return 1
    version = data.get("Version", {})
    print(f"{data.get('DataVersion', version.get('Id', 0))}\t{version.get('Name', '?')}\t{data.get('LevelName', '?')}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
