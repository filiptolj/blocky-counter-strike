#!/usr/bin/env python3
"""Converts official CS2 radar images into the blocky map data used by the game.

Usage:  python3 tools/radar_to_map.py <radar_dir> [map ...]

<radar_dir> must contain `<map>_radar_psd.png` (or `<map>.png`) for each map, taken from
https://github.com/MurkyYT/cs2-map-icons (images/radars/). Writes assets/maps/<map>.txt.

How it works: Valve colors radar floors by height along a color gradient (darker = lower).
Each 1 m cell becomes a wall, a box (light gray squares) or a floor whose height is the
position of its color on the gradient. Heights are calibrated with the real height of the
T and CT spawns (from `setpos` positions). Bombsites are found from the orange outlines.

Output format (text):
    name <title>
    m_per_px <meters per radar pixel>
    site <A|B> <x0> <y0> <x1> <y1>       (radar pixels)
    t <x> <y>                            (spawn point, radar pixels)
    ct <x> <y>
    grid
    <rows: '#' wall, 'c' box, otherwise the floor level in 0.25 m steps (see ALPHABET)>
"""
import os
import sys
from PIL import Image

LEVEL_M = 0.25
ALPHABET = "0123456789abdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ"  # no 'c': reserved for boxes
MAX_LEVEL = len(ALPHABET) - 1

# Per-map settings. `gradient`: floor colors from lowest to highest. `box_lum`: neutral grays at
# least this bright are boxes, darker neutral grays that are not on the gradient are walls.
# `overview`: (pos_x, pos_y, units per pixel) from the radar's overview file.
# `t` / `ct`: real spawn positions (x, y, z) in game units. `bombs`: radar icon positions (0..1).
MAPS = {
    "de_dust2": dict(
        title="Dust II",
        gradient=[(44, 76, 85), (64, 85, 87), (80, 89, 78), (91, 102, 73)],
        box_lum=93,
        overview=(-2476, 3239, 4.4),
        t=[(-1181, -754, 120), (-980, -754, 120), (-822, -796, 117), (-657, -756, 120), (-493, -808, 109)],
        ct=[(182, 2439, -121), (258, 2481, -121), (334, 2434, -120), (160, 2370, -120), (351, 2353, -120)],
        bombs={"A": (0.80, 0.16), "B": (0.21, 0.12)},
        # The CT spawn zone is drawn with light stripes over the low floor.
        hatched=[(618, 150, 678, 268)],
    ),
    "de_mirage": dict(
        title="Mirage",
        gradient=[(54, 74, 80), (66, 75, 80), (86, 86, 86), (99, 90, 83), (118, 96, 84)],
        box_lum=105,
        overview=(-3230, 1713, 5.0),
        t=[(1136, 32, -165), (1296, 32, -168), (1136, -64, -165), (1216, -115, -164), (1216, -211, -164)],
        ct=[(-1776, -1800, -264), (-1656, -1800, -267), (-1720, -1896, -268), (-1776, -1976, -264), (-1656, -1976, -268)],
        bombs={"A": (0.54, 0.76), "B": (0.23, 0.28)},
    ),
    "de_inferno": dict(
        title="Inferno",
        gradient=[(27, 59, 61), (44, 56, 64), (53, 60, 64), (62, 65, 67), (69, 69, 69), (79, 79, 79), (92, 92, 92)],
        box_lum=100,
        overview=(-2087, 3870, 4.9),
        t=[(-1587, 441, -64), (-1520, 431, -64), (-1676, 352, -64), (-1657, 420, -64), (-1662, 289, -64)],
        ct=[(2397, 2079, 133), (2457, 2153, 132), (2493, 2090, 133), (2292, 2028, 136), (2353, 1977, 136)],
        bombs={"A": (0.81, 0.69), "B": (0.49, 0.22)},
    ),
    "de_ancient": dict(
        title="Ancient",
        gradient=[(37, 45, 35), (45, 54, 42), (53, 77, 46), (67, 89, 42), (71, 99, 23), (94, 112, 33)],
        box_lum=105,
        overview=(-2953, 2164, 5.0),
        t=[(-520, -2224, -163), (-392, -2224, -163), (-584, -2288, -162), (-456, -2288, -163), (-328, -2288, -163)],
        ct=[(-512, 1696, 27), (-448, 1728, 31), (-352, 1728, 29), (-256, 1728, 26), (-192, 1696, 26)],
        bombs={"A": (0.31, 0.25), "B": (0.80, 0.40)},
    ),
}


class Gradient:
    def __init__(self, stops):
        self.stops = stops
        self.lengths = [dist(a, b) for a, b in zip(stops, stops[1:])]
        self.total = sum(self.lengths)

    def t(self, p, tolerance=14):
        """Position (0..1) of a color along the gradient, or None if it isn't a floor color."""
        best, best_t, start = 1e9, None, 0.0
        for (c0, c1), length in zip(zip(self.stops, self.stops[1:]), self.lengths):
            d = [b - a for a, b in zip(c0, c1)]
            u = sum((p[i] - c0[i]) * d[i] for i in range(3)) / (length * length)
            u = min(max(u, 0.0), 1.0)
            dd = dist(p, [c0[i] + d[i] * u for i in range(3)])
            if dd < best:
                best, best_t = dd, (start + u * length) / self.total
            start += length
        return best_t if best < tolerance else None


def dist(a, b):
    return sum((x - y) ** 2 for x, y in zip(a, b)) ** 0.5


def neutral(p):
    return max(p[:3]) - min(p[:3]) < 6


def to_px(cfg, x, y):
    pos_x, pos_y, scale = cfg["overview"]
    return (x - pos_x) / scale, (pos_y - y) / scale


def find_bomb_zones(im, cfg):
    """Bounding boxes of the orange bombsite outlines, labeled with the radar's bomb icons."""
    w, h = im.size
    orange = set()
    for y in range(h):
        for x in range(w):
            r, g, b, a = im.getpixel((x, y))
            if a > 100 and r - g > 30 and r - b > 45 and b < 80:
                orange.add((x, y))
    clusters, seen = [], set()
    for p in orange:
        if p in seen:
            continue
        stack, comp = [p], []
        seen.add(p)
        while stack:
            x, y = stack.pop()
            comp.append((x, y))
            for dx in (-3, -2, -1, 0, 1, 2, 3):
                for dy in (-3, -2, -1, 0, 1, 2, 3):
                    q = (x + dx, y + dy)
                    if q in orange and q not in seen:
                        seen.add(q)
                        stack.append(q)
        if len(comp) > 40:
            xs, ys = [c[0] for c in comp], [c[1] for c in comp]
            clusters.append((min(xs), min(ys), max(xs), max(ys)))
    zones = {}
    for name, (bx, by) in cfg["bombs"].items():
        bx, by = bx * w, by * h
        box = list(min(clusters, key=lambda c: dist(((c[0] + c[2]) / 2, (c[1] + c[3]) / 2), (bx, by))))
        # Outlines are broken up by white corner marks: merge the nearby pieces.
        grown = True
        while grown:
            grown = False
            for c in clusters:
                near = c[0] <= box[2] + 30 and c[2] >= box[0] - 30 and c[1] <= box[3] + 30 and c[3] >= box[1] - 30
                inside = box[0] <= c[0] and box[1] <= c[1] and box[2] >= c[2] and box[3] >= c[3]
                if near and not inside:
                    box = [min(box[0], c[0]), min(box[1], c[1]), max(box[2], c[2]), max(box[3], c[3])]
                    grown = True
        zones[name] = tuple(box)
    return zones


def convert(name, cfg, src, dst):
    im = Image.open(src).convert("RGBA")
    w, h = im.size
    m_per_px = cfg["overview"][2] * 0.0254
    px_per_m = 1.0 / m_per_px
    grad = Gradient(cfg["gradient"])
    hatched = cfg.get("hatched", [])

    def in_hatched(x, y):
        return any(x0 <= x <= x1 and y0 <= y <= y1 for x0, y0, x1, y1 in hatched)

    # 1. Classify cells and read their gradient position.
    n = int(w / px_per_m) + 2
    cells = []
    for cz in range(n):
        row = []
        for cx in range(n):
            counts = {"out": 0, "wall": 0, "box": 0, "floor": 0}
            ts = []
            x0, y0 = int((cx - 1) * px_per_m), int((cz - 1) * px_per_m)
            x1, y1 = int(cx * px_per_m), int(cz * px_per_m)
            for y in range(max(y0, 0), min(y1, h)):
                for x in range(max(x0, 0), min(x1, w)):
                    p = im.getpixel((x, y))
                    if p[3] < 128:
                        counts["out"] += 1
                        continue
                    t = grad.t(p)
                    if t is not None:
                        counts["floor"] += 1
                        ts.append(t)
                    elif neutral(p) and p[0] >= cfg["box_lum"]:
                        counts["box"] += 1
                    elif neutral(p):
                        counts["wall"] += 1
                    else:
                        counts["floor"] += 1  # outlines / markings drawn on the floor
            total = sum(counts.values()) or 1
            if counts["floor"] / total >= 0.45:
                if ts:
                    ts.sort()
                    pick = ts[len(ts) // 10] if in_hatched((x0 + x1) // 2, (y0 + y1) // 2) else ts[len(ts) // 2]
                    row.append(pick)
                else:
                    row.append(None)
            elif counts["box"] / total >= 0.4:
                row.append("box")
            else:
                row.append("wall")
        cells.append(row)

    # 2. Calibrate: gradient position -> game height, using the real spawn heights.
    def t_near(points):
        vals = []
        for x, y, _ in points:
            px, py = to_px(cfg, x, y)
            cx, cz = int(px / px_per_m) + 1, int(py / px_per_m) + 1
            for dz in range(-2, 3):
                for dx in range(-2, 3):
                    v = cells[cz + dz][cx + dx] if 0 <= cz + dz < n and 0 <= cx + dx < n else None
                    if isinstance(v, float):
                        vals.append(v)
        vals.sort()
        return vals[len(vals) // 2]

    t_t, t_ct = t_near(cfg["t"]), t_near(cfg["ct"])
    z_t = sum(p[2] for p in cfg["t"]) / len(cfg["t"])
    z_ct = sum(p[2] for p in cfg["ct"]) / len(cfg["ct"])
    if abs(t_t - t_ct) > 0.15:
        slope = (z_t - z_ct) / (t_t - t_ct)
    else:
        slope = 260.0  # spawns at similar heights: assume a typical ~6.5 m height range
    # Keep the whole map within the encodable range (about 15 m).
    slope = min(max(slope, 120.0), MAX_LEVEL * LEVEL_M / 0.0254 * 0.95)
    z_of = lambda t: z_t + (t - t_t) * slope
    z_min = min(z_of(0.0), z_of(1.0))
    print(f"{name}: spawn t T={t_t:.2f} CT={t_ct:.2f}, {slope * 0.0254:.1f} m over the gradient")

    levels = []
    for row in cells:
        out = []
        for v in row:
            if isinstance(v, float):
                out.append(min(MAX_LEVEL, round((z_of(v) - z_min) * 0.0254 / LEVEL_M)))
            else:
                out.append(v)
        levels.append(out)

    # 3. Clean up.
    zones = find_bomb_zones(im, cfg)
    cleanup(levels, n)
    flatten_zones(levels, n, zones.values(), px_per_m)
    limit_slopes(levels, n)

    with open(dst, "w") as f:
        f.write(f"name {cfg['title']}\n")
        f.write(f"m_per_px {m_per_px:.6f}\n")
        for site, (x0, y0, x1, y1) in sorted(zones.items()):
            f.write(f"site {site} {x0} {y0} {x1} {y1}\n")
        for side in ("t", "ct"):
            for x, y, _ in cfg[side]:
                px, py = to_px(cfg, x, y)
                f.write(f"{side} {px:.1f} {py:.1f}\n")
        f.write("grid\n")
        for row in levels:
            f.write("".join("#" if v in ("wall", None) else "c" if v == "box" else ALPHABET[v] for v in row) + "\n")
    print(f"{name}: wrote {n}x{n} cells to {dst}, sites {sorted(zones)}")


def cleanup(levels, n):
    """Fills floor cells without height info from their neighbours and removes height spikes."""
    def floor_neighbours(x, z):
        out = []
        for dz in (-1, 0, 1):
            for dx in (-1, 0, 1):
                if (dx or dz) and 0 <= x + dx < n and 0 <= z + dz < n and isinstance(levels[z + dz][x + dx], int):
                    out.append(levels[z + dz][x + dx])
        return out

    for _ in range(6):
        for z in range(n):
            for x in range(n):
                if levels[z][x] is None:
                    nb = sorted(floor_neighbours(x, z))
                    if nb:
                        levels[z][x] = nb[len(nb) // 2]
    for _ in range(2):
        for z in range(n):
            for x in range(n):
                v = levels[z][x]
                if not isinstance(v, int):
                    continue
                nb = sorted(floor_neighbours(x, z))
                if len(nb) >= 5:
                    med = nb[len(nb) // 2]
                    agree = sum(1 for a in nb if abs(a - med) <= 1)
                    if abs(v - med) > 3 and agree >= len(nb) * 0.7:
                        levels[z][x] = med

    # 1-cell wide ridges/trenches (outline edges): compare with the cells two steps away.
    def lv(x, z):
        return levels[z][x] if 0 <= x < n and 0 <= z < n and isinstance(levels[z][x], int) else None

    for _ in range(2):
        for z in range(n):
            for x in range(n):
                v = lv(x, z)
                if v is None:
                    continue
                for dx, dz in ((1, 0), (0, 1)):
                    a, b = lv(x - dx, z - dz), lv(x + dx, z + dz)
                    a2, b2 = lv(x - 2 * dx, z - 2 * dz), lv(x + 2 * dx, z + 2 * dz)
                    if a is not None and b is not None:
                        if abs(a - b) <= 2 and abs(v - a) > 3 and abs(v - b) > 3:
                            levels[z][x] = (a + b) // 2
                    elif b is not None and b2 is not None and abs(b - b2) <= 1 and abs(v - b) > 3:
                        levels[z][x] = b  # strip along a wall
                    elif a is not None and a2 is not None and abs(a - a2) <= 1 and abs(v - a) > 3:
                        levels[z][x] = a


def flatten_zones(levels, n, zones, px_per_m):
    """Bombsite outlines leave a faint halo; floors inside them are flattened."""
    for (x0, y0, x1, y1) in zones:
        cells = []
        for z in range(n):
            for x in range(n):
                cx, cy = (x - 0.5) * px_per_m, (z - 0.5) * px_per_m
                if x0 - 4 <= cx <= x1 + 4 and y0 - 4 <= cy <= y1 + 4 and isinstance(levels[z][x], int):
                    cells.append((x, z))
        if not cells:
            continue
        values = sorted(levels[z][x] for x, z in cells)
        med = values[len(values) // 2]
        for x, z in cells:
            if abs(levels[z][x] - med) <= 6:
                levels[z][x] = med


def limit_slopes(levels, n, max_step=2):
    """Radar stairs are flat color blocks, so heights jump at the edge of a staircase.
    Real ledges have an outline (which became a wall), so any remaining floor-to-floor jump
    is a staircase: spread it into steps of at most `max_step` levels (0.5 m) per cell."""
    for _ in range(300):
        changed = False
        new = [row[:] for row in levels]
        for z in range(n):
            for x in range(n):
                a = levels[z][x]
                if not isinstance(a, int):
                    continue
                for dx, dz in ((1, 0), (0, 1)):
                    if not (0 <= x + dx < n and 0 <= z + dz < n):
                        continue
                    b = levels[z + dz][x + dx]
                    if not isinstance(b, int) or abs(a - b) <= max_step:
                        continue
                    lo, hi = ((x, z), (x + dx, z + dz)) if a < b else ((x + dx, z + dz), (x, z))
                    new[hi[1]][hi[0]] = min(new[hi[1]][hi[0]], levels[hi[1]][hi[0]] - 1)
                    new[lo[1]][lo[0]] = max(new[lo[1]][lo[0]], levels[lo[1]][lo[0]] + 1)
                    changed = True
        levels[:] = new
        if not changed:
            return


def main():
    radar_dir = sys.argv[1]
    names = sys.argv[2:] or list(MAPS)
    out_dir = os.path.join(os.path.dirname(__file__), "..", "assets", "maps")
    os.makedirs(out_dir, exist_ok=True)
    for name in names:
        src = os.path.join(radar_dir, f"{name}_radar_psd.png")
        if not os.path.exists(src):
            src = os.path.join(radar_dir, f"{name}.png")
        convert(name, MAPS[name], src, os.path.join(out_dir, f"{name}.txt"))


if __name__ == "__main__":
    main()
