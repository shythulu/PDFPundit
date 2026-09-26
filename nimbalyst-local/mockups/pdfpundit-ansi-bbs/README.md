# PDFPundit ANSI BBS mockup: state and how to review it

Status as of 2026-09-26: approved direction, not final art. This folder holds
everything needed to review or change the mockup at
[`../pdfpundit-ansi-bbs.mockup.html`](../pdfpundit-ansi-bbs.mockup.html).

| File | What it is |
| --- | --- |
| `generate.py` | The source of truth. Builds the mockup HTML and the PNG stills. Python 3, standard library only. |
| `darkberry-palette.json` | DarkBerry palette v0.3.0, vendored from <https://darkberry.slacklab.ca/palette.json> so the build works offline. |
| `frames/*.png` | One still per screen, plus each of the 8 drag-animation steps. Read these to review the mockup without a browser. |
| `../pdfpundit-ansi-bbs.mockup.html` | Build output (about 1.6 MB). Open it in a browser to see the animation. Don't edit it by hand. |

```sh
python3 generate.py            # rewrite the mockup HTML
python3 generate.py --frames   # also re-render frames/*.png (needs Chrome or Chromium)
```

After changing the generator, re-render the frames and look at them before
committing. The HTML is too big to review as text.

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
- **The drag animation has 10 frames at most.** It uses 8.

## Frames

| Still | Screen |
| --- | --- |
| `01-idle.png` | Idle: the logo, the cat (meme pose, behind the plate), last callers and menu on the left, how-it-works and system info on the right. |
| `02-drag-1…8-*.png` | Dragging PDFs to the cat (see the animation table below). |
| `03-batch.png` | The batch repairing itself. `thesis_ar.pdf` is parked (‼) waiting on a font decision while the runner moves on. The cat still takes more files. |
| `04-font-pick.png` | Resolving the parked file: five candidate fonts, each decoding the same glyph codes into a different preview. Only the right font gives readable Arabic. |
| `05-result.png` | A finished file with before/after findings, recovery bars and font resolutions, plus the per-file menu. The cat is shown contented ("burp."). |
| `06-theme-chooser.png` | Theme chooser (`T`): each theme's 16 ANSI colours and logo ramp in a list; the selected theme broken into roles, gradients and a live sample. |

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

### Layout (cells, 0-based column, row)

- Logo: (12, 1), 6 rows. Tagline: row 7.
- Cat: (27, 8) at scale 0.93, 58×26 cells (rows 8–33). The plate strip is at (25, 30), 61 wide.
- Left: LAST CALLERS (1, 9) 25×10, MENU (1, 20) 25×13. Right: HOW iT WORKS (86, 9) 25×12, SYSTEM (86, 22) 25×11.
- Hint: row 35. Hotkeys: row 36. Status bar: row 37.

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

- **Terminals don't report a drag until the drop.** During an operating-system
  drag, the terminal passes no hover events to the app. The path only arrives as a
  paste on drop. So the animation as mocked can't track a file that is still being
  dragged. It may have to play as a reaction after the drop, unless a target
  terminal supports a drag-and-drop protocol. This is in the feature plan's
  Risks and still needs a decision from the user.
- The mockup renders in a browser. A real terminal draws half-blocks and box lines
  with its own font, so check the art in the terminals you target. Only truecolor
  is mocked; the 256- and 16-colour fallbacks are not.
- In the theme chooser, the dimmed logo behind the popup shows as grey blocks.
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
