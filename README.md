# PDFPundit

**Goal:** force serious people to interact with a very unserious application in
order to achieve best-in-class PDF repair and Markdown creation.

Forensic investigators at extremely serious policing agencies, and very serious
auditors, should have no choice but to feed their PDFs to a kitty cat.

PDFPundit is a pure-Rust terminal app. It repairs corrupted PDFs from the raw
bytes up and exports them to Markdown with correct Unicode. To use it, you drop
the file on the cat. The results are designed to hold up in a case file:
originals are never modified, output is reproducible, and it works fully
offline. An optional chain-of-custody mode will hash every input and output and
write a run report for the case file. There is no command-line version. The cat
is not optional.

Design and plans live in `nimbalyst-local/plans/`.

## Install

1. Download the archive for your machine from the GitHub Releases page.

   | Machine | Archive |
   |---|---|
   | Mac with Apple silicon | `pdfpundit-aarch64-apple-darwin.tar.xz` |
   | Linux, x86-64 | `pdfpundit-x86_64-unknown-linux-gnu.tar.xz` |
   | Windows, x86-64 | `pdfpundit-x86_64-pc-windows-msvc.zip` |

2. Check it against its `.sha256` file, or against `sha256.sum`.
   - macOS: `shasum -a 256 -c pdfpundit-aarch64-apple-darwin.tar.xz.sha256`
   - Linux: `sha256sum -c pdfpundit-x86_64-unknown-linux-gnu.tar.xz.sha256`
   - Windows: compare `Get-FileHash pdfpundit-x86_64-pc-windows-msvc.zip` with
     the file's contents.
3. Unpack it and put `pdfpundit` (`pdfpundit.exe` on Windows) somewhere on your
   `PATH`.

The archives are not code-signed yet. macOS Gatekeeper blocks the first run, so
allow it once in System Settings, Privacy & Security. Windows SmartScreen may
warn the same way.

Each archive holds the binary, `LICENSE`, this README, `THIRD_PARTY_NOTICES.md`
and `THIRD_PARTY_CRATES.md`. There is no installer and nothing on crates.io.

## Run

Open a terminal and type `pdfpundit`. Arguments are ignored.

It needs a real terminal on both stdin and stdout. Piped or redirected, it
prints one line and exits with code 2:

```
PDFPundit runs in a terminal; drop PDFs on the cat.
```

## Feed the cat

Drag PDFs from your file manager onto the terminal window, or paste their paths.
Each file the cat accepts goes into the queue, which repairs them one by one.

| Terminal | What a drop does |
|---|---|
| kitty | The cat watches the drag. Only a drop on the cat counts. |
| Others | The terminal pastes the paths, and the cat chomps them. |
| Windows | The terminal types the paths as keys. The cat still chomps. |

If your terminal does neither, press `b` to browse for files instead.

## Keys

| Key | What it does |
|---|---|
| `b` | Browse for PDFs to feed the cat |
| `e` | Export the selected file to Markdown |
| Enter | Open the selected file's menu, which has Export → Markdown |
| `T` | Choose a theme. The default is DarkBerry Blackwater. |
| `i` | Answer the next font question now |
| `l` | Leave font questions for later |
| `q` | Quit |

`T`, `i` and `l` work in the full layout only.

## Font questions

Sometimes a repair cannot tell which font a damaged file meant. The file then
waits in the queue with a question, and the rest of the batch goes on. Press `i`
to answer it in the font picker.

| Key in the picker | What it does |
|---|---|
| `↑` `↓` | Move between candidate fonts |
| Enter | Use the font under the cursor |
| `b` | Use the best guess, for this file's other questions too |
| `s` | Skip. The best guess is used and the finding is marked partial. |
| `a` | Use the best guess for every waiting question |
| Esc | Close the picker. The file keeps waiting. |

Set `[fonts] prompt_unresolved = false` to take the best guess without asking.

## Where outputs go

| Output | Name |
|---|---|
| Repaired PDF | `<name>.repaired.pdf` |
| Markdown export | `<name>.md` |

Both go beside the input. Set `[general] output_dir` to an absolute path to
send them there instead. An output never replaces a file. If the name is taken,
PDFPundit writes `<name>.repaired (2).pdf`, then `(3)`, and so on.

The original is never modified. If its folder is read-only, the repair stops
and asks you to set `output_dir`.

## Config file

`config.toml` sits in the config folder. A missing file means the defaults. A
bad key gives a warning at start-up and the default for that key.

| Platform | Config file |
|---|---|
| macOS | `~/Library/Application Support/dev.shythulu.PDFPundit/config.toml` |
| Linux | `$XDG_CONFIG_HOME/pdfpundit/config.toml`, else `~/.config/pdfpundit/config.toml` |
| Windows | `%APPDATA%\shythulu\PDFPundit\config\config.toml` |

## Terminal size

| Terminal size | Layout |
|---|---|
| 112 × 38 or larger | Full layout: the cat, the queue, results, themes, font questions |
| 32 × 16 or larger | Widget: a small cat that takes drops. `‼` means a file needs you, so grow the window. |
| Smaller | One status line. It takes no drops and asks the terminal to grow. |

Set `[ui] layout` to `full` or `widget` to pin one of the first two.

## Guarantees

- **Offline.** The binary has no network code.
- **Deterministic.** The same input and the same answers give the same output
  bytes, on every run and every machine.
- **No cat in the output.** Repaired PDFs and Markdown carry nothing from the
  app's looks.
- **Originals untouched.** PDFPundit only reads its inputs.

## Limits

- No OCR. A scanned page with no text layer exports no text.
- No images in the Markdown export yet.
- No word lists yet, so text repair has no dictionary to check against.
- The chain-of-custody mode is not built yet.

## License

MIT, see `LICENSE`. Data compiled into the binary (fonts, glyph lists, the
DarkBerry palette) carries its own terms, listed in `THIRD_PARTY_NOTICES.md`.
Every Rust crate the binary links, with its licence text, is in
`THIRD_PARTY_CRATES.md`.
