"""Build the PDFPundit wiki into site/dist/.

Each page in site/pages/ is a body fragment. Its first line is a comment:
    <!-- title: Page title | description: One sentence for search results -->
This script wraps every fragment in the shared layout and nav, then copies the
static files. Standard library only.

    python site/build.py
"""

import re
import shutil
from pathlib import Path

SITE = Path(__file__).resolve().parent
PAGES = SITE / "pages"
DIST = SITE / "dist"
STATIC = ["wiki.css", "favicon.svg"]

# Nav order. Each entry: (section, [(file, label), ...]).
NAV = [
    ("Project", [("index.html", "Home"), ("how-it-works.html", "How it works"), ("roadmap.html", "Roadmap and status")]),
    ("Take part", [("contributing.html", "Contributing"), ("research.html", "Research guide")]),
    ("Elsewhere", [
        ("https://github.com/shythulu/PDFPundit", "GitHub"),
        ("https://github.com/shythulu/PDFPundit/issues", "Issues"),
        ("https://www.slacklab.ca/", "slackLab"),
    ]),
]

FONTS = (
    "https://fonts.googleapis.com/css2?family=Bodoni+Moda:ital,opsz,wght@0,6..96,400..900;1,6..96,400..900"
    "&family=Bodoni+Moda+SC:opsz,wght@6..96,400..900&family=JetBrains+Mono:wght@400;600"
    "&family=Manufacturing+Consent&display=swap"
)

LAYOUT = """<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title}</title>
<meta name="description" content="{description}">
<meta name="theme-color" content="#402838">
<link rel="icon" href="favicon.svg" type="image/svg+xml">
<link rel="preconnect" href="https://fonts.googleapis.com">
<link rel="preconnect" href="https://fonts.gstatic.com" crossorigin>
<link rel="stylesheet" href="{fonts}">
<link rel="stylesheet" href="wiki.css">
</head>
<body>
<div class="shell">
<aside class="side">
<a class="brand" href="index.html"><span class="monogram" aria-hidden="true">P</span><span class="wordmark">PDFPundit</span></a>
<nav aria-label="Wiki">
{nav}
</nav>
</aside>
<main class="main">
<article class="page">
{body}
<footer class="foot">PDFPundit is a <a href="https://www.slacklab.ca/">slackLab</a> project. Coloured with <a href="https://darkberry.slacklab.ca">Darkberry</a> Fen. <a href="https://github.com/shythulu/PDFPundit/edit/main/site/pages/{file}">Edit this page</a>.</footer>
</article>
</main>
</div>
</body>
</html>
"""

META = re.compile(r"<!--\s*title:\s*(.*?)\s*\|\s*description:\s*(.*?)\s*-->")


def nav_html(current: str) -> str:
    parts = []
    for section, links in NAV:
        items = []
        for href, label in links:
            here = ' aria-current="page"' if href == current else ""
            items.append(f'<li><a href="{href}"{here}>{label}</a></li>')
        parts.append(f"<h2>{section}</h2>\n<ul>\n" + "\n".join(items) + "\n</ul>")
    return "\n".join(parts)


def main() -> None:
    if DIST.exists():
        shutil.rmtree(DIST)
    DIST.mkdir()
    for page in sorted(PAGES.glob("*.html")):
        text = page.read_text(encoding="utf-8")
        first, _, body = text.partition("\n")
        m = META.match(first)
        if not m:
            raise SystemExit(f"{page.name}: first line must be <!-- title: ... | description: ... -->")
        title, description = m.groups()
        html = LAYOUT.format(
            title=title, description=description, fonts=FONTS,
            nav=nav_html(page.name), body=body.strip(), file=page.name,
        )
        (DIST / page.name).write_text(html, encoding="utf-8")
    for name in STATIC:
        shutil.copy(SITE / name, DIST / name)
    (DIST / ".nojekyll").touch()
    print(f"built {len(list(PAGES.glob('*.html')))} pages into {DIST}")


if __name__ == "__main__":
    main()
