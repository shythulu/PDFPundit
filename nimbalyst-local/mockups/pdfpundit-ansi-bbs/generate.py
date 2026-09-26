#!/usr/bin/env python3
"""Generates the PDFPundit ANSI BBS mockup (../pdfpundit-ansi-bbs.mockup.html).

The mockup HTML is build output: change this script, not the HTML. Every
screen is a 112x38 character grid; block-pixel art (logo, cat, plate, file
icon) uses half-block cells, two pixels per cell. The cat face is drawn in
code from a pose (see `pose()` and `DRAG`), so poses and animation frames
are data. Colours come from themes; DarkBerry is read from
darkberry-palette.json (vendored from https://darkberry.slacklab.ca/).

Usage (Python 3, standard library only):
    python3 generate.py            # rewrite the mockup HTML
    python3 generate.py --frames   # also render frames/*.png (needs Chrome)

See README.md in this folder for what each frame shows and why.
"""
import copy
import html
import json
import math
import os
import re
import shutil
import subprocess
import sys
import tempfile
from functools import lru_cache

HERE = os.path.dirname(os.path.abspath(__file__))

# ── colour maths (OKLab interpolation for smooth ramps) ───────────────────


def hx(h):
    h = h.lstrip('#')
    return tuple(int(h[i:i + 2], 16) for i in (0, 2, 4))


def _s2l(c):
    c /= 255
    return c / 12.92 if c <= 0.04045 else ((c + 0.055) / 1.055) ** 2.4


def _l2s(c):
    c = max(0.0, min(1.0, c))
    return 255 * (12.92 * c if c <= 0.0031308 else 1.055 * c ** (1 / 2.4) - 0.055)


@lru_cache(maxsize=None)
def lab(h):
    r, g, b = (_s2l(v) for v in hx(h))
    l = 0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b
    m = 0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b
    s = 0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b
    l, m, s = (math.copysign(abs(v) ** (1 / 3), v) for v in (l, m, s))
    return (0.2104542553 * l + 0.7936177850 * m - 0.0040720468 * s,
            1.9779984951 * l - 2.4285922050 * m + 0.4505937099 * s,
            0.0259040371 * l + 0.7827717662 * m - 0.8086757660 * s)


def unlab(L, a, b):
    l = (L + 0.3963377774 * a + 0.2158037573 * b) ** 3
    m = (L - 0.1055613458 * a - 0.0638541728 * b) ** 3
    s = (L - 0.0894841775 * a - 1.2914855480 * b) ** 3
    rgb = (4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
           -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
           -0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s)
    return '#%02x%02x%02x' % tuple(round(_l2s(v)) for v in rgb)


@lru_cache(maxsize=None)
def _mix(a, b, t):
    A, B = lab(a), lab(b)
    return unlab(*(A[i] + (B[i] - A[i]) * t for i in range(3)))


def mix(a, b, t):
    return _mix(a, b, round(max(0.0, min(1.0, t)), 3))


def ramp(stops, t):
    t = max(0.0, min(1.0, t))
    n = len(stops) - 1
    i = min(n - 1, int(t * n))
    return mix(stops[i], stops[i + 1], t * n - i)


def avg(cols):
    ls = [lab(c) for c in cols]
    return unlab(*(sum(v[i] for v in ls) / len(ls) for i in range(3)))


# ── themes ────────────────────────────────────────────────────────────────
KEYS = 'KbgcrmywDBGCRMYW'          # role slots (CGA order)
ANSI_PAIRS = ['KD', 'rR', 'gG', 'yY', 'bB', 'mM', 'cC', 'wW']   # ANSI 0–7 / 8–15


def mk_theme(name, desc, ansi, logo, **r):
    a = dict(zip(KEYS, ansi))
    t = dict(name=name, desc=desc, ansi=a, logo=logo)
    t['border'] = r.get('border', [a['W'], a['C'], a['B'], a['b'], a['D']])
    t['modal'] = r.get('modal', [a['W'], a['M'], a['m'], a['b']])
    t['menu'] = r.get('menu', [a['W'], a['Y'], a['y'], a['D']])
    t['bar'] = r.get('bar', [a['b'], a['B'], a['C']])
    t['bar2'] = r.get('bar2', [a['m'], a['M'], a['W']])
    t['ok'] = r.get('ok', [a['g'], a['G']])
    t['vu'] = r.get('vu', [a['r'], a['R'], a['Y'], a['G']])
    t['sep'] = r.get('sep', [a['K'], a['D'], a['B']])
    t['tag'] = r.get('tag', [a['D'], a['B'], a['M'], a['W']])
    t['shadow'] = r.get('shadow', mix(a['K'], a['b'], .6))
    return t


_DB = json.load(open(os.path.join(HERE, 'darkberry-palette.json')))
_ORDER = ['black', 'red', 'green', 'yellow', 'blue', 'magenta', 'cyan', 'white']


def db_theme(fk, desc):
    fl = _DB[fk]
    c = {k: v['hex'] for k, v in fl['colors'].items()}
    an = fl['ansiColors']
    roles = [c['base'], mix(c['base'], c['jam'], .28), c['gooseberry'], c['juniper'], c['cranberry'],
             c['jam'], c['apricot'], c['subtext0'], c['overlay1'], c['lavender'], c['gooseberry'],
             c['blueberry'], c['cranberry'], c['berry'], c['honey'], c['text']]
    t = mk_theme('DarkBerry ' + fl['name'], desc, roles,
                 [c['text'], c['petal'], c['berry'], c['jam'], mix(c['jam'], c['base'], .5)],
                 border=[c['petal'], c['berry'], c['jam'], c['overlay0'], c['surface2']],
                 modal=[c['text'], c['berry'], c['jam'], c['surface2']],
                 menu=[c['text'], c['honey'], c['apricot'], c['overlay0']],
                 bar=[c['surface2'], c['bilberry'], c['blueberry'], c['plum']],
                 bar2=[c['jam'], c['berry'], c['petal']],
                 ok=[c['surface2'], c['gooseberry']],
                 vu=[c['cranberry'], c['apricot'], c['honey'], c['gooseberry']],
                 sep=[c['base'], c['surface2'], c['overlay1']],
                 tag=[c['overlay0'], c['lavender'], c['berry'], c['text']],
                 shadow=c['crust'])
    t['ansi16'] = [(an[o]['normal']['hex'], an[o]['bright']['hex']) for o in _ORDER]
    t['family'] = 'DarkBerry'
    return t


THEMES = {t['name']: t for t in [
    db_theme('blackwater', 'bog-witch berry · darkest'),
    db_theme('mire', 'bog-witch berry · dark'),
    db_theme('fen', 'bog-witch berry · dusk'),
    db_theme('wisp', 'bog-witch berry · light'),
    mk_theme('ACiD Classic', 'the original 16 · 1994',
             ['#000000', '#0000AA', '#00AA00', '#00AAAA', '#AA0000', '#AA00AA', '#AA5500', '#AAAAAA',
              '#555555', '#5555FF', '#55FF55', '#55FFFF', '#FF5555', '#FF55FF', '#FFFF55', '#FFFFFF'],
             ['#FFFFFF', '#55FFFF', '#00AAAA', '#5555FF', '#0000AA', '#AA00AA']),
    mk_theme('Pastel Parlour', 'lilac · blush · mint',
             ['#1c1a22', '#3a3350', '#7fb8a4', '#9fb4d8', '#d9798f', '#b889c4', '#d9a877', '#d8d2dc',
              '#6b6474', '#c0c4e6', '#bfe6c9', '#cfe3f7', '#ff9fb5', '#e1c0e9', '#f4ddc7', '#f9f6ed'],
             ['#f9f6ed', '#fcbbdb', '#e1c0e9', '#c0c4e6', '#dcf1f0']),
    mk_theme('Mono Ink', 'greyscale · low colour',
             ['#0c0c0c', '#2e2e2e', '#8a8a8a', '#a8a8a8', '#6e6e6e', '#7a7a7a', '#9a9a9a', '#bdbdbd',
              '#555555', '#cfcfcf', '#e0e0e0', '#f0f0f0', '#ffffff', '#d8d8d8', '#eeeeee', '#ffffff'],
             ['#ffffff', '#d0d0d0', '#a0a0a0', '#707070', '#404040']),
]}
for _t in THEMES.values():
    _t.setdefault('ansi16', [(_t['ansi'][n], _t['ansi'][b]) for n, b in ANSI_PAIRS])
DEFAULT_THEME = 'DarkBerry Blackwater'
T = THEMES[DEFAULT_THEME]


def R(v):
    if v is None:
        return None
    return v if v.startswith('#') else T['ansi'][v]


BG = lambda: T['ansi']['K']

# ── canvas ────────────────────────────────────────────────────────────────
TOK = re.compile(r'(\{[A-Za-z_]?(?:/[A-Za-z_])?\})')
TOKF = re.compile(r'\{([A-Za-z_]?)(?:/([A-Za-z_]))?\}')


def plain(s):
    return TOK.sub('', s)


class Canvas:
    def __init__(self, w, h):
        self.w, self.h = w, h
        self.c = [[[' ', R('w'), BG()] for _ in range(w)] for _ in range(h)]
        self.emb = {}
        self.blink = set()

    def put(self, x, y, ch, fg=None, bg=None):
        if 0 <= x < self.w and 0 <= y < self.h:
            for k in [k for k in self.emb if k[1] == y and k[0] <= x < k[0] + self.emb[k][0]]:
                del self.emb[k]
            cell = self.c[y][x]
            if cell[0] == 'HB':
                cell[2] = BG()
            cell[0] = ch
            if fg:
                cell[1] = R(fg)
            if bg:
                cell[2] = R(bg)

    def fill(self, x, y, w, h, bg='K', ch=' ', fg='w'):
        for j in range(y, y + h):
            for i in range(x, x + w):
                self.put(i, j, ch, fg, bg)

    def rich(self, x, y, t, bg=None, fg='w'):
        cx = x
        for tok in TOK.split(t):
            if not tok:
                continue
            m = TOKF.fullmatch(tok)
            if m:
                if m.group(1):
                    fg = m.group(1)
                if m.group(2):
                    bg = None if m.group(2) == '_' else m.group(2)
                continue
            for ch in tok:
                self.put(cx, y, ch, fg, bg)
                cx += 1
        return cx

    def gtext(self, x, y, t, stops, sym=False, bg=None):
        n = len(t)
        for i, ch in enumerate(t):
            if sym:
                p = min(i, n - 1 - i) / max(1, (n - 1) / 2)
            else:
                p = i / max(1, n - 1)
            if ch != ' ':
                self.put(x + i, y, ch, ramp(stops, p), bg)

    def hramp(self, x, y, w, stops):
        for i in range(w):
            self.put(x + i, y, '█', ramp(stops, i / max(1, w - 1)))

    def embed(self, x, y, w, text, fg='w', rtl=True, bg='K'):
        for i in range(w):
            self.put(x + i, y, ' ')
        self.emb[(x, y)] = (w, text, R(fg), rtl, R(bg))

    def pix(self, x, y, grid):
        H = len(grid)
        W = max(len(r) for r in grid)

        def g(r, c):
            return grid[r][c] if r < H and c < len(grid[r]) else None

        for cy in range((H + 1) // 2):
            for cx in range(W):
                t, b = g(2 * cy, cx), g(2 * cy + 1, cx)
                if t is None and b is None:
                    continue
                X, Y = x + cx, y + cy
                if not (0 <= X < self.w and 0 <= Y < self.h):
                    continue
                cell = self.c[Y][X]
                ut, ub = (cell[1], cell[2]) if cell[0] == 'HB' else (cell[2], cell[2])
                self.c[Y][X] = ['HB', t or ut, b or ub]

    def bar(self, x, y, w, pct, stops):
        n = int(round(w * pct))
        for i in range(w):
            if i < n:
                self.put(x + i, y, '█', ramp(stops, i / max(1, w - 1)))
            elif i == n:
                self.put(x + i, y, '▓', stops[-1])
            elif i == n + 1:
                self.put(x + i, y, '▒', stops[-1])
            else:
                self.put(x + i, y, '·', 'D')

    def vu(self, x, y, w, pct):
        n = int(round(w * pct))
        for i in range(w):
            if i < n:
                self.put(x + i, y, '■', ramp(T['vu'], i / max(1, w - 1)))
            else:
                self.put(x + i, y, '·', 'D')

    def dim_rect(self, x, y, w, h, f):
        for j in range(y, y + h):
            for i in range(x, x + w):
                if 0 <= i < self.w and 0 <= j < self.h:
                    cell = self.c[j][i]
                    cell[1] = mix(cell[1], BG(), f)
                    cell[2] = mix(cell[2], BG(), f)


def darken_cell(cell, f=.7):
    if cell[0] == 'HB':
        cell[1], cell[2] = mix(cell[1], BG(), f), mix(cell[2], BG(), f)
    else:
        cell[1], cell[2] = mix(cell[1], BG(), f), mix(cell[2], BG(), f)


class Box:
    def __init__(self, cv, x, y, w, h, title=None, note=None, grad=None,
                 fill='K', tfg='W', tbg='b', shadow=False, dbl=True):
        self.cv, self.x, self.y, self.w, self.h = cv, x, y, w, h
        self.grad, self.fillc, self.dbl = grad or T['border'], fill, dbl
        if shadow:
            pts = [(i, j) for j in range(y + 1, y + h + 1) for i in (x + w, x + w + 1)]
            pts += [(i, y + h) for i in range(x + 2, x + w)]
            for i, j in pts:
                if 0 <= i < cv.w and 0 <= j < cv.h:
                    darken_cell(cv.c[j][i], .75)
        cv.fill(x + 1, y + 1, w - 2, h - 2, fill)
        H, V, TL, TR, BL, BR = ('═', '║', '╔', '╗', '╚', '╝') if dbl else ('─', '│', '┌', '┐', '└', '┘')
        for i in range(w):
            for j in (0, h - 1):
                ch = H
                if i == 0:
                    ch = TL if j == 0 else BL
                elif i == w - 1:
                    ch = TR if j == 0 else BR
                cv.put(x + i, y + j, ch, self.col(i, j), fill)
        for j in range(1, h - 1):
            cv.put(x, y + j, V, self.col(0, j), fill)
            cv.put(x + w - 1, y + j, V, self.col(w - 1, j), fill)
        L, Rr = ('╡', '╞') if dbl else ('┤', '├')
        if title:
            cv.put(x + 2, y, L, self.col(2, 0))
            e = cv.rich(x + 3, y, f'{{{tfg}/{tbg}}} {title} ')
            cv.put(e, y, Rr, self.col(e - x, 0), fill)
        if note:
            n = len(plain(note))
            nx = x + w - 4 - (n + 2)
            cv.put(nx, y, L, self.col(nx - x, 0), fill)
            cv.rich(nx + 1, y, ' ' + note + ' ', bg=fill)
            cv.put(nx + n + 3, y, Rr, self.col(nx + n + 3 - x, 0), fill)

    def col(self, i, j):
        t = (i / max(1, self.w - 1) + j / max(1, self.h - 1)) / 2
        return ramp(self.grad, t)

    def sep(self, row, stops=None):
        j = row - self.y
        L, Rr = ('╟', '╢') if self.dbl else ('├', '┤')
        self.cv.put(self.x, row, L, self.col(0, j), self.fillc)
        self.cv.put(self.x + self.w - 1, row, Rr, self.col(self.w - 1, j), self.fillc)
        self.cv.gtext(self.x + 1, row, '─' * (self.w - 2), stops or T['sep'], sym=True, bg=self.fillc)

    def line(self, row, t, x_off=1):
        return self.cv.rich(self.x + x_off, row, t, bg=self.fillc)


def dimmed(cv, f=.68):
    d = copy.deepcopy(cv)
    for row in d.c:
        for cell in row:
            if cell[0] == 'HB':
                cell[1], cell[2] = mix(cell[1], BG(), f), mix(cell[2], BG(), f)
            else:
                cell[1], cell[2] = mix(cell[1], BG(), f), mix(cell[2], BG(), .85)
    d.emb = {k: (v[0], v[1], mix(v[2], BG(), f), v[3], BG()) for k, v in d.emb.items()}
    d.blink = set()
    return d


# ── render ────────────────────────────────────────────────────────────────
CLS = {}
HALVES = set()


def cid(h):
    if h not in CLS:
        CLS[h] = len(CLS)
    return CLS[h]


def render(cv):
    bg0 = BG()
    rows = []
    for y in range(cv.h):
        runs = []
        x = 0
        while x < cv.w:
            if (x, y) in cv.emb:
                w, text, fg, rtl, ebg = cv.emb[(x, y)]
                runs.append([('emb', fg, rtl, ebg), text, w])
                x += w
                continue
            ch, fg, bg = cv.c[y][x]
            bl = (x, y) in cv.blink
            if ch == 'HB' or ch in '▀▄█':
                top, bot = {'█': (fg, fg), '▀': (fg, bg), '▄': (bg, fg)}.get(ch, (fg, bg))
                if top == bot:
                    key = ('solid', top, bl)
                else:
                    key = ('half', top, bot)
                    HALVES.add((top, bot))
                t = ' '
            else:
                key, t = ('txt', fg, bg, bl), ch
            if runs and runs[-1][0] == key and key[0] != 'half':
                runs[-1][1] += t
                runs[-1][2] += 1
            else:
                runs.append([key, t, 1])
            x += 1
        out = []
        for key, t, w in runs:
            st = f' style="width:{w}ch"' if w > 1 else ''
            if key[0] == 'emb':
                d = 'rtl' if key[2] else 'ltr'
                bgc = f' k{cid(key[3])}' if key[3] != bg0 else ''
                out.append(f'<i class="e f{cid(key[1])}{bgc}" dir="{d}"{st}>{html.escape(t)}</i>')
            elif key[0] == 'solid':
                out.append(f'<i class="k{cid(key[1])}{" bl" if key[2] else ""}"{st}></i>')
            elif key[0] == 'half':
                out.append(f'<i class="h{cid(key[1])}_{cid(key[2])}"{st}></i>')
            else:
                _, fg, bg, bl = key
                if t.strip() == '' and bg == bg0:
                    out.append(f'<i{st}></i>')
                    continue
                cls = f'f{cid(fg)}' + (f' k{cid(bg)}' if bg != bg0 else '') + (' bl' if bl else '')
                out.append(f'<i class="{cls}"{st}>{html.escape(t)}</i>')
        rows.append('<div>' + ''.join(out) + '</div>')
    return '\n'.join(rows)


# ── logo ──────────────────────────────────────────────────────────────────
FONT = {
    'P': ["#######.", "########", "##....##", "##....##", "########",
          "#######.", "##......", "##......", "##......", "##......"],
    'D': ["######..", "#######.", "##...###", "##....##", "##....##",
          "##....##", "##....##", "##...###", "#######.", "######.."],
    'F': ["########", "########", "##......", "##......", "######..",
          "######..", "##......", "##......", "##......", "##......"],
    'U': ["##....##", "##....##", "##....##", "##....##", "##....##",
          "##....##", "##....##", "###..###", "########", ".######."],
    'N': ["##....##", "###...##", "####..##", "####..##", "##.##.##",
          "##.##.##", "##..####", "##..####", "##...###", "##....##"],
    'I': ["######", "######", "..##..", "..##..", "..##..",
          "..##..", "..##..", "..##..", "######", "######"],
    'T': ["########", "########", "...##...", "...##...", "...##...",
          "...##...", "...##...", "...##...", "...##...", "...##..."],
}


def logo_grid(word='PDFPUNDIT', gap=2):
    rows = [''] * 10
    for n, ch in enumerate(word):
        for r in range(10):
            rows[r] += FONT[ch][r] + ('.' * gap if n < len(word) - 1 else '')
    Wd = len(rows[0]) + 1
    grid = [[None] * Wd for _ in range(11)]
    for r in range(10):
        for c, v in enumerate(rows[r]):
            if v == '#':
                sheen = 0.12 * math.sin(c / Wd * math.pi)
                grid[r][c] = mix(ramp(T['logo'], r / 9), '#ffffff', sheen)
    for r in range(10):
        for c, v in enumerate(rows[r]):
            if v == '#' and grid[r + 1][c + 1] is None:
                grid[r + 1][c + 1] = T['shadow']
    return grid


# ── procedural cat face (model space 62×56): white fluff, judging squint ──
# A pose drives the face: yaw turns the head (features slide round a sphere),
# pitch tips it up, eo opens the eyes, gx/gy aim the pupils, ears perks the
# ears, mouth opens the jaw (0 = the resting half-open mouth, 1 = ready to eat).
FUR = ['#ffffff', '#f5eff5', '#e2d8e4', '#bcaec2', '#8a7c91']
LINE = '#3a2d3c'
EAR_L = ((6.0, 27), (8.5, 1.0), (27.5, 12.5))
EAR_L_IN = ((10.2, 22), (10.4, 5.8), (22.5, 13.8))


def pose(**kw):
    p = dict(yaw=0.0, pitch=0.0, eo=0.0, mouth=0.0, gx=0.0, gy=0.0, ears=0.0, happy=False,
             meme=1.0, plate=0.0)
    p.update(kw)
    return p


POSES = {'closed': pose(), 'open': pose(eo=1, mouth=1, gy=.2, meme=0, plate=None),
         'happy': pose(happy=True, meme=0, plate=None)}


def ell(x, y, cx, cy, rx, ry):
    return ((x - cx) / rx) ** 2 + ((y - cy) / ry) ** 2


def in_tri(x, y, a, b, c):
    def s(p, q, r):
        return (p[0] - r[0]) * (q[1] - r[1]) - (q[0] - r[0]) * (p[1] - r[1])
    d1, d2, d3 = s((x, y), a, b), s((x, y), b, c), s((x, y), c, a)
    return not ((d1 < 0 or d2 < 0 or d3 < 0) and (d1 > 0 or d2 > 0 or d3 > 0))


def seg(x, y, a, b):
    ax, ay = a
    bx, by = b
    dx, dy = bx - ax, by - ay
    t = max(0, min(1, ((x - ax) * dx + (y - ay) * dy) / (dx * dx + dy * dy)))
    return math.hypot(x - ax - t * dx, y - ay - t * dy)


def hsh(a, b):
    v = math.sin(a * 12.9898 + b * 78.233) * 43758.5453
    return v - math.floor(v)


def fluffy(x, y, cx, cy, rx, ry, amp, seed):
    e = ell(x, y, cx, cy, rx, ry)
    if e > 1.5:
        return False, e
    ang = math.atan2((y - cy) / ry, (x - cx) / rx)
    thr = 1 + amp * math.sin(ang * 19 + seed) + amp * .6 * math.sin(ang * 33 + seed * 2.3)
    return e <= thr, e


def ear_pts(tri, side, p):
    """Place one ear: side -1 = viewer's left, +1 = viewer's right."""
    pts = [((62 - x) if side > 0 else x, y) for x, y in tri]
    base_x = (pts[0][0] + pts[2][0]) / 2
    squash = 1 - 0.3 * p['yaw'] * side if p['yaw'] * side > 0 else 1 + 0.08 * abs(p['yaw'])
    out = []
    for i, (x, y) in enumerate(pts):
        x = base_x + (x - base_x) * squash + p['yaw'] * 8
        y = y + p['pitch'] * 6
        if i == 1:                                   # tip perks up and forward
            y -= p['ears'] * 2.5
            x -= side * p['ears'] * 1.2
        out.append((x, y))
    return out


def cat_sample(x, y, p, ears):
    op = p['mouth']
    happy = p['happy']
    col = None
    hx, hy = 31 + p['yaw'] * 2.5, 31 - p['pitch'] * 1.5
    h1, e1 = fluffy(x, y, hx, hy, 24.5, 18.5, .035, 0.7)
    jy = 38.5 - p['pitch'] * 4
    h2, _ = fluffy(x, y, 15 + p['yaw'] * 3.5, jy, 13, 10.5, .07, 2.1)
    h2b, _ = fluffy(62 - x, y, 15 - p['yaw'] * 3.5, jy, 13, 10.5, .07, 2.1)
    h3 = op > .25 and ell(x, y, 31 + p['yaw'] * 4, 44 + 3.5 * op - p['pitch'] * 3, 11, 8 * op) <= 1
    head = h1 or h2 or h2b or h3

    for outer, inner in ears:
        if not head and in_tri(x, y, *outer):
            col = ramp(['#cfc4d3', '#f3edf4', '#ffffff'], (y - 2) / 22)
            if in_tri(x, y, *inner):
                col = ramp(['#f7d9e3', '#efb8ca', '#d995ad'], (y - 6) / 16)
                if hsh(int(x * 1.3), int(y * .8)) < .2:
                    col = mix(col, '#ffffff', .6)
    if not head:
        return whiskers(x, y, p, col)

    # map the screen point back onto the face (features ride a sphere)
    R_, Ry = 26.0, 22.0
    sx = max(-.999, min(.999, (x - 31) / R_))
    sy = max(-.999, min(.999, (y - 31) / Ry))
    fx = 31 + R_ * math.sin(math.asin(sx) - p['yaw'])
    fy = 31 + Ry * math.sin(math.asin(sy) + p['pitch'])
    X = 31 - abs(fx - 31)
    left = fx < 31

    ecx, ecy = (20.0 if left else 42.0), 28.5
    d = math.sqrt(min(e1, 2.5))
    t = 0.04 + 0.3 * min(1.3, d) ** 3 + 0.06 * (y - 14) / 40
    if y > 40:
        t += 0.12 * (y - 40) / 15
    t += 0.05 * (hsh(int(fx * 1.4), int(fy * 0.6)) - .5)        # fur streaks
    if ell(fx, fy, ecx, ecy - 1.4, 7.5, 4.8) <= 1:
        t += 0.07 * (1 - p['eo'] * .6)                         # brow shadow
    m = p['meme']
    if m and ell(fx, fy, ecx + (2.6 if left else -2.6), ecy - 3.4, 3.6, 1.5) <= 1:
        t += 0.13 * m                                          # furrowed, judging brow
    col = ramp(FUR, t)

    # eyes: lids lift from the judging squint (eo 0) to wide (eo 1)
    eo = p['eo']
    if happy:
        if abs(ell(fx, fy, ecx, ecy + 1.2, 5, 3) - 1) < 0.3 and fy < ecy + 1.2:
            col = LINE
    else:
        ry = 3.0 + 1.4 * eo
        lid0 = ecy - ((0.95 if left else 0.0) * m + 0.5 * (1 - m))
        slope = ((0.17 if left else -0.26) * m + (0.13 if left else -0.13) * (1 - m)) * (1 - eo)
        lowlid = ecy + (2.3 if left else 1.3) * m + 3.2 * (1 - m)
        lidline = (lid0 + slope * (fx - ecx)) * (1 - eo) + (ecy - 3.7) * eo
        de = ell(fx, fy, ecx, ecy, 5.4 - 0.1 * eo, ry)
        if de <= 1 and fy >= lidline and (fy <= lowlid or eo > .5):
            dark = ramp(['#0f0a0d', '#2a2521', '#4d4f40'] if m > .5 else ['#1f181c', '#4a4b3f', '#77815b'],
                        (fy - lidline) / 2.6)
            iris = ramp(['#ecebb0', '#bcc77c', '#7d8f4f'], math.sqrt(de))
            col = mix(dark, iris, min(1, eo * 1.6))
            px, py = ecx + p['gx'] * 1.9, ecy + 0.2 + p['gy'] * 1.4
            prx, pry = 1.3 + 1.5 * eo, 2.6 + 1.1 * eo
            if eo > .25 and ell(fx, fy, px, py, prx, pry) <= 1:
                col = '#110b10'
            hlx, hly = (ecx - 1.6, ecy - 1.8) if eo > .25 else (ecx + 1.4, lidline + 0.9)
            if (fx - hlx) ** 2 + (fy - hly) ** 2 < (1.0 if eo > .25 else 0.35):
                col = '#ffffff' if eo > .25 else '#dfe2cf'
        elif de <= 1.3 + 0.1 * eo and abs(fy - lidline) < 0.8 + 0.4 * eo:
            col = LINE
        elif de <= 1 and fy > lowlid and eo <= .5:
            col = mix(col, '#8a7c91', .45)                     # puffy lower lid
        elif 1 < de <= 1.35 and fy > ecy:
            col = mix(col, '#8a7c91', .6)

    # muzzle, whisker pads
    chin_y = 46 + 7.6 * op
    if ell(X, fy, 26.2, 39.8, 6, 4.3) <= 1 or ell(fx, fy, 31, chin_y, 4.8 + .6 * op, 2.4) <= 1:
        col = ramp(['#ffffff', '#ece4ed'], (fy - 36) / 9)
        if any((X - a) ** 2 + (fy - b) ** 2 < 0.35 for a, b in ((21.8, 39.8), (22.4, 41.6), (23.9, 40.6))):
            col = '#c9bdcc'
    if in_tri(fx, fy, (28.4, 35.8), (33.6, 35.8), (31, 38.6)):
        col = ramp(['#f6c2d1', '#e59ab1', '#c47790'], (fy - 35.8) / 2.8)

    # mouth: interpolates from the resting half-open mouth to fully open
    for a_, b_ in (((31, 38.6), (31, 40.2)),):
        if seg(X, fy, a_, b_) < 0.6:
            col = LINE
    if not happy:
        mm = m * (1 - op)
        mcx = 31 - 1.4 * mm
        mcy, mrx, mry = 42.1 + 3.7 * op + .2 * mm, 3.3 + 4.5 * op + 1.1 * mm, 1.35 + 5.85 * op + .55 * mm
        top = 40.7 - 0.9 * op - 0.3 * mm + 0.16 * mm * (fx - 31)
        md = ell(fx, fy, mcx, mcy, mrx, mry)
        if op < .5:
            for a_, b_ in (((31, 40.2), (28.8, 41.2)), ((28.8, 41.2), (27.2, 40.7))):
                if seg(X, fy, a_, b_) < 0.6:
                    col = LINE
        if fy >= top:
            if md <= 1:
                col = ramp(['#1f0610', '#4f1024', '#86223f'], math.sqrt(md)) if op > .2 else \
                    ramp(['#2b1320', '#5a2338'], md)
                ty, trx, tr_y = 42.9 + 7.3 * op + .45 * mm, 2.4 + 3 * op + .9 * mm, 0.85 + 2.05 * op + .35 * mm
                if ell(fx, fy, mcx, ty, trx, tr_y) <= 1:
                    col = ramp(['#ffa9c2', '#e06a8e'], ell(fx, fy, mcx, ty, trx, tr_y))
                    if op > .5 and abs(fx - 31) < 0.45 and fy < ty + 1:
                        col = '#c14a72'
            elif md <= 1.2 and op > .2:
                col = LINE
            elif abs(ell(fx, fy, mcx, mcy, mrx + .4, mry + .45) - 1) < 0.28 and fy > mcy and op <= .2:
                col = '#b3a3b8'
        if op > .35:
            f = (op - .35) / .65
            if in_tri(X, fy, (31 - mrx * .78, top), (31 - mrx * .45, top), (31 - mrx * .6, top + 4.4 * f)):
                col = '#fdfafc'
            if op > .7 and md <= 1.25 and in_tri(X, fy, (26.2, mcy + mry * .95), (27.9, mcy + mry * .95),
                                                 (27.2, mcy + mry * .95 - 2.6 * f)):
                col = '#f3eef4'
    return whiskers(x, y, p, col)


def whiskers(x, y, p, col):
    op = p['mouth']
    wx = x - 26 * math.sin(p['yaw']) * .85
    wy = y + 22 * math.sin(p['pitch']) * .85
    X = 31 - abs(wx - 31)
    lift = -1.6 * op
    for a, b in (((22.5, 40.0), (1.0, 36.5 + lift)), ((22.5, 41.5), (0.5, 42.2 + lift)),
                 ((23.0, 43.0), (2.5, 47.6 + lift))):
        if seg(X, wy, a, b) < 0.4:
            fade = 1 - max(0, (a[0] - X) / (a[0] - b[0])) * 0.5
            col = mix(col or BG(), '#fbf7fb', .8 * fade)
    return col


def plate_grid(w, h, off):
    """Pixel grid for the meme's dinner plate; `off` slides it down (pixels)."""
    g = [[None] * w for _ in range(h)]
    cx, cy, rx, ry = w / 2, 9.5 + off, w / 2 + 1, 9
    for y in range(h):
        for x in range(w):
            samples = [c for c in (plate_px(x + a, y + b, cx, cy, rx, ry, w) for a, b in
                                   ((.25, .25), (.75, .25), (.25, .75), (.75, .75))) if c]
            if len(samples) >= 2:
                g[y][x] = avg(samples)
    return g


def plate_px(x, y, cx, cy, rx, ry, w):
    e = ell(x, y, cx, cy, rx, ry)
    if e > 1:
        return None
    if e > .86 and y < cy:
        return '#1a1117'                                          # far edge in shadow, as in the photo
    if e > .74 and y < cy:
        return '#ffffff'                                          # lit rim
    ie = ell(x, y, cx, cy + .8, rx * .8, ry * .72)
    if abs(ie - 1) < .07:
        return '#c9c0cf'                                          # inner rim
    if ie < 1:
        if y < cy - 3.2 and w * .14 < x < w * .36 and hsh(int(x * 1.3), int(y * 1.7)) < .6:
            return ['#6d9b4a', '#a3c96a', '#4f7a36', '#c0392b', '#e8d27a'][int(hsh(int(x * 2), int(y)) * 5)]
        return ramp(['#d9d5e2', '#eeebf3'], (y - cy + 6) / 6)     # the well
    return ramp(['#f1eef6', '#c9c3d3'], abs(x - cx) / rx)


def cat_grid(scale, p='closed', glow=0.0):
    if isinstance(p, str):
        p = POSES[p]
    ears = [(ear_pts(EAR_L, s, p), ear_pts(EAR_L_IN, s, p)) for s in (-1, 1)]
    Wd = round(62 * scale)
    Hd = round(56 * scale / 2) * 2
    grid = [[None] * Wd for _ in range(Hd)]
    accent = R('M')
    for py in range(Hd):
        for px in range(Wd):
            samples = []
            for sx, sy in ((.25, .25), (.75, .25), (.25, .75), (.75, .75)):
                c = cat_sample((px + sx) / scale, (py + sy) / scale, p, ears)
                if c:
                    samples.append(c)
            if len(samples) >= 2:
                grid[py][px] = avg(samples)
            elif glow:
                x, y = (px + .5) / scale, (py + .5) / scale
                dg = ell(x, y, 31, 28.8, 30.6, 27.6)
                if dg <= 1.0:
                    r = math.sqrt(dg)
                    ang = math.atan2((y - 28.8) / 27.6, (x - 31) / 30.6)
                    if (1 - r) * 27.6 * scale < 0.9 and int((ang + math.pi) / (2 * math.pi) * 44) % 2 == 0:
                        grid[py][px] = mix(BG(), accent, glow)
                    else:
                        grid[py][px] = mix(BG(), accent, 0.2 * glow * r ** 8)
    return grid



def doc_grid():
    g = [[None] * 12 for _ in range(16)]
    for y in range(16):
        for x in range(12):
            if x >= 8 and y <= 3 and x - 8 > y:
                continue
            c = '#f7f3f8'
            if x >= 8 and y <= 3:
                c = '#c9bfcf'
            elif x in (0, 11) or y in (0, 15) or (x >= 8 and y == 4 and x - 8 <= 3):
                c = '#8f8597'
            elif 7 <= y <= 9 and 1 <= x <= 10:
                c = '#e0445c' if y != 8 or x % 2 else '#ff8a9c'
            elif y in (3, 5, 11, 13) and 2 <= x <= (6 if y < 7 else 9):
                c = '#b5acbb'
            g[y][x] = c
    return g


CURSOR = ["o......", "oo.....", "oWo....", "oWWo...", "oWWWo..", "oWWWWo.", "oWWWWWo",
          "oWWWooo", "oWoWo..", "oo.oWo.", "o...oWo", ".....o."]


def cursor_grid():
    m = {'o': '#1a1020', 'W': '#ffffff'}
    return [[m.get(v) for v in row] for row in CURSOR]


# ── widgets ───────────────────────────────────────────────────────────────
W, H = 112, 38


def hk(key, label, lf='w'):
    return f'{{D}}[{{Y}}{key}{{D}}]{{{lf}}} {label}'


def statusbar(cv, parts, right):
    y = cv.h - 1
    cv.fill(0, y, cv.w, 1, 'b')
    left = ' {Y}PDFPuNDiT{W} v0.1 ' + ''.join(f'{{c}}│{{W}} {p} ' for p in parts)
    cv.rich(0, y, left, bg='b')
    right = f'{{M}}{T["name"]} {{c}}│{{W}} ' + right + ' '
    cv.rich(cv.w - len(plain(right)), y, right, bg='b')


def header(cv, crumb):
    cv.put(1, 0, '▐', ramp(T['logo'], .1))
    cv.put(2, 0, '█', ramp(T['logo'], .4))
    cv.put(3, 0, '▌', ramp(T['logo'], .8))
    e = cv.rich(5, 0, '{W}PDF{M}PuN{m}DiT {D}» ' + crumb)
    cv.gtext(e + 1, 0, '─' * (cv.w - e - 2), T['border'][::-1] + T['border'][1:])


def sparkles(cv, pts):
    for x, y, ch, k in pts:
        cv.put(x, y, ch, k)


# ── frames ────────────────────────────────────────────────────────────────
CAT_X, CAT_Y, CAT_S = 27, 8, 0.93


DRAG = [   # pose, doc cell (x, y), ring glow, side-panel dim, hint, caption
    (pose(), (100, 7), 0, .12, 'idle', 'the meme: file enters'),
    (pose(ears=.6, eo=.3, meme=.6, plate=.5), (94, 10), 0, .22, 'notice', 'ears up'),
    (pose(yaw=.25, pitch=.12, ears=1, eo=.7, meme=.2, plate=2), (88, 13), 0, .32, 'notice', 'turns'),
    (pose(yaw=.42, pitch=.22, ears=1, eo=1, mouth=.1, meme=0, plate=4), (82, 16), 0, .42, 'notice', 'looks up'),
    (pose(yaw=.36, pitch=.15, ears=.8, eo=1, mouth=.3, meme=0, plate=7), (77, 19), .3, .48, 'notice', 'tracks it'),
    (pose(yaw=.24, pitch=.06, ears=.6, eo=1, mouth=.55, meme=0, plate=None), (72, 21), .6, .55, 'feed', 'jaw drops'),
    (pose(yaw=.12, ears=.4, eo=1, mouth=.8, meme=0, plate=None), (68, 23), .85, .55, 'feed', 'wider'),
    (pose(yaw=.05, pitch=-.02, ears=.3, eo=1, mouth=1, meme=0, plate=None), (64, 24), 1, .55, 'feed', 'ready to eat'),
]
DRAG_DURS = [.9, .22, .22, .3, .24, .22, .22, 1.7]


def draw_plate(cv, off):
    grid = plate_grid(61, 10, off * 2)
    cv.pix(25, 30, grid)
    for j in range(35, 38):                       # the plate slides off behind the footer rows
        for i in range(cv.w):
            if cv.c[j][i][0] == 'HB':
                cv.c[j][i] = [' ', R('w'), BG()]


def gaze_to(doc):
    """Aim the pupils at the middle of the dragged file (cat model space)."""
    px = (doc[0] + 6 - CAT_X) / CAT_S
    py = ((doc[1] - CAT_Y) * 2 + 8) / CAT_S
    return max(-1, min(1, (px - 31) / 22)), max(-1, min(1, (py - 28) / 18))


def frame_main(step=None):
    drag = step is not None
    cv = Canvas(W, H)
    cv.pix(12, 1, logo_grid())
    tag = '·∙· f u r e n s i c   p d f   r e p a i r ·∙·'
    cv.gtext((W - len(tag)) // 2, 7, tag, T['tag'], sym=True)

    lc = Box(cv, 1, 9, 25, 10, title='LAST CALLERS')
    for i, (ic, k, f) in enumerate([('√', 'G', 'thesis_ar.pdf'), ('√', 'G', 'minutes_q3.pdf'),
                                     ('~', 'Y', 'invoice_scan.pdf'), ('·', 'w', 'contract_signed.pdf'),
                                     ('×', 'R', 'payroll_locked.pdf')]):
        lc.line(10 + i, f' {{{k}}}{ic} {{C}}{f}')
    lc.line(16, ' {D}57 files · 91 runs')
    mb = Box(cv, 1, 20, 25, 13, title='MENU')
    for i, (k, label) in enumerate([('B', 'browse for pdfs'), ('H', 'history'), ('S', 'setup'),
                                    ('T', 'theme'), ('?', 'help'), ('Q', 'quit')]):
        mb.line(21 + i, ' ' + hk(k, label))
    mb.sep(28)
    e = mb.line(30, ' {c}main {W}» ')
    cv.put(e, 30, '█', 'w')
    cv.blink.add((e, 30))

    hb = Box(cv, 86, 9, 25, 12, title='HOW iT WORKS')
    for i, t in enumerate(['{M}1 {w}drop pdfs on the cat', '{M}2 {w}it repairs them', '  {D}all by itself',
                           '{M}3 {w}it only asks when', '  {D}it gets stuck', '{M}4 {w}fixed copies land',
                           '  {D}beside the original', '  {C}*.repaired.pdf']):
        hb.line(10 + i, ' ' + t)
    sb = Box(cv, 86, 22, 25, 11, title='SYSTEM')
    for i, t in enumerate(['{D}engine   {W}pure rust', '{D}network  {G}off', '{D}fonts    {W}bundled',
                           '{D}theme    {M}' + T['name'].split()[-1], '{D}history  {W}57 files', '{D}originals{G} untouched']):
        sb.line(24 + i, ' ' + t)

    if drag:
        p, doc, glow, dim, hint_kind, _ = DRAG[step]
        if p['eo'] > 0:
            p = dict(p)
            p['gx'], p['gy'] = gaze_to(doc)
        cv.dim_rect(0, 8, 27, 26, dim)
        cv.dim_rect(85, 8, 27, 26, dim)
        cv.pix(CAT_X, CAT_Y, cat_grid(CAT_S, p, glow=glow))
        if p['plate'] is not None:
            draw_plate(cv, p['plate'])
        cv.pix(doc[0], doc[1], doc_grid())
        cv.pix(doc[0] + 5, doc[1] + 4, cursor_grid())
        cv.rich(min(doc[0] + 9, 91), doc[1] + 7, '{W/m} thesis_ar.pdf +2 {/_}')
        if hint_kind == 'feed':
            hint = '» release to feed the cat · 3 pdfs «'
            cv.gtext((W - len(hint)) // 2, 35, hint, [R('m'), R('M'), R('W')], sym=True)
        else:
            hint = '·∙· the cat has noticed something ·∙·' if hint_kind == 'notice' else '·∙· drop a pdf on the cat ·∙·'
            cv.gtext((W - len(hint)) // 2, 35, hint, T['tag'], sym=True)
    else:
        cv.pix(CAT_X, CAT_Y, cat_grid(CAT_S, 'closed'))
        draw_plate(cv, 0)
        sparkles(cv, [(30, 10, '*', 'M'), (80, 11, '∙', 'C'), (83, 20, '+', 'Y'),
                      (29, 25, '∙', 'W'), (82, 29, '*', 'm'), (28, 18, '·', 'B')])
        hint = '·∙· drop a pdf on the cat ·∙·'
        cv.gtext((W - len(hint)) // 2, 35, hint, T['tag'], sym=True)
    cv.rich(1, 36, ' ' + '  '.join([hk('B', 'browse'), hk('H', 'history'), hk('S', 'setup'),
                                     hk('T', 'theme'), hk('?', 'help'), hk('Q', 'quit')]))
    if drag:
        statusbar(cv, ['node 1', '{M}dragging 3 files', '{G}offline'], '112×38 {c}│{W} 11:38')
    else:
        statusbar(cv, ['node 1', 'idle', '0 queued', '{G}offline'], '112×38 {c}│{W} 11:38')
    return cv


QUEUE_A = [
    ('√', 'G', 'report_2024.pdf', 'repaired · 3 fixed', 'w', False),
    ('√', 'G', 'contract_signed.pdf', 'clean · nothing to fix', 'D', False),
    ('‼', 'M', 'thesis_ar.pdf', 'needs input · 2 fonts', 'M', True),
    ('√', 'G', 'minutes_q3.pdf', 'repaired · 1 fixed', 'w', False),
    ('☼', 'C', 'invoice_scan.pdf', 'repairing · C9 salvage', 'C', True),
    ('∙', 'D', 'board_deck.pdf', 'queued', 'D', False),
    ('×', 'R', 'payroll_locked.pdf', 'encrypted · decrypt first', 'R', False),
]
QUEUE_B = [
    ('√', 'G', 'report_2024.pdf', 'repaired · 3 fixed', 'w', False),
    ('√', 'G', 'contract_signed.pdf', 'clean', 'D', False),
    ('√', 'G', 'thesis_ar.pdf', 'repaired · 2 fonts', 'w', False),
    ('√', 'G', 'minutes_q3.pdf', 'repaired · 1 fixed', 'w', False),
    ('~', 'Y', 'invoice_scan.pdf', 'partial · 88% salvaged', 'Y', False),
    ('☼', 'C', 'board_deck.pdf', 'repairing', 'C', True),
    ('×', 'R', 'payroll_locked.pdf', 'encrypted', 'R', False),
]


def queue_box(cv, x, y, w, entries, sel, note, nw=21):
    qb = Box(cv, x, y, w, len(entries) + 4, title='QUEUE', note=note)
    qb.line(y + 1, ' {D}   file                  status')
    for i, (ic, icf, name, st, stf, bl) in enumerate(entries):
        ry = y + 2 + i
        if i == sel:
            cv.fill(x + 1, ry, w - 2, 1, 'b')
        cv.put(x + 2, ry, ic, icf)
        if bl:
            cv.blink.add((x + 2, ry))
        nf, sf = ('W', 'W') if i == sel else ('C', stf)
        cv.rich(x + 5, ry, f'{{{nf}}}{name:<{nw}} {{{sf}}}{st}')
    return qb


def progress(cv, y, cur, pct1, batch, pct2):
    pb = Box(cv, 1, y, W - 2, 4)
    pb.line(y + 1, f' {cur}')
    cv.bar(40, y + 1, 56, pct1, T['bar'])
    cv.rich(98, y + 1, f'{{W}}{int(pct1 * 100):>3}%')
    pb.line(y + 2, f' {batch}')
    cv.bar(40, y + 2, 56, pct2, T['bar2'])
    cv.rich(98, y + 2, f'{{W}}{int(pct2 * 100):>3}%')


def frame_batch():
    cv = Canvas(W, H)
    header(cv, '{w}batch {D}· {c}auto-repair ON ')
    queue_box(cv, 1, 2, 64, QUEUE_A, 4, '{G}3{w}/7 done {D}·{M} 1 needs input {D}·{R} 1 failed')

    ab = Box(cv, 1, 13, 64, 19, title='ANALYSiS » invoice_scan.pdf')
    ab.line(14, ' {D}PDF {W}1.4 {D}· {W}6{D} pages · {W}1.1{D} MB · {w}Microsoft: Print To PDF')
    ab.line(16, ' {G}■{w} carve {D}─ {G}■{w} diagnose {D}─ {G}■{w} plan {D}─ '
                '{C}☼{W} repair {D}─ ∙ emit ─ ∙ verify')
    cv.blink.add((next(i for i in range(W) if cv.c[16][i][0] == '☼'), 16))
    ab.line(17, ' {D}toolpath {W}Resave {D}· structural only · keeps outlines')
    ab.sep(18)
    ab.line(19, ' {W}FiNDiNGS {D}streamed as found')
    ab.line(20, ' {R}[ERR] {W}C9 {w}zlib stream damaged   {D}obj 14 0 · p.3 contents')
    ab.line(21, ' {Y}[WRN] {W}C3 {w}trailer missing       {D}rebuild from carved /Root')
    ab.line(22, ' {c}[iNF]    {w}header intact         {D}%PDF-1.4')
    ab.line(23, ' {c}[iNF]    {w}212 objects carved    {D}6/6 pages found')
    ab.sep(24)
    ab.line(25, ' {W}LOG')
    ab.line(26, ' {D}11:42:07 {w}inflate obj 14 0: bad block at byte 2,113')
    ab.line(27, ' {D}11:42:07 {w}resync at byte 2,176 {D}→ {G}salvaged 88%{w} of stream')
    ab.line(28, ' {D}11:42:08 {w}rebuilding xref {D}· 212 entries')
    e = ab.line(29, ' {D}11:42:08 {w}re-linking p.3 /Contents ')
    cv.put(e, 29, '█', 'w')
    cv.blink.add((e, 29))

    nb = Box(cv, 67, 2, 44, 9, title='‼ NEEDS iNPUT', grad=T['modal'], tbg='m')
    nb.line(4, ' {C}thesis_ar.pdf {D}· 214 pages · arabic')
    nb.line(5, ' {w}2 fonts can be {W}read{w} but not {W}reproduced{w}.')
    nb.line(6, ' {D}parked · the batch keeps going.')
    nb.line(8, '  ' + hk('i', 'resolve now') + '   ' + hk('l', 'later'))

    cv.pix(68, 11, cat_grid(0.68, 'closed'))
    cv.rich(69, 31, '{D}drop more pdfs on the cat to queue them')

    progress(cv, 32, '{w}repairing {C}invoice_scan.pdf', .71, '{w}batch {W}3{D}/{W}7 {D}·{M} 1 parked', .52)
    cv.rich(1, 36, ' ' + '  '.join([hk('↑↓', 'select'), hk('enter', 'file menu'), hk('i', 'resolve'),
                                     hk('+', 'add files'), hk('p', 'pause'), hk('q', 'quit')]))
    statusbar(cv, ['batch 3/7', '{M}1 needs input', '{R}1 failed', '{G}offline'], '11:42')
    return cv


def frame_fontpick(base):
    cv = dimmed(base)
    mb = Box(cv, 8, 3, 96, 29, title='PiCK A FONT » thesis_ar.pdf', note='{W}1{D} of {W}2',
             grad=T['modal'], tbg='m', shadow=True)
    mb.line(5, ' {D}slot {W}F3 {D}({w}CIDFont+F1{D}) · first seen p.{W}12{D} · {W}418{D} glyph codes · '
               'language guess {W}arabic')
    mb.line(6, ' {w}The text decodes through the surviving {W}/ToUnicode{w}, but no bundled font matched by name.')
    mb.line(7, ' {w}Pick the candidate whose preview {W}reads correctly{w}.')
    msep = [BG(), R('m'), R('M')]
    mb.sep(8, msep)
    for cx, t in ((12, 'candidate'), (36, 'score'), (64, 'fit'), (70, 'conf')):
        cv.rich(cx, 9, '{D}' + t)
    cands = [
        ('Noto Naskh Arabic', .64, .31, 'مقدمة في تحليل البيانات: الفصل الأول — المنهجية والنتائج', True),
        ('Noto Kufi Arabic', .55, .22, 'مقدمه فى تحليل البيانات: الفصل الاول — المنهجيه والنتائج', True),
        ('Noto Sans Arabic', .47, .18, 'مقدمة ڤي ټحليل الںيانات: الڡصل الأول — المںهجية والںتائج', True),
        ('Noto Nastaliq Urdu', .21, .07, 'ۓۃڈ ٹںڑ ۂۀۓ ڑژ ڷڸڹ ۓۃ ڈٹ ۂۀ ڑژۓ ۃڈٹ ںڑۂ', True),
        ('Noto Sans (latin)', .04, .01, 'Ù…Ù‚Ø¯Ù…Ø© Ù ÙŠ ØªØ­Ù„ÙŠÙ„ Ø§Ù„Ø¨ÙŠØ§Ù†Ø§Øª', False),
    ]
    for i, (name, fit, conf, prev, rtl) in enumerate(cands):
        ry = 10 + i * 3
        sel = i == 0
        if sel:
            cv.fill(9, ry, 94, 2, 'b')
        cv.rich(10, ry, ('{Y}► ' if sel else '  ') + ('{W}' if sel else '{w}') + f'{name:<22}')
        cv.vu(36, ry, 26, fit)
        cv.rich(64, ry, f'{{W}}{fit:.2f}  {{{"W" if sel else "D"}}}{conf:.2f}')
        if sel:
            cv.rich(78, ry, '{Y}◄ best guess')
        cv.rich(14, ry + 1, '{D}preview')
        cv.embed(23, ry + 1, 78, prev, fg='W' if sel else ('w' if i < 3 else 'D'), rtl=rtl,
                 bg='b' if sel else 'K')
    mb.sep(25, msep)
    mb.line(26, ' {D}font source  {C}(•){W} bundled sister fonts   {D}( ){w} system fonts by name   '
                + hk('tab', 'switch', 'D'))
    cv.rich(11, 28, '{W/m} ► Pick {/K}')
    cv.rich(22, 28, '{D}[ {W}Use best for both {D}]')
    cv.rich(47, 28, '{D}[ {w}Skip {D}]{D} keeps best guess, marks finding {Y}partial')
    cv.rich(10, 30, '{D}↑↓ candidate · enter pick · b best for both · s skip · esc later (file stays parked)')
    statusbar(cv, ['batch 4/7', '{M}resolving thesis_ar.pdf', '{R}1 failed', '{G}offline'], '11:43')
    return cv


def frame_result():
    cv = Canvas(W, H)
    header(cv, '{w}results ')
    queue_box(cv, 1, 2, 50, QUEUE_B, 2, '{G}5{w}/7 done {D}·{R} 1 failed', nw=20)

    rb = Box(cv, 52, 2, 59, 30, title='RESULT » thesis_ar.pdf', note='{G}√ repaired')
    rb.line(4, ' {D}out   {W}thesis_ar.repaired.pdf')
    rb.line(5, ' {D}in    {w}~/cases/2026-091/evidence/')
    rb.line(6, ' {D}path  {W}TemplateAssemble {D}· 3.8 s · re-diagnosed {G}clean')
    rb.sep(7)
    rb.line(8, ' {W}FiNDiNGS                          {D}before after')
    rb.line(9, ' {R}[ERR] {W}C2 {w}xref table missing      {R}  ×    {G}√ {D}1,842 obj')
    rb.line(10, ' {R}[ERR] {W}C8 {w}font resources deleted  {R}  ×    {G}√ {D}2 fonts')
    rb.line(11, ' {Y}[WRN] {W}C6 {w}font mapping lost       {Y}  ×    {G}√ {D}12 pages')
    rb.line(12, ' {c}[iNF]    {w}header intact           {D}  ·    ·')
    rb.sep(13)
    rb.line(14, ' {W}RECOVERY')
    rb.line(15, ' {D}pages   {W}214{D}/{W}214')
    cv.bar(76, 15, 26, 1.0, T['ok'])
    rb.line(16, ' {D}text    {W}96%')
    cv.bar(76, 16, 26, .96, T['ok'])
    rb.line(17, ' {D}images  {W}31{D}/{W}32')
    cv.bar(76, 17, 26, .97, T['ok'])
    rb.line(18, ' {D}        1 unplaceable → {w}thesis_ar.images/p041-im2.jp2')
    rb.sep(19)
    rb.line(20, ' {W}FONTS')
    rb.line(21, ' {W}F1 {w}TimesNewRomanPSMT {D}embedded {G}intact')
    rb.line(22, ' {W}F3 {w}CIDFont+F1        {D}bundled  {C}Noto Naskh Arabic {D}(picked)')
    rb.line(23, ' {W}F7 {w}unknown           {D}bundled  {C}Noto Sans {Y}partial{D} bold→reg')
    rb.sep(24)
    rb.line(25, ' ' + '  '.join([hk('o', 'open'), hk('e', 'export .md'), hk('d', 're-diagnose'), hk('c', 'copy')]))
    cv.gtext(54, 28, '─── case closed · the cat has inspected this pdf ───', [R('D'), R('m'), R('M'), R('W')], sym=True)

    cv.pix(8, 17, cat_grid(0.52, 'happy'))
    cv.rich(5, 31, '{D}the cat is satisfied. {m}burp.')

    mn = Box(cv, 17, 7, 32, 10, title='FiLE', grad=T['menu'], tbg='y', shadow=True)
    items = [('Open repaired PDF', 'o'), ('Reveal in folder', 'f'), ('Export → Markdown', 'e'),
             ('Re-diagnose', 'd'), None, ('Repair options…', 'R'), ('Copy report', 'c'),
             ('Remove from queue', 'x')]
    for i, it in enumerate(items):
        ry = 8 + i
        if it is None:
            cv.gtext(18, ry, '─' * 30, [BG(), R('y'), R('Y')], sym=True)
            continue
        label, key = it
        if i == 0:
            cv.fill(18, ry, 30, 1, 'b')
            cv.rich(18, ry, f'{{Y/b}} ► {{W}}{label:<22}{{Y}}{key:>3} ')
        else:
            cv.rich(18, ry, f'   {{w}}{label:<22}{{Y}}{key:>3}')

    progress(cv, 32, '{w}repairing {C}board_deck.pdf', .34, '{w}batch {W}5{D}/{W}7 {D}·{R} 1 failed', .79)
    cv.rich(1, 36, ' ' + '  '.join([hk('↑↓', 'move'), hk('enter', 'choose'), hk('esc', 'close menu'), hk('q', 'quit')]))
    statusbar(cv, ['batch 5/7', '{G}0 need input', '{R}1 failed', '{G}offline'], '11:46')
    return cv


def frame_themes():
    cv = dimmed(frame_main(), .8)
    pb = Box(cv, 4, 2, 104, 33, title='THEME', note=f'{{W}}{len(THEMES)}{{D}} themes · {{W}}live preview',
             grad=T['modal'], tbg='m', shadow=True)
    for j in range(3, 34):
        cv.put(38, j, '│', mix(R('D'), BG(), .2))
    cv.put(38, 2, '╤', pb.col(34, 0))
    cv.put(38, 34, '╧', pb.col(34, 32))

    for i, (name, th) in enumerate(THEMES.items()):
        ry = 4 + i * 3
        sel = name == T['name']
        if sel:
            cv.fill(5, ry, 33, 2, 'b')
        star = '{Y}★' if name == DEFAULT_THEME else ' '
        cv.rich(6, ry, ('{Y}► {W}' if sel else '  {w}') + f'{name:<22} ' + star)
        for k, (n, b) in enumerate(th['ansi16']):
            cv.c[ry + 1][8 + k] = ['HB', b, n]
        for k in range(18):
            cv.put(17 + k, ry + 1, '█', ramp(th['logo'], k / 17))
    cv.rich(6, 26, '{D}+ {w}load theme file…')
    cv.rich(8, 27, '{D}~/.config/pdfpundit/themes/')
    cv.rich(8, 28, '{D}catppuccin-style palette.json')
    cv.rich(8, 29, '{D}or *.toml · 16 ansi + ramps')
    cv.rich(8, 31, '{D}[{Y}e{D}]{w} copy & edit current')
    cv.rich(8, 32, '{Y}★{D} default')

    th = T
    a = th['ansi']
    cv.rich(40, 4, f'{{W}}{th["name"]} {{D}}· {{w}}{th["desc"]}')
    cv.rich(40, 6, '{D}source {C}darkberry.slacklab.ca {D}· palette.json v' + _DB['version'])
    cv.rich(40, 5, '{D}truecolor {G}√ {D}· 256-colour {G}√ {D}· 16-colour fallback {G}√')
    cv.rich(40, 7, '{W}ROLES')
    left = [('W', 'headings'), ('w', 'body text'), ('C', 'file names'), ('Y', 'hotkeys · warn'), ('D', 'dim · shadows')]
    right = [('R', 'errors'), ('G', 'repaired · ok'), ('M', 'needs input'), ('c', 'info'), ('b', 'lightbar · status')]
    for col_x, items in ((40, left), (73, right)):
        for i, (k, label) in enumerate(items):
            ry = 8 + i
            cv.put(col_x, ry, '█', k)
            cv.put(col_x + 1, ry, '█', k)
            if k == 'b':
                cv.rich(col_x + 3, ry, f'{{W/b}} {label} {{/K}}')
            else:
                cv.rich(col_x + 3, ry, f'{{{k}}}{label}')
            cv.rich(col_x + 22, ry, '{D}' + a[k])
    cv.rich(40, 13, '{W}GRADIENTS')
    for i, (label, key) in enumerate([('border', 'border'), ('logo', 'logo'), ('progress', 'bar'),
                                      ('attention', 'modal'), ('batch', 'bar2')]):
        cv.rich(40, 14 + i, f'{{D}}{label:<10}')
        cv.hramp(51, 14 + i, 54, th[key])
    cv.rich(40, 20, '{W}ANSi 0–15')
    for k, (n, b) in enumerate(ANSI_PAIRS):
        n, b = th['ansi16'][k]
        for dx in range(5):
            cv.put(51 + k * 7 + dx, 21, '█', n)
            cv.put(51 + k * 7 + dx, 22, '█', b)
        cv.rich(51 + k * 7, 23, '{D}' + ['blk', 'red', 'grn', 'yel', 'blu', 'mag', 'cyn', 'wht'][k])
    cv.rich(40, 21, '{D}0–7')
    cv.rich(40, 22, '{D}8–15')

    cv.rich(40, 25, '{W}SAMPLE')
    sb = Box(cv, 40, 26, 66, 7, title='QUEUE', note='{G}3{w}/7 {D}·{M} 1 needs input')
    cv.fill(41, 27, 64, 1, 'b')
    cv.rich(42, 27, '{C/b}☼  {W}invoice_scan.pdf     repairing · C9 salvage')
    cv.rich(42, 28, '{G}√  {C}report_2024.pdf      {w}repaired · 3 fixed')
    cv.rich(42, 29, '{M}‼  {C}thesis_ar.pdf        {M}needs input · 2 fonts')
    cv.rich(42, 30, '{R}[ERR] {w}C9 zlib stream   {Y}[WRN] {w}C3 trailer   {c}[iNF] {w}header ok')
    cv.bar(42, 31, 50, .71, th['bar'])
    cv.rich(94, 31, '{W} 71%')
    cv.rich(6, 33, '')
    cv.rich(40, 33, '{D}↑↓ preview (screen recolours live) · enter apply · esc cancel')
    statusbar(cv, ['node 1', '{M}choosing theme', '{G}offline'], '112×38 {c}│{W} 11:38')
    return cv


# ── page ──────────────────────────────────────────────────────────────────
CSS = """
*{box-sizing:border-box}
body{margin:0;padding:36px 24px 80px;background:#0a060b;color:#b9aabb;
  font-family:Menlo,Monaco,"DejaVu Sans Mono",Consolas,monospace}
.wrap{width:max-content;margin:0 auto}
h1{font-size:20px;font-weight:700;color:#fff0f8;margin:0 0 6px;letter-spacing:1px}
h1 span{color:#ff7ad1}
.sub{font-size:13px;color:#8a7a8e;margin:0 0 8px;max-width:1000px;line-height:1.5}
.sub b{color:#ff7ad1;font-weight:400}
.fl{font-size:13px;color:#b9aabb;margin:40px 0 10px;max-width:1000px;line-height:1.5}
.fl b{color:#ffcf6b;font-weight:400}
.term{border:1px solid #2a1c2e;border-radius:8px;overflow:hidden;width:max-content;
  box-shadow:0 18px 60px rgba(0,0,0,.7)}
.tb{height:28px;display:flex;align-items:center;gap:7px;padding:0 12px;position:relative;
  background:linear-gradient(#20152a,#170f1c);border-bottom:1px solid #2a1c2e}
.tb .d{width:10px;height:10px;border-radius:50%;background:#3d2c44}
.tb .t{position:absolute;left:0;right:0;text-align:center;font-size:11px;color:#6e5e72}
.s{font-size:14px;padding:8px 10px}
.s div{height:16px;line-height:16px;white-space:pre}
.s i{display:inline-block;width:1ch;height:16px;vertical-align:top;overflow:hidden;font-style:normal}
.s i.e{unicode-bidi:isolate;text-align:right}
.s i.e[dir=ltr]{text-align:left}
.bl{animation:bl 1.1s steps(1) infinite}
.anim{position:relative}
.anim .af{visibility:hidden}
.anim .af:first-child{position:relative}
.anim .af+.af{position:absolute;top:0;left:0}
.strip{display:grid;grid-template-columns:repeat(4,max-content);gap:14px 14px;margin-top:14px}
.strip figure{margin:0}
.strip .s{zoom:.27;border-radius:6px}
.strip figcaption{font-size:11px;color:#8a7a8e;margin-top:4px}
@keyframes bl{50%{opacity:0}}
"""


def page(frames, bare=False):
    body = []
    css = [CSS]
    for label, cv, title in frames:
        if label:
            body.append(f'<div class="fl">{label}</div>')
        body.append('<div class="term"><div class="tb"><span class="d"></span><span class="d"></span>'
                    f'<span class="d"></span><span class="t">{title}</span></div>')
        if isinstance(cv, dict):
            body.append(anim_html(cv, css))
        else:
            body.append(f'<div class="s" style="background:{BG()}">{render(cv)}</div></div>')
    for h, i in CLS.items():
        css.append(f'.f{i}{{color:{h}}}.k{i}{{background:{h}}}')
    for t, b in sorted(HALVES):
        css.append(f'.h{cid(t)}_{cid(b)}{{background:linear-gradient({t} 50%,{b} 50%)}}')
    for h, i in CLS.items():   # classes added while emitting halves
        css.append(f'.f{i}{{color:{h}}}.k{i}{{background:{h}}}')
    return f"""<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<title>PDFPundit — ANSI BBS mockups</title>
<style>{''.join(css)}</style>
</head>
<body{' style="padding:12px"' if bare else ''}>
<div class="wrap">
{'' if bare else HEADER}
{''.join(body)}
</div>
</body>
</html>
"""


HEADER = """<h1><span>PDFPUNDiT</span> — ANSI BBS mockups</h1>
<p class="sub">112×38 cells, truecolor. Theme: <b>DarkBerry Blackwater</b> (default; the Mire, Fen and Wisp flavours and a few others are in the theme chooser). The main way in is
<b>dropping PDFs on the cat's face</b>: it opens its mouth when a file hovers over it, eats it, and the
batch repairs itself. It only asks you something when a decision really needs a human.
Generated by <code>pdfpundit-ansi-bbs/generate.py</code>; see the README beside it.</p>"""


def crop(cv, x, y, w, h):
    out = Canvas(w, h)
    out.c = [[list(cv.c[j][i]) for i in range(x, x + w)] for j in range(y, y + h)]
    out.blink = set()
    return out


def anim_html(a, css):
    total = sum(a['durs'])
    t = 0
    frames, thumbs = [], []
    for i, (cv, dur) in enumerate(zip(a['frames'], a['durs'])):
        s, e = t / total * 100, (t + dur) / total * 100
        t += dur
        if i == 0:
            kf = f'0%{{visibility:visible}}{e:.2f}%{{visibility:hidden}}100%{{visibility:hidden}}'
        elif i == len(a['frames']) - 1:
            kf = f'0%{{visibility:hidden}}{s:.2f}%{{visibility:visible}}100%{{visibility:visible}}'
        else:
            kf = f'0%{{visibility:hidden}}{s:.2f}%{{visibility:visible}}{e:.2f}%{{visibility:hidden}}100%{{visibility:hidden}}'
        css.append(f'@keyframes af{i}{{{kf}}}')
        frames.append(f'<div class="s af" style="background:{BG()};animation:af{i} {total:.2f}s step-end infinite">'
                      f'{render(cv)}</div>')
        thumbs.append(f'<figure><div class="s" style="background:{BG()}">{render(crop(cv, 24, 5, 88, 31))}</div>'
                      f'<figcaption>{i + 1} · {a["captions"][i]}</figcaption></figure>')
    return (f'<div class="anim">{"".join(frames)}</div></div>'
            f'<div class="strip">{"".join(thumbs)}</div>')


def frame_drag():
    return {'frames': [frame_main(i) for i in range(len(DRAG))], 'durs': DRAG_DURS,
            'captions': [d[5] for d in DRAG]}


FRAMES = [
    ('<b>Frame 1</b> — idle. The cat is the drop target; menus and history sit to the side.',
     lambda: frame_main(), 'pdfpundit — 112×38 — DarkBerry Blackwater'),
    ('<b>Frame 2</b> — dragging PDFs toward the cat (8 frames, loops). It perks its ears, turns and looks up at '
     'the file, follows it down, and opens wide as it arrives. The drop ring fades in and the side panels step back.',
     frame_drag, 'pdfpundit — 112×38 — DarkBerry Blackwater — drop target active'),
    ('<b>Frame 3</b> — the batch repairs itself. thesis_ar.pdf is parked (‼) on a font decision; '
     'the runner moved on. The cat still takes more files.', frame_batch,
     'pdfpundit — 112×38 — DarkBerry Blackwater — repairing…'),
    ('<b>Frame 4</b> — resolving the parked file. Each candidate font decodes the same glyph codes '
     'differently; only the right one reads correctly.', lambda: frame_fontpick(frame_batch()),
     'pdfpundit — 112×38 — DarkBerry Blackwater — needs input'),
    ('<b>Frame 5</b> — a finished file, with the per-file menu open. The batch keeps running underneath.',
     frame_result, 'pdfpundit — 112×38 — DarkBerry Blackwater — results'),
    ('<b>Frame 6</b> — theme chooser (<b>T</b>). Each theme shows its 16 ANSI colours (bright over normal) '
     'and logo ramp; the right side breaks the selected theme into roles, gradients and a live sample.',
     frame_themes, 'pdfpundit — 112×38 — DarkBerry Blackwater — theme'),
]


OUT = os.path.join(HERE, '..', 'pdfpundit-ansi-bbs.mockup.html')
CHROME = ['/Applications/Google Chrome.app/Contents/MacOS/Google Chrome',
          'google-chrome', 'chromium', 'chromium-browser']


def main(out=OUT):
    with open(out, 'w') as f:
        f.write(page([(l, fn(), t) for l, fn, t in FRAMES]))
    print('wrote', os.path.normpath(out))


def still_frames():
    """Every screen as a still, including each step of the drag animation."""
    yield '01-idle', frame_main(), FRAMES[0][2]
    for i, d in enumerate(DRAG):
        yield f'02-drag-{i + 1}-{d[5].split(":")[-1].strip().replace(" ", "-")}', frame_main(i), FRAMES[1][2]
    yield '03-batch', frame_batch(), FRAMES[2][2]
    yield '04-font-pick', frame_fontpick(frame_batch()), FRAMES[3][2]
    yield '05-result', frame_result(), FRAMES[4][2]
    yield '06-theme-chooser', frame_themes(), FRAMES[5][2]


def render_frames(outdir=os.path.join(HERE, 'frames')):
    chrome = next((c for c in CHROME if os.path.exists(c) or shutil.which(c)), None)
    if not chrome:
        sys.exit('Chrome/Chromium not found; cannot render frames/*.png')
    os.makedirs(outdir, exist_ok=True)
    with tempfile.TemporaryDirectory() as tmp:
        for name, cv, title in still_frames():
            src = os.path.join(tmp, name + '.html')
            with open(src, 'w') as f:
                f.write(page([('', cv, title)], bare=True))
            png = os.path.join(outdir, name + '.png')
            subprocess.run([chrome, '--headless=new', '--disable-gpu', '--hide-scrollbars',
                            '--window-size=992,680', f'--screenshot={png}', 'file://' + src],
                           check=True, capture_output=True)
            print('wrote', os.path.relpath(png, HERE))


if __name__ == '__main__':
    main()
    if '--frames' in sys.argv[1:]:
        render_frames()
