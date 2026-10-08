# PDFPundit ANSI BBS mockup: state and how to review it

Status as of 2026-10-07: approved direction, not final art. This folder holds
everything needed to review or change the mockup at
[`../pdfpundit-ansi-bbs.mockup.html`](../pdfpundit-ansi-bbs.mockup.html).

| File | What it is |
| --- | --- |
| `generate.py` | The source of truth. Builds the mockup HTML and the PNG stills. Python 3, standard library only. |
| `dump_goldens.py` | Dumps cell-exact JSON goldens of every frame and cat pose to `tests/data/ui/` for the Rust UI tests. Imports `generate.py` unchanged. |
| `darkberry-palette.json` | DarkBerry palette v0.3.0, vendored from <https://darkberry.slacklab.ca/palette.json> so the build works offline. |
| `frames/*.png` | One still per screen, plus each step of both drop animations (full size and widget). Read these to review the mockup without a browser. |
| `../pdfpundit-ansi-bbs.mockup.html` | Build output (about 1.9 MB). Open it in a browser to see the animation. Don't edit it by hand. |

```sh
python3 generate.py            # rewrite the mockup HTML
python3 generate.py --frames   # also re-render frames/*.png (needs Chrome or Chromium)
```

After changing the generator, re-render the frames and look at them before
committing, and re-run `python3 -I dump_goldens.py`: the Rust UI must match
those goldens cell for cell, and a test fails while they are stale. The HTML is too big to review as text. When you change only one
part, check the rest is untouched: `cmp` the other PNGs against their committed
versions. Colour class numbers are handed out in render order, so render
everything inside `page()`. A `render()` call made while the frame list is
being built renumbers every frame after it.

## Decisions this mockup encodes

These come from the user. Rationale lives in
[`../../plans/pdfpundit-desktop-app.md`](../../plans/pdfpundit-desktop-app.md)
(the Goal section) and [`../../plans/pdfpundit-ui-design.md`](../../plans/pdfpundit-ui-design.md).

- **Goal:** force serious people (forensic investigators, auditors) to use a very
  unserious app to get best-in-class PDF repair and Markdown. The cat is not
  optional, and the output files never contain the cat.
- **"Retro" means full-colour ANSI 0day BBS art**, not an amber or green CRT.
  An amber CRT mockup was rejected and deleted.
- **The window is fixed at 112×38 cells**, truecolor, with smooth gradients.
- **Default theme: DarkBerry Blackwater.** The other DarkBerry flavours and
  three other themes are in the chooser.
- **The main way in is dropping PDFs on the cat's face.** Menus sit to the side.
- **The cat is the white "Smudge" meme cat.** The idle and first drag frame must be
  recognisable as the meme: the judging squint, the lopsided half-open mouth,
  and the dinner plate in the foreground.
- **Each drop animation has 10 frames at most.** Both use 8.
- **Two drop reactions (2026-10-07).** Most terminals report nothing until the
  drop, so on drop the cat eats the files: the chomp (frame 2b). In kitty, whose
  `OSC 72` drag-and-drop protocol reports the drag, the cat also tracks the
  file before eating it (frame 2). See UI plan D2 for which terminals support
  what.
- **Widget mode:** in a terminal smaller than 112×38, the whole app becomes a
  32×16 cat-head tile for tiling desktops. The cat is half the full cat's width
  and height (a quarter of its area), with one status line. The switch is
  automatic by terminal size, and a config setting can pin a layout. When a
  decision is needed, the cat stares, blinks ‼ and says "zoom me". Enlarging the
  tile brings back the full UI. See UI plan D5.

## Frames

| Still | Screen |
| --- | --- |
| `01-idle.png` | Idle: the logo, the cat (meme pose, behind the plate), last callers and menu on the left, how-it-works and system info on the right. |
| `02-drag-1…8-*.png` | Dragging PDFs to the cat, in kitty only (see the drag animation table below). |
| `02b-chomp-1…8-*.png` | The drop reaction in every other terminal: the files land on the cat's head and it eats them (see the chomp table below). |
| `03-batch.png` | The batch repairing itself. `thesis_ar.pdf` is parked (‼) waiting on a font decision while the runner moves on. The cat still takes more files. |
| `04-font-pick.png` | Resolving the parked file: five candidate fonts, each decoding the same glyph codes into a different preview. Only the right font gives readable Arabic. |
| `05-result.png` | A finished file with before/after findings, recovery bars and font resolutions, plus the per-file menu. The cat is shown contented ("burp."). |
| `06-theme-chooser.png` | Theme chooser (`T`): each theme's 16 ANSI colours and logo ramp in a list; the selected theme broken into roles, gradients and a live sample. |
| `07-widget-idle.png` | Widget, idle: the meme cat behind its plate, "feed me a pdf". |
| `07-widget-working.png` | Widget, working: contented cat, batch progress bar and 3/7. |
| `07-widget-needs-you.png` | Widget, a decision is needed: wide eyes on the viewer, ‼ blinking in the corner, "‼ thesis_ar.pdf · zoom me". |
| `07-widget-done.png` | Widget, done: contented cat, "burp." and the tally (6 √, 1 ~, 1 ×). |
| `08-widget-drop-1…8-*.png` | The widget's drop reaction in kitty (see the widget table below). |
| `08b-widget-chomp-1…8-*.png` | The widget's chomp: frame 2b at widget scale. |
| `09-tiled-desktop.png` | The widget in context: a tile on a tiled desktop beside an editor with case notes and a shell. |

### Drag animation

Defined by `DRAG` and `DRAG_DURS` in `generate.py`. Pose fields are described
in the `pose()` comment: `yaw` head turn, `pitch` looking up, `eo` eyes open,
`mouth` 0 (resting) to 1 (fully open), `ears` perk, `meme` how much of the meme
expression remains, and `plate` how far the plate has slid down (`None` means
gone). Pupils always aim at the file.

| Step | Caption | Pose | File cell | Ring glow | Hold |
| --- | --- | --- | --- | --- | --- |
| 1 | the meme: file enters | meme squint, plate in place | (100, 7) | 0 | 0.9 s |
| 2 | ears up | ears .6, eyes .3, meme .6 | (94, 10) | 0 | 0.22 s |
| 3 | turns | yaw .25, pitch .12, eyes .7 | (88, 13) | 0 | 0.22 s |
| 4 | looks up | yaw .42, pitch .22, eyes open, mouth .1 | (82, 16) | 0 | 0.3 s |
| 5 | tracks it | yaw .36, pitch .15, mouth .3 | (77, 19) | .3 | 0.24 s |
| 6 | jaw drops | yaw .24, mouth .55, plate gone | (72, 21) | .6 | 0.22 s |
| 7 | wider | yaw .12, mouth .8 | (68, 23) | .85 | 0.22 s |
| 8 | ready to eat | facing front, mouth 1 | (64, 24) | 1 | 1.7 s, then loop |

Over the animation the side panels dim step by step. The hint line changes
from "drop a pdf on the cat" to "the cat has noticed something" to "release to
feed the cat".

In kitty the drop happens at step 8, and the cat carries on from the chomp's
step 4. The mockup loops step 8 instead.

### Chomp (drop without drag tracking)

Defined by `CHOMP`, `CHOMP_DURS`, `CHOMP_DOC`, `CHOMP_FX` and `CRUMBS` in `generate.py`.
A paste has no position, so the files land on the head, centred over the mouth.
Two pose fields were added for it: `chew` (happy shut eyes, mouth still drawn)
and `puff` (cheeks filled out). Both default to off, so the other frames are
unchanged. From step 4 the file hangs from the mouth and gets shorter; `CHOMP_DOC`
gives the first doc row drawn; a first row past the end means the file is all
eaten and only `CRUMBS` are drawn.

| Step | Caption | Pose | File (cell x, pixel row, first row) | Ring glow | Hold |
| --- | --- | --- | --- | --- | --- |
| 1 | plop | eyes .5, ears .6, meme .4, plate in place | (50, 18, 0), on the forehead, with the name label | 0 | 0.45 s |
| 2 | looks up | pitch .18, eyes open, mouth .15 | (50, 18, 0) | .3 | 0.3 s |
| 3 | jaw drops | mouth .8, plate gone | (50, 34, 0), falling between the eyes (it overlaps their inner rims by about a cell) | .6 | 0.2 s |
| 4 | chomp | chew, puff .6 | (50, 54, 4), hanging from the mouth; "CHOMP!" | 0 | 0.45 s |
| 5 | nom | chew, puff 1 | (50, 54, 9); "nom" | 0 | 0.28 s |
| 6 | nom nom | chew, puff .5 | crumbs only; "nom" twice | 0 | 0.28 s |
| 7 | gulp | eyes .5, pitch −.05 | gone; "gulp" | 0 | 0.35 s |
| 8 | burp | happy | gone; status shows "3 queued" | 0 | 1.6 s, then loop |

The hint line goes "plop · 3 pdfs landed on the cat" → "the cat has noticed
something" → "the cat is eating your pdfs" → "c h o m p" → "nom" → "nom nom nom"
→ "gulp" → "burp. 3 pdfs queued for repair". The widget uses `WCHOMP_DOC` and
`WCHOMP_FX` and shorter hints. At widget size there are no crumbs and no
"CHOMP!" text, which would not fit.

### Widget drop animation

`frame_widget('drop', step)` reuses `DRAG`'s poses, glow, hint and timing
unchanged. Only the file changes. It becomes the 7×8 px `mini_doc_grid()` and
follows `WDRAG_DOC`, straight down the right-hand edge (clear of the eyes) and
then in to the mouth: (25, 0) → (25, 5) → (24, 7) → (21, 8) → (18, 9). The hint
line goes from "feed me a pdf" to "the cat has noticed" to "» release to feed «".

### Layout (cells, 0-based column, row)

- Logo: (12, 1), 6 rows. Tagline: row 7.
- Cat: (27, 8) at scale 0.93, 58×26 cells (rows 8–33). The plate strip is at (25, 30), 61 wide.
- Left: LAST CALLERS (1, 9) 25×10, MENU (1, 20) 25×13. Right: HOW iT WORKS (86, 9) 25×12, SYSTEM (86, 22) 25×11.
- Hint: row 35. Hotkeys: row 36. Status bar: row 37.
- Widget (32×16): cat at (1, 0) at scale 0.465 (`CAT_S / 2`), 29×13 cells
  (rows 0–12). Plate at (1, 11), rows 11–13. Status line: row 14. Status bar:
  row 15. At that scale, lines under a pixel wide vanish, so `cat_grid` sets
  `_MINW` to keep lids, mouth and whiskers at least one pixel thick. It does this
  only below scale 0.5, so the full-size cats are unaffected.

### Theme roles (DarkBerry)

`db_theme()` maps palette names to the mockup's colour roles:

| Role | Palette colour |
| --- | --- |
| background | `base` |
| lightbar / status bar | `base` mixed 28% toward `jam` |
| headings / body / dim text | `text` / `subtext0` / `overlay1` |
| file names | `blueberry` |
| hotkeys and warnings | `honey` |
| errors | `cranberry` |
| repaired / ok | `gooseberry` |
| needs input, accents | `berry` (bright) / `jam` |
| info | `juniper` |
| border gradient | `petal` → `berry` → `jam` → `overlay0` → `surface2` |

Gradients are interpolated in OKLab between palette stops. The ANSI 0–15 swatches
in the chooser come straight from each flavour's `ansiColors`.

## Known issues and open questions

- **Drag tracking is kitty-only for now.** Decided on 2026-10-07: the chomp is
  the drop reaction everywhere, and frame 2's tracking runs only where the
  terminal reports drags (kitty's `OSC 72`). During a drag, kitty sends only
  MIME types, so frame 2's "thesis_ar.pdf +2" label can't show until the drop.
- In the widget's "nom" step, the three remaining doc rows read as a small
  bracket under the mouth.
- The mockup renders in a browser. A real terminal draws half-blocks and box lines
  with its own font, so check the art in the terminals you target. Only truecolor
  is mocked; the 256- and 16-colour fallbacks are not.
- In the theme chooser, the dimmed logo behind the popup shows as grey blocks.
- Widget mode:
  - Tiles smaller than 32×16 are not mocked. The plan is a one-line fallback
    (`=^..^= 3/7 ‼`).
  - Most tiling WMs ignore the terminal's resize request, so "zoom me" (the user
    zooms the tile) is the real signal.
  - At half scale, the whiskers show as a few grey pixels at the cheeks.
- The font-pick previews are illustrative strings, not real decoder output.
- The poses were drawn procedurally. The user's Instagram reference reel (a cat
  turning its head and looking up) needs a login and hasn't been reviewed.
- Not mocked yet:
  - the browse picker
  - the history screen
  - setup
  - the chain-of-custody prompt (the cat asks for a case reference and examiner)
  - the Markdown export flow
  - error and encrypted-file states
