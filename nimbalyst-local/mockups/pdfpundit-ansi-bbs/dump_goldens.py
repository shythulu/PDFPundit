#!/usr/bin/env python3
"""Dumps cell-exact golden frames from generate.py for the Rust UI tests (T-00).

generate.py is the reference for the cat and the layouts (D-055). This script
imports it unchanged, captures the cells it draws, and writes one JSON file per
frame to tests/data/ui/. The Rust tests (T-17, T-18, T-19, T-21, T-22a/b, T-24)
must match these files on 100% of cells, characters and colours both.

Run it with an isolated interpreter from anywhere:

    python3 -I nimbalyst-local/mockups/pdfpundit-ansi-bbs/dump_goldens.py [OUTDIR]

OUTDIR defaults to tests/data/ui at the repo root. Every *.json file already in
OUTDIR is removed first, so the directory holds exactly the dumped set. Two runs
give byte-identical files (the output carries no time, path or version).

What is dumped:
- canvas frames: every 112x38 and 32x16 still from still_frames(), under its
  still name (01-idle, 02-drag-*, 02b-chomp-*, 03-batch, 04-font-pick,
  05-result, 06-theme-chooser, 07-widget-*, 08-widget-drop-*,
  08b-widget-chomp-*). 09-tiled-desktop is skipped: it is HTML around the
  07-widget-working canvas.
- cat grids: cat_grid() at the four scales the frames use (0.93, 0.465, 0.68,
  0.52) for every pose in POSES and WPOSES and every DRAG and CHOMP keyframe,
  both as the full layout aims it (drag-N, chomp-N) and as the widget aims it
  (wdrag-N, wchomp-N). A keyframe's pose and glow are captured from the frame
  function's own cat_grid() call, so the gaze is exactly what the frame used.
  Files are named cat-<scale>-<pose>.

File format (one object per file, keys in this order):
  name, kind ("canvas" or "cat"), w, h (cells), then for cats scale, glow and
  pose (every pose() field; plate is null when the plate is gone), then cells
  and blink, then for canvases placeholder.
  cells: row-major, w*h entries, one row per line. A text cell is
  [ch, fg, bg]. A half-block cell is ["HB", top, bottom]: the Rust side draws
  U+2580 with fg = top, bg = bottom. In a cat grid a pixel that is not drawn
  is null, and a cell with neither pixel drawn is null. Colours are lowercase
  "#rrggbb".
  blink: [x, y] pairs, sorted by row then column.
  placeholder: see PLACEHOLDER_POLICY; null on every frame but 04-font-pick.

index.json lists every frame with its kind and pins the generator by the
SHA-256 of generate.py and darkberry-palette.json.
"""
import hashlib
import importlib.util
import json
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.normpath(os.path.join(HERE, '..', '..', '..'))
GENERATE = os.path.join(HERE, 'generate.py')
PALETTE = os.path.join(HERE, 'darkberry-palette.json')

SCALES = [0.93, 0.465, 0.68, 0.52]
POSE_FIELDS = ['yaw', 'pitch', 'eo', 'mouth', 'gx', 'gy', 'ears', 'happy', 'meme', 'plate', 'chew', 'puff']
POSE_BOOLS = {'happy', 'chew'}

PLACEHOLDER = '[embedded text]'
PLACEHOLDER_POLICY = (
    'The browser draws these spans as embedded text (Canvas.emb): Arabic with RTL shaping, and one '
    'mojibake line holding a soft hyphen. A terminal cannot reproduce either cell for cell, so every '
    'embedded span is replaced: its w cells, starting at (x, y), hold the text of "text" left-aligned '
    'and then spaces, every cell with the span\'s fg and bg. "spans" keeps each original string and '
    'its direction for reference.'
)


def load_generator():
    sys.dont_write_bytecode = True      # leave no __pycache__ beside the mockup
    spec = importlib.util.spec_from_file_location('generate', GENERATE)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


def colour(c):
    if c is None:
        return None
    if not (isinstance(c, str) and len(c) == 7 and c[0] == '#'):
        raise ValueError(f'not a #rrggbb colour: {c!r}')
    int(c[1:], 16)
    return c.lower()


def sha256(path):
    with open(path, 'rb') as f:
        return hashlib.sha256(f.read()).hexdigest()


def canvas_cells(cv):
    """The canvas as rows of [ch, fg, bg], with embedded spans replaced."""
    rows = [[[ch, colour(fg), colour(bg)] for ch, fg, bg in row] for row in cv.c]
    spans = []
    for (x, y), (w, text, fg, rtl, bg) in sorted(cv.emb.items(), key=lambda kv: (kv[0][1], kv[0][0])):
        if len(PLACEHOLDER) > w:
            raise ValueError(f'placeholder does not fit the span at {(x, y)}')
        fill = PLACEHOLDER + ' ' * (w - len(PLACEHOLDER))
        for i, ch in enumerate(fill):
            if 0 <= x + i < cv.w:
                rows[y][x + i] = [ch, colour(fg), colour(bg)]
        spans.append({'x': x, 'y': y, 'w': w, 'rtl': rtl, 'fg': colour(fg), 'bg': colour(bg), 'text': text})
    placeholder = None
    if spans:
        placeholder = {'policy': PLACEHOLDER_POLICY, 'text': PLACEHOLDER, 'spans': spans}
    return rows, placeholder


def cat_cells(grid):
    """cat_grid's pixel rows packed two to a cell, as Canvas.pix packs them."""
    if len(grid) % 2:
        raise ValueError('cat grid has an odd number of pixel rows')
    rows = []
    for cy in range(len(grid) // 2):
        top, bottom = grid[2 * cy], grid[2 * cy + 1]
        rows.append([None if t is None and b is None else ['HB', colour(t), colour(b)]
                     for t, b in zip(top, bottom)])
    return rows


def pose_json(p):
    out = {}
    for k in POSE_FIELDS:
        v = p[k]
        if k in POSE_BOOLS:
            out[k] = bool(v)
        elif v is None:
            if k != 'plate':
                raise ValueError(f'pose field {k} is None')
            out[k] = None
        else:
            out[k] = float(v)
    if set(p) != set(POSE_FIELDS):
        raise ValueError(f'unexpected pose fields: {sorted(set(p) ^ set(POSE_FIELDS))}')
    return out


def dumps(v):
    return json.dumps(v, ensure_ascii=False, separators=(', ', ': '))


def frame_json(name, kind, w, h, rows, blink, extra_head=(), extra_tail=()):
    if len(rows) != h or any(len(r) != w for r in rows):
        raise ValueError(f'{name}: cells are not {w}x{h}')
    lines = ['{', f'"name": {dumps(name)},', f'"kind": {dumps(kind)},', f'"w": {w},', f'"h": {h},']
    for k, v in extra_head:
        lines.append(f'{dumps(k)}: {dumps(v)},')
    lines.append('"cells": [')
    for y, row in enumerate(rows):
        body = ', '.join(dumps(c) for c in row)
        lines.append(body + (',' if y < h - 1 else ''))
    lines.append('],')
    blink = sorted(blink, key=lambda p: (p[1], p[0]))
    tail = [('blink', [[x, y] for x, y in blink])] + list(extra_tail)
    for i, (k, v) in enumerate(tail):
        lines.append(f'{dumps(k)}: {dumps(v)}' + (',' if i < len(tail) - 1 else ''))
    lines.append('}')
    return '\n'.join(lines) + '\n'


def capture_cat_calls(g, fn):
    """Runs fn() and returns the (scale, pose, glow) of each cat_grid call it makes."""
    calls = []
    real = g.cat_grid

    def recorder(scale, p='closed', glow=0.0):
        calls.append((scale, dict(g.POSES[p] if isinstance(p, str) else p), glow))
        return real(scale, p, glow)

    g.cat_grid = recorder
    try:
        fn()
    finally:
        g.cat_grid = real
    return calls


def keyframes(g):
    """(name, pose, glow) for every pose the cat goldens cover, in a fixed order."""
    out = [(k, dict(p), 0.0) for k, p in g.POSES.items()]
    out += [('widget-' + k, dict(p), 0.0) for k, p in g.WPOSES.items()]
    sources = [('drag', len(g.DRAG), g.frame_main), ('chomp', len(g.CHOMP), g.frame_chomp),
               ('wdrag', len(g.DRAG), lambda i: g.frame_widget('drop', i)),
               ('wchomp', len(g.CHOMP), lambda i: g.frame_widget('chomp', i))]
    for prefix, n, fn in sources:
        for i in range(n):
            calls = capture_cat_calls(g, lambda: fn(i))
            if len(calls) != 1:
                raise ValueError(f'{prefix}-{i + 1}: expected one cat_grid call, got {len(calls)}')
            _, p, glow = calls[0]
            out.append((f'{prefix}-{i + 1}', p, float(glow)))
    return out


def main(outdir):
    g = load_generator()
    if g.T['name'] != g.DEFAULT_THEME:
        raise ValueError('generate.py no longer draws in its default theme')
    files = {}
    index = []

    for name, cv, _title, _px in g.still_frames():
        if isinstance(cv, dict):            # 09-tiled-desktop: HTML around 07-widget-working
            continue
        rows, placeholder = canvas_cells(cv)
        files[name] = frame_json(name, 'canvas', cv.w, cv.h, rows, cv.blink,
                                 extra_tail=[('placeholder', placeholder)])
        index.append({'name': name, 'kind': 'canvas'})

    for scale in SCALES:
        for key, p, glow in keyframes(g):
            name = f'cat-{scale!r}-{key}'
            rows = cat_cells(g.cat_grid(scale, p, glow=glow))
            head = [('scale', scale), ('glow', glow), ('pose', pose_json(p))]
            files[name] = frame_json(name, 'cat', len(rows[0]), len(rows), rows, [], extra_head=head)
            index.append({'name': name, 'kind': 'cat'})

    if len(files) != len(index):
        raise ValueError('two frames share a name')
    meta = {
        'generator': 'nimbalyst-local/mockups/pdfpundit-ansi-bbs/generate.py',
        'pinned_commit': '9439a30',
        'generate_py_sha256': sha256(GENERATE),
        'palette_sha256': sha256(PALETTE),
        'theme': g.DEFAULT_THEME,
        'scales': SCALES,
        'frames': index,
    }

    os.makedirs(outdir, exist_ok=True)
    for f in sorted(os.listdir(outdir)):
        if f.endswith('.json'):
            os.remove(os.path.join(outdir, f))
    for name, text in sorted(files.items()):
        with open(os.path.join(outdir, name + '.json'), 'w', encoding='utf-8', newline='\n') as f:
            f.write(text)
    with open(os.path.join(outdir, 'index.json'), 'w', encoding='utf-8', newline='\n') as f:
        f.write(json.dumps(meta, ensure_ascii=False, indent=1) + '\n')
    print(f'wrote {len(files)} frames to {outdir}')


if __name__ == '__main__':
    if len(sys.argv) > 2:
        sys.exit('usage: dump_goldens.py [OUTDIR]')
    main(os.path.abspath(sys.argv[1]) if len(sys.argv) == 2 else os.path.join(ROOT, 'tests', 'data', 'ui'))
