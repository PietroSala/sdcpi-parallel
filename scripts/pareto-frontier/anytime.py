#!/usr/bin/env python3
"""Il buffer anytime di una corsa: leggerlo, troncarlo, potarlo.

Il binario, con `--anytime F`, appende a F una riga JSON per ogni apice che
entra nel record condiviso, con il secondo in cui e' entrato contato
dall'avvio del processo, e la svuota subito: una corsa uccisa in qualunque
istante lascia su disco tutto quello che aveva trovato. L'ultima riga dice
che la ricerca e' finita da sola, e la sua assenza dice che e' stata uccisa.

La frontiera a un istante si ottiene troncando il buffer a quell'istante e
potando gli apici con la stessa regola del binario (`minimal_apexes`): un
apice e' ridondante quando un altro e' altrettanto stretto su ogni componente
tenuta, confrontando i tick della griglia e non i valori, e a tick uguali
sopravvive quello arrivato prima.
"""
from __future__ import annotations

import gzip
import json
from pathlib import Path


def read(path: Path) -> tuple[list[dict], int]:
    """Le righe valide del buffer e quante ne sono state scartate.

    Una uccisione puo' tagliare a meta' la riga in scrittura: quella si perde,
    e nient'altro."""
    events, dropped = [], 0
    opener = gzip.open if str(path).endswith(".gz") else open
    with opener(path, "rt") as fh:
        for line in fh:
            line = line.strip()
            if not line:
                continue
            try:
                events.append(json.loads(line))
            except json.JSONDecodeError:
                dropped += 1
    return events, dropped


def minimal(apexes: list[tuple[int, list[int]]], components: list[int]) -> list[int]:
    """Gli indici degli apici minimali, la regola del binario."""
    first: dict[tuple, int] = {}
    for n, ticks in apexes:
        key = tuple(ticks[c] for c in components)
        if key not in first or n < first[key]:
            first[key] = n
    live: list[tuple] = []
    for key in sorted(first, reverse=True):          # i piu' stretti per primi
        if any(all(w >= x for w, x in zip(m, key)) for m in live):
            continue
        live = [m for m in live if not all(x >= w for x, w in zip(key, m))]
        live.append(key)
    return sorted(first[k] for k in live)


class Buffer:
    def __init__(self, path: Path) -> None:
        self.events, self.dropped = read(path)
        self.start = next((e for e in self.events if e.get("start")), None)
        self.search = next((e for e in self.events if e.get("search")), None)
        self.end = next((e for e in self.events if e.get("end")), None)
        self.final = next((e for e in self.events if e.get("minimal")), None)
        self.stats = [e for e in self.events if "stat" in e]
        self.points = [e for e in self.events if "n" in e]
        self.components = (self.search or {}).get("components")
        if self.components is None and self.points:
            self.components = list(range(len(self.points[0]["k"])))

    def apexes_by(self, t: float) -> list[dict]:
        return [e for e in self.points if e["t"] <= t]

    def frontier_at(self, t: float) -> list[dict]:
        """I punti della frontiera all'istante `t`, come li avrebbe scritti il
        binario se fosse finito in quel momento."""
        seen = self.apexes_by(t)
        if not seen:
            return []
        live = set(minimal([(e["n"], e["k"]) for e in seen], self.components or []))
        return [e for e in seen if e["n"] in live]

    def curve(self, t: float, steps: int = 24) -> list[tuple[float, int, int]]:
        """(istante, apici, punti) a passi regolari fino a `t`: la curva anytime."""
        seen = self.apexes_by(t)
        if not seen:
            return []
        out, step = [], max(1, len(seen) // steps)
        for i in range(step, len(seen) + 1, step):
            sub = seen[:i]
            live = minimal([(e["n"], e["k"]) for e in sub], self.components or [])
            out.append((sub[-1]["t"], len(sub), len(live)))
        return out

    def report(self, t: float, curve: bool = True) -> dict:
        """Quello che il driver mette nel record della corsa."""
        front = self.frontier_at(t)
        complete = self.end is not None and self.end["t"] <= t
        r = {
            "status": "complete" if complete else "truncated",
            "cut_at": t,
            "points_found": len(front),
            "apexes": len(self.apexes_by(t)),
            "points": [p["v"] for p in front],
            "dropped_lines": self.dropped,
        }
        if self.end is not None:
            r["end_seconds"] = self.end["t"]
            r["end_status"] = self.end["status"]
            r["search_seconds"] = self.end["search_seconds"]
            r["apexes_total"] = self.end["apexes"]
        if self.final is not None:
            r["points_total"] = self.final["points"]
        if self.stats:
            last = [s for s in self.stats if s["t"] <= t]
            if last:
                r["stat"] = last[-1]["stat"]
                r["stat_seconds"] = last[-1]["t"]
        if curve:
            r["curve"] = [[round(a, 6), b, c] for a, b, c in self.curve(t)]
        return r


def main() -> int:
    import argparse
    p = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    p.add_argument("buffer", type=Path)
    p.add_argument("--at", type=float, default=120.0, help="l'istante del taglio")
    p.add_argument("--curve", action="store_true")
    a = p.parse_args()
    b = Buffer(a.buffer)
    r = b.report(a.at, curve=a.curve)
    print(f"eventi {len(b.events)}, righe scartate {b.dropped}")
    print(f"apici entro {a.at} s: {r['apexes']}   frontiera: {r['points_found']} punti")
    if b.end:
        print(f"fine ricerca a {b.end['t']:.3f} s, stato {b.end['status']}, "
              f"{b.end['apexes']} apici, ricerca {b.end['search_seconds']:.3f} s")
    else:
        print("fine ricerca assente: il processo e' stato ucciso")
    if b.final:
        ok = "coincide" if b.final["points"] == r["points_found"] else "DIVERGE"
        print(f"potatura del binario {b.final['points']} punti, la rilettura {ok}")
    if a.curve:
        for t, ap, pt in r["curve"]:
            print(f"  {t:9.3f} s {ap:8} apici {pt:8} punti")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
