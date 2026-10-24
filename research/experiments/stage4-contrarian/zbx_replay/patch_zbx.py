#!/usr/bin/env python3
"""Unpack zlib-bitexact-rs 0.131.1 (BSD-3-Clause, crates.io) and parameterize it for zlib level 4..9 and memLevel 1..9.

The published crate exposes only deflate_raw() at level 9 / memLevel 8 (the MAME CHD configuration). The engine underneath
is zlib 1.3.1's deflate_slow, which zlib also uses for levels 4..9; level and memLevel change only the
configuration_table row (good, lazy, nice, chain), hash_bits = memLevel + 7 and lit_bufsize = 1 << (memLevel + 6).
This script makes those values constructor arguments and adds deflate_raw_params(input, level, mem_level).
It then copies ./harness next to the patched crate so `cargo build` resolves the path dependency.

    python3 patch_zbx.py <zlib-bitexact-rs-0.131.1.crate> <builddir>

The .crate is https://crates.io/api/v1/crates/zlib-bitexact-rs/0.131.1/download
(sha256 2823c223ca3e7ae8232406ca377105b307cc5ad9f63179e05e134e46817ca6c2 on 2026-10-05).
"""
import hashlib
import shutil
import sys
import tarfile
from pathlib import Path

SHA = "2823c223ca3e7ae8232406ca377105b307cc5ad9f63179e05e134e46817ca6c2"

# zlib deflate.c configuration_table rows (good_length, max_lazy, nice_length, max_chain); levels 4..9 use deflate_slow.
CONFIG = """
// zlib configuration_table rows for the deflate_slow levels (index = level; 0..3 unused here).
const CONFIG: [(usize, usize, usize, usize); 10] = [
    (0, 0, 0, 0), (0, 0, 0, 0), (0, 0, 0, 0), (0, 0, 0, 0),
    (4, 4, 16, 16), (8, 16, 32, 32), (8, 16, 128, 128), (8, 32, 128, 256),
    (32, 128, 258, 1024), (32, 258, 258, 4096),
];
"""

EDITS = [
    ("const LEVEL: i32 = 9;\n", "const LEVEL: i32 = 9;\n" + CONFIG),
    ("    pub(crate) fn new(input: &'a [u8]) -> Self {\n        Self {\n",
     "    pub(crate) fn new(input: &'a [u8]) -> Self {\n        Self::with_params(input, LEVEL, 8)\n    }\n\n"
     "    pub(crate) fn with_params(input: &'a [u8], level: i32, mem_level: usize) -> Self {\n"
     "        assert!((4..=9).contains(&level) && (1..=9).contains(&mem_level));\n"
     "        let hash_bits = mem_level + 7;\n        let hash_size = 1usize << hash_bits;\n"
     "        let lit_bufsize = 1usize << (mem_level + 6);\n"
     "        let (good, lazy, nice, chain) = CONFIG[level as usize];\n        Self {\n"),
    ("head: vec![0u16; HASH_SIZE],", "head: vec![0u16; hash_size],"),
    ("hash_mask: HASH_MASK,", "hash_mask: (hash_size - 1) as u32,"),
    ("hash_shift: HASH_SHIFT,", "hash_shift: hash_bits.div_ceil(MIN_MATCH) as u32,"),
    ("max_chain_length: MAX_CHAIN_LENGTH,", "max_chain_length: chain,"),
    ("max_lazy_match: MAX_LAZY_MATCH,", "max_lazy_match: lazy,"),
    ("good_match: GOOD_MATCH,", "good_match: good,"),
    ("nice_match: NICE_MATCH,", "nice_match: nice,"),
    ("level: LEVEL,", "level,"),
    ("sym_buf: vec![0u8; LIT_BUFSIZE * 3],", "sym_buf: vec![0u8; lit_bufsize * 3],"),
    ("sym_end: (LIT_BUFSIZE - 1) * 3,", "sym_end: (lit_bufsize - 1) * 3,"),
]

TAIL = """
/// Contrarian patch: raw DEFLATE at zlib `level` (4..=9) and `mem_level` (1..=9), windowBits 15, default strategy.
pub(crate) fn deflate_raw_params(input: &[u8], level: i32, mem_level: usize) -> Vec<u8> {
    let mut s = DeflateState::with_params(input, level, mem_level);
    s.tr_init();
    s.deflate_slow();
    s.bw.out
}
"""

LIB_TAIL = """
/// Contrarian patch: raw DEFLATE at zlib `level` (4..=9) and `mem_level` (1..=9), windowBits 15, default strategy.
pub fn deflate_raw_params(input: &[u8], level: i32, mem_level: usize) -> Vec<u8> {
    deflate::deflate_raw_params(input, level, mem_level)
}
"""


def main(crate: Path, build: Path) -> None:
    got = hashlib.sha256(crate.read_bytes()).hexdigest()
    if got != SHA:
        sys.exit(f"sha256 mismatch: {got}")
    if build.exists():
        shutil.rmtree(build)
    build.mkdir(parents=True)
    with tarfile.open(crate) as t:
        t.extractall(build, filter="data")
    src = build / "zlib-bitexact-rs-0.131.1"
    d = src / "src" / "deflate.rs"
    text = d.read_text()
    for old, new in EDITS:
        if text.count(old) != 1:
            sys.exit(f"edit anchor not unique/found: {old[:60]!r}")
        text = text.replace(old, new)
    d.write_text(text + TAIL)
    lib = src / "src" / "lib.rs"
    lib.write_text(lib.read_text() + LIB_TAIL)
    shutil.copytree(Path(__file__).resolve().parent / "harness", build / "harness")
    print(f"patched crate at {src}; harness at {build / 'harness'}")


if __name__ == "__main__":
    main(Path(sys.argv[1]), Path(sys.argv[2]))
