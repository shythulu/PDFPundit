#!/usr/bin/env bash
# Stage 4 verifier (2026-10-05): rebuild pdflatex for TOOL-416 without apt (Stage 2 used Debian's
# texlive-latex-base, which the restart removed). TeX Live net installer, scheme-minimal plus the LaTeX
# format (collection-latex is not needed: latex-bin pulls the kernel), everything under $S/texlive.
#   S=<scratch dir> bash texlive_install.sh
# Time-boxed: 20 minutes; nice -n 10.
set -euo pipefail
: "${S:?set S to the scratch directory}"
# mirror.ctan.org redirected to a mirror whose TLS chain did not verify here (curl error 60); a fixed mirror is used
MIRROR=${CTAN_MIRROR:-https://mirrors.mit.edu/CTAN}/systems/texlive/tlnet
mkdir -p "$S/src/tl" && cd "$S/src/tl"
[ -f install-tl-unx.tar.gz ] || curl -sSfL -o install-tl-unx.tar.gz $MIRROR/install-tl-unx.tar.gz
sha256sum install-tl-unx.tar.gz
rm -rf install-tl-2* && tar xzf install-tl-unx.tar.gz && cd install-tl-2*
cat > tl.profile <<PROF
selected_scheme scheme-minimal
TEXDIR $S/texlive
TEXMFLOCAL $S/texlive/texmf-local
TEXMFSYSCONFIG $S/texlive/texmf-config
TEXMFSYSVAR $S/texlive/texmf-var
TEXMFHOME $S/texlive/texmf-home
TEXMFCONFIG $S/texlive/texmf-config
TEXMFVAR $S/texlive/texmf-var
binary_x86_64-linux 1
instopt_adjustpath 0
instopt_adjustrepo 0
instopt_letter 0
instopt_portable 1
tlpdbopt_autobackup 0
tlpdbopt_install_docfiles 0
tlpdbopt_install_srcfiles 0
PROF
timeout 900 nice -n 10 perl ./install-tl --no-interaction --profile=tl.profile --repository "$MIRROR"
TLB="$S/texlive/bin/x86_64-linux"
timeout 600 nice -n 10 "$TLB/tlmgr" --repository "$MIRROR" install latex-bin
"$TLB/pdflatex" --version | head -1
