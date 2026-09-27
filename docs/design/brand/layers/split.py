#!/usr/bin/env python3
"""Splits the Overseer mark into three layers for animation (Voice Mode, AC-177).

    python3 docs/design/brand/layers/split.py

Reads ../overseer-logo.png (the owner's transparent mark, never changed) and writes, on the same
canvas so they stack without offsets:

    overseer-logo-core.png      the dark core with its rim and glow, as a whole disc
    overseer-logo-swooshes.png  the three swooshes, as one ring
    overseer-logo-star.png      the star and its light

Stacked in that order (core, swooshes, star) they give back the original; the script prints the
difference. Standard library only. The three swooshes overlap each other and are not separated:
the parts of a swoosh hidden behind another do not exist in the image."""
import math, pathlib, struct, zlib

HERE = pathlib.Path(__file__).resolve().parent
SOURCE = HERE.parent / "overseer-logo.png"


def read_png(path):
    data = path.read_bytes()
    assert data[:8] == b"\x89PNG\r\n\x1a\n"
    pos, idat, w, h = 8, [], 0, 0
    while pos < len(data):
        n, kind = struct.unpack(">I4s", data[pos:pos + 8])
        body = data[pos + 8:pos + 8 + n]
        if kind == b"IHDR":
            w, h, depth, colour, _, _, interlace = struct.unpack(">IIBBBBB", body)
            assert (depth, colour, interlace) == (8, 6, 0), "8-bit RGBA, non-interlaced only"
        elif kind == b"IDAT":
            idat.append(body)
        pos += 12 + n
    raw = zlib.decompress(b"".join(idat))
    stride = w * 4
    out, prev, p = bytearray(h * stride), bytearray(stride), 0
    for y in range(h):
        f = raw[p]
        line = bytearray(raw[p + 1:p + 1 + stride])
        p += 1 + stride
        if f == 1:
            for i in range(4, stride):
                line[i] = (line[i] + line[i - 4]) & 255
        elif f == 2:
            for i in range(stride):
                line[i] = (line[i] + prev[i]) & 255
        elif f == 3:
            for i in range(stride):
                line[i] = (line[i] + (((line[i - 4] if i >= 4 else 0) + prev[i]) >> 1)) & 255
        elif f == 4:
            for i in range(stride):
                a = line[i - 4] if i >= 4 else 0
                b = prev[i]
                c = prev[i - 4] if i >= 4 else 0
                pa, pb, pc = abs(b - c), abs(a - c), abs(a + b - 2 * c)
                line[i] = (line[i] + (a if (pa <= pb and pa <= pc) else (b if pb <= pc else c))) & 255
        out[y * stride:(y + 1) * stride] = line
        prev = line
    return w, h, out


def write_png(path, w, h, px):
    stride, raw = w * 4, bytearray()
    for y in range(h):
        raw.append(0)
        raw += px[y * stride:(y + 1) * stride]

    def chunk(kind, body):
        return struct.pack(">I", len(body)) + kind + body + struct.pack(">I", zlib.crc32(kind + body) & 0xffffffff)
    path.write_bytes(b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 6, 0, 0, 0))
                     + chunk(b"IDAT", zlib.compress(bytes(raw), 9)) + chunk(b"IEND", b""))


def fit_circle(points):
    n = len(points)
    sx = sum(x for x, _ in points); sy = sum(y for _, y in points)
    sxx = sum(x * x for x, _ in points); syy = sum(y * y for _, y in points); sxy = sum(x * y for x, y in points)
    sz = sum(x * x + y * y for x, y in points)
    sxz = sum(x * (x * x + y * y) for x, y in points); syz = sum(y * (x * x + y * y) for x, y in points)
    m = [[sxx, sxy, sx, sxz], [sxy, syy, sy, syz], [sx, sy, n, sz]]
    for i in range(3):
        piv = m[i][i]
        m[i] = [v / piv for v in m[i]]
        for j in range(3):
            if j != i:
                f = m[j][i]
                m[j] = [a - f * b for a, b in zip(m[j], m[i])]
    cx, cy = m[0][3] / 2, m[1][3] / 2
    return cx, cy, math.sqrt(m[2][3] + cx * cx + cy * cy)


def median(values):
    s = sorted(values)
    return s[len(s) // 2]


def main():
    w, h, px = read_png(SOURCE)

    def at(x, y):
        i = (y * w + x) * 4
        return px[i], px[i + 1], px[i + 2], px[i + 3]

    def lum(x, y):
        r, g, b, a = at(x, y)
        return (0.2126 * r + 0.7152 * g + 0.0722 * b) * a / 255

    def polar(cx, cy, r, t):
        return int(round(cx + r * math.cos(t))), int(round(cy + r * math.sin(t)))

    # 1. The star: the centroid of the near-white pixels in the middle.
    white = [(x, y) for y in range(h // 2 - 120, h // 2 + 120) for x in range(w // 2 - 120, w // 2 + 120) if lum(x, y) > 235]
    sx, sy = sum(x for x, _ in white) / len(white), sum(y for _, y in white) / len(white)

    # 2. The core's outline. Its rim is the first bright thing met walking outward from the middle,
    #    except where a swoosh passes in front of it. The core is close to a circle but not one,
    #    so the rim's radius is fitted as a smooth curve over the angle; the angles where a swoosh
    #    hides the rim drop out of the fit and the curve carries on under them.
    RAYS = 1440

    def first_bright(ox, oy, t):
        r = 150.0
        while r < 380:
            if lum(*polar(ox, oy, r, t)) > 120:
                return max((lum(*polar(ox, oy, r + d / 2, t)), r + d / 2) for d in range(24))[1]
            r += 0.5
        return None

    rim = []
    for k in range(720):
        t = math.radians(k / 2)
        r = first_bright(sx, sy, t)
        if r:
            rim.append((sx + r * math.cos(t), sy + r * math.sin(t)))
    for _ in range(6):
        cx, cy, R0 = fit_circle(rim)
        keep = [p for p in rim if abs(math.hypot(p[0] - cx, p[1] - cy) - R0) < 3.0]
        if len(keep) == len(rim) or len(keep) < 200:
            break
        rim = keep
    seen = [(2 * math.pi * k / RAYS, first_bright(cx, cy, 2 * math.pi * k / RAYS)) for k in range(RAYS)]
    seen = [(t, r) for t, r in seen if r and abs(r - R0) < 9]
    ORDER = 3

    def basis(t):
        return [1.0] + [f(k * t) for k in range(1, ORDER + 1) for f in (math.cos, math.sin)]

    def solve(rows, values):
        n = len(rows[0])
        m = [[sum(r[i] * r[j] for r in rows) for j in range(n)] + [sum(r[i] * v for r, v in zip(rows, values))] for i in range(n)]
        for i in range(n):
            piv = max(range(i, n), key=lambda j: abs(m[j][i]))
            m[i], m[piv] = m[piv], m[i]
            m[i] = [v / m[i][i] for v in m[i]]
            for j in range(n):
                if j != i:
                    f = m[j][i]
                    m[j] = [a - f * b for a, b in zip(m[j], m[i])]
        return [row[n] for row in m]

    for _ in range(8):
        coef = solve([basis(t) for t, _ in seen], [r for _, r in seen])
        keep = [(t, r) for t, r in seen if abs(r - sum(c * b for c, b in zip(coef, basis(t)))) < 2.5]
        if len(keep) == len(seen) or len(keep) < RAYS // 4:
            break
        seen = keep
    radius = [sum(c * b for c, b in zip(coef, basis(2 * math.pi * k / RAYS))) for k in range(RAYS)]
    print(f"star at {sx:.1f}, {sy:.1f}; core centre {cx:.1f}, {cy:.1f}; rim radius {min(radius):.1f} to {max(radius):.1f} "
          f"(fitted on {len(seen)} of {RAYS} rays)")

    def ray(x, y):
        return int(round((math.atan2(y - cy, x - cx) % (2 * math.pi)) / (2 * math.pi) * RAYS)) % RAYS

    # 3. The rim and its glow as one profile over the distance from the rim, taken where nothing
    #    else is near the core; and the core's own dark just inside, per ray.
    LO, HI = -26.0, 44.0                   # the band around the rim, as a distance from it
    steps = int((HI - LO) * 2) + 1
    free = [k for k in range(RAYS)
            if at(*polar(cx, cy, radius[k] + 40, 2 * math.pi * k / RAYS))[3] < 6
            and abs(max((lum(*polar(cx, cy, radius[k] - 6 + d / 2, 2 * math.pi * k / RAYS)), d) for d in range(24))[1] / 2 - 6) < 2.5]
    assert len(free) > 40, "no free stretch of rim found"
    profile = []                           # premultiplied r, g, b and alpha per half pixel
    for s in range(steps):
        samples = [at(*polar(cx, cy, radius[k] + LO + s / 2, 2 * math.pi * k / RAYS)) for k in free]
        profile.append(tuple(median([p[c] * p[3] / 255 for p in samples]) for c in range(3)) + (median([p[3] for p in samples]),))
    dark = profile[0]
    inside = []
    for k in range(RAYS):
        samples = [at(*polar(cx, cy, radius[(k + o) % RAYS] + LO - 2 - d, 2 * math.pi * (k + o) / RAYS)) for d in range(0, 8, 2) for o in (-4, -1, 1, 4)]
        inside.append(tuple(median([p[c] for p in samples]) for c in range(4)))

    def model(rho, k):
        """The core as a whole disc, at a distance rho from its rim on ray k: straight r, g, b, alpha."""
        s = max(0.0, min(steps - 1.001, (rho - LO) * 2))
        lo, hi, f = profile[int(s)], profile[int(s) + 1], s - int(s)
        pr = [lo[c] * (1 - f) + hi[c] * f for c in range(4)]
        if pr[3] <= 0:
            return 0.0, 0.0, 0.0, 0.0
        straight = [min(255.0, pr[c] * 255 / pr[3]) for c in range(3)]
        if rho >= 0:
            return straight[0], straight[1], straight[2], pr[3]
        # inside the rim, this ray's own dark, fading into the rim's light
        mix = min(1.0, max(0.0, (pr[0] + pr[1] + pr[2] - dark[0] - dark[1] - dark[2]) / 90.0))
        i = inside[k]
        return tuple(i[c] * (1 - mix) + straight[c] * mix for c in range(3)) + (i[3] * (1 - mix) + pr[3] * mix,)

    def apart(o, m):
        return math.sqrt(sum(((o[c] * o[3] - m[c] * m[3]) / 255) ** 2 for c in range(3)) + (o[3] - m[3]) ** 2)

    # 4. Along each ray, where the core ends and a swoosh begins: the first place where the picture
    #    stops looking like the core (allowing the rim to sit a few pixels off) and stays that way.
    edge = []
    for k in range(RAYS):
        t = 2 * math.pi * k / RAYS
        miss, exact = [], []
        for s in range(steps):
            rho = LO + s / 2
            o = at(*polar(cx, cy, radius[k] + rho, t))
            exact.append(apart(o, model(rho, k)) > 30)
            miss.append(min(apart(o, model(rho + d, k)) for d in (-5, -3.5, -2, -1, 0, 1, 2, 3.5, 5)) > 30)
        found = HI
        for s in range(steps - 8):
            if all(miss[s:s + 8]):
                # a swoosh in front of the rim starts where the picture first leaves the core's dark,
                # which is a little before the place found with the rim allowed to sit off
                floor = 0 if LO + s / 2 <= 2 else int((2 - LO) * 2)
                while s > floor and exact[s - 1]:
                    s -= 1
                found = LO + s / 2
                break
        edge.append(found)
    for _ in range(2):                     # steady the edge from ray to ray
        edge = [median([edge[(k + o) % RAYS] for o in range(-14, 15)]) for k in range(RAYS)]
    print(f"a swoosh meets the core on {sum(1 for e in edge if e < HI)} of {RAYS} rays, in front of its rim on {sum(1 for e in edge if e < -1)}")

    core = bytearray(len(px))
    swoosh = bytearray(len(px))
    star = bytearray(len(px))
    reach = max(radius) + HI

    # 5. The layers. Core at the back, swooshes over it; each pixel chosen so the stack gives the original.
    clipped = 0
    for y in range(h):
        for x in range(w):
            i = (y * w + x) * 4
            o = px[i:i + 4]
            r = math.hypot(x - cx, y - cy)
            if r >= reach:
                swoosh[i:i + 4] = o
                continue
            k = ray(x, y)
            rho = r - radius[k]
            if rho < edge[k]:
                core[i:i + 4] = o           # the core and its glow, as drawn
                continue
            if rho >= HI:
                swoosh[i:i + 4] = o
                continue
            m = model(rho, k)
            ma, oa = m[3] / 255, o[3] / 255
            core[i:i + 4] = bytes(int(round(min(255, max(0, v)))) for v in m)
            if ma > 0.97:                   # an opaque core under it: the swoosh is what the picture shows
                swoosh[i:i + 4] = o
                continue
            sa = (oa - ma) / (1 - ma)
            if sa <= 0.004:                 # nothing but glow here, and no more of it than the picture has
                core[i:i + 4] = o
                continue
            a8 = max(1, min(255, int(math.ceil(min(1.0, sa) * 255))))
            sa = a8 / 255
            want = [(o[c] * oa - m[c] * ma * (1 - sa)) / sa for c in range(3)]
            clipped += 1 if min(want) < -3 or max(want) > 258 else 0
            swoosh[i:i + 4] = bytes(int(round(min(255, max(0, v)))) for v in want) + bytes([a8])
    print(f"{clipped} pixels of a swoosh's soft edge could not be matched exactly over the core's glow")

    # 6. The star: what is brighter than the core's own dark, inside a zone around the star.
    ZONE = min(230.0, min(radius) + LO - math.hypot(sx - cx, sy - cy) - 18)
    ring = []
    for k in range(360):
        t = math.radians(k)
        samples = [at(*polar(sx, sy, ZONE + 2 + d, t + math.radians(o))) for d in range(0, 14, 2) for o in range(-12, 13, 3)]
        ring.append(tuple(median([p[c] for p in samples]) for c in range(3)))
    mean = tuple(sum(p[c] for p in ring) / 360 for c in range(3))
    for y in range(int(sy - ZONE) - 1, int(sy + ZONE) + 2):
        for x in range(int(sx - ZONE) - 1, int(sx + ZONE) + 2):
            r = math.hypot(x - sx, y - sy)
            if r >= ZONE:
                continue
            i = (y * w + x) * 4
            o = px[i:i + 4]
            deg = math.degrees(math.atan2(y - sy, x - sx)) % 360
            a, b = ring[int(deg) % 360], ring[(int(deg) + 1) % 360]
            f, t = deg - int(deg), r / ZONE
            base = [min(o[c], (mean[c] * (1 - t) + (a[c] * (1 - f) + b[c] * f) * t)) for c in range(3)]
            alpha = max((o[c] - base[c]) / (255 - base[c]) for c in range(3))
            a8 = min(255, int(math.ceil(alpha * 255)))
            core[i:i + 4] = bytes(int(round(v)) for v in base) + bytes([o[3]])
            if a8 > 0:
                sa = a8 / 255
                star[i:i + 4] = bytes(int(round(min(255, max(0, (o[c] - (1 - sa) * base[c]) / sa)))) for c in range(3)) + bytes([a8])

    # 7. Stack them again and compare with the original.
    worst, total, count = 0, 0, 0
    for i in range(0, len(px), 4):
        r_, g_, b_, a_ = core[i] * core[i + 3] / 255, core[i + 1] * core[i + 3] / 255, core[i + 2] * core[i + 3] / 255, core[i + 3] / 255
        for layer in (swoosh, star):
            la = layer[i + 3] / 255
            r_, g_, b_ = layer[i] * la + r_ * (1 - la), layer[i + 1] * la + g_ * (1 - la), layer[i + 2] * la + b_ * (1 - la)
            a_ = la + a_ * (1 - la)
        oa = px[i + 3] / 255
        d = max(abs(r_ - px[i] * oa), abs(g_ - px[i + 1] * oa), abs(b_ - px[i + 2] * oa), abs(a_ * 255 - px[i + 3]))
        worst = max(worst, d); total += d; count += 1
    print(f"stacked layers against the original: largest difference {worst:.1f} of 255, mean {total / count:.3f}")

    for name, layer in (("core", core), ("swooshes", swoosh), ("star", star)):
        path = HERE / f"overseer-logo-{name}.png"
        write_png(path, w, h, layer)
        print(f"{path.name}: {path.stat().st_size // 1024} KB")
    print(f"rotate the swooshes around {cx:.1f}, {cy:.1f}; scale the star around {sx:.1f}, {sy:.1f} (pixels on a {w} by {h} canvas)")


if __name__ == "__main__":
    main()
