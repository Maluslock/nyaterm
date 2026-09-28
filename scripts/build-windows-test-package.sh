#!/usr/bin/env bash
#
# Cross-compile the Windows x64 test package for the tmux -CC spike branch.
#
# Produces: dist-windows/nyaterm-windows-x64/{nyaterm.exe,nyaterm-mcp.exe,README.txt}
#           dist-windows/nyaterm-windows-x64.zip
#
# Requirements: cargo-xwin (`cargo install cargo-xwin`), a `clang-cl` on PATH
# (a symlink to clang works: clang picks CL driver mode from argv[0]), and the
# xwin CRT/SDK cache (~/.cache/cargo-xwin).
#
# Two traps this script exists to avoid:
#
# 1. Missing `tauri/custom-protocol` feature. `tauri`'s build script computes
#    `dev = !has_feature("custom-protocol")`, and in dev mode `tauri-codegen`
#    embeds NO frontend assets and points the main window at
#    `build.devUrl` (http://localhost:1420). A plain `cargo xwin build` therefore
#    emits an exe that only renders if a vite dev server happens to run on the
#    target machine. The feature must be enabled explicitly — this app has no
#    `[features]` section, so it is requested as `tauri/custom-protocol`.
#
# 2. A global `RUSTFLAGS=-C target-feature=+crt-static` also applies to host
#    units. `pnpm build` runs `build:mcp-sidecar`, which builds a host binary, and
#    that fails with "cannot produce proc-macro for `darling_macro` ... does not
#    support these crate types". crt-static is therefore passed through a
#    target-scoped `--config`, so it only affects windows-msvc units.
#
# The script verifies the artifact instead of trusting the build: embedded
# frontend asset keys, tmux gateway strings, and the absence of dynamic CRT
# imports.
set -euo pipefail

REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TARGET=x86_64-pc-windows-msvc
OUT="$REPO/dist-windows/nyaterm-windows-x64"
EXE="$REPO/src-tauri/target/$TARGET/release/nyaterm.exe"
SIDECAR="$REPO/src-tauri/binaries/nyaterm-mcp-$TARGET.exe"

cd "$REPO"
export PATH="$HOME/.cargo/bin:$PATH"

echo "== 1/4 frontend + linux sidecar =="
env -u RUSTFLAGS pnpm build

echo "== 2/4 windows sidecar =="
if [ ! -f "$SIDECAR" ]; then
  NYATERM_MCP_TARGET=$TARGET node scripts/build-mcp-sidecar.mjs
fi

echo "== 3/4 cargo xwin build ($TARGET, tauri/custom-protocol) =="
env -u RUSTFLAGS cargo xwin build \
  --release \
  --target "$TARGET" \
  --manifest-path src-tauri/Cargo.toml \
  --features tauri/custom-protocol \
  --config "target.$TARGET.rustflags=[\"-C\",\"target-feature=+crt-static\"]"

echo "== verify =="
probe=$(ls dist/assets/*.js | head -1 | xargs basename)
grep -aq -- "$probe" "$EXE" || { echo "FATAL: frontend assets not embedded ($probe missing)"; exit 1; }
grep -aq -- "tmux control mode detected" "$EXE" || { echo "FATAL: tmux gateway strings missing"; exit 1; }
grep -aq -- "tmux-cc-test" "$EXE" || { echo "FATAL: build tag missing"; exit 1; }
# Note: never pipe objdump into `grep -q` — grep exits early, objdump takes SIGPIPE
# and `set -o pipefail` turns that into a 141 that aborts the script.
objdump -p "$EXE" > "$EXE.imports.txt"
if grep -qiE "DLL Name: (vcruntime|msvcp|ucrtbase|api-ms-win-crt)" "$EXE.imports.txt"; then
  echo "FATAL: dynamic CRT imports found; crt-static did not apply"
  exit 1
fi
rm -f "$EXE.imports.txt"
echo "ok: assets embedded ($probe), tmux gateway present, build tag present, static CRT"

echo "== 4/4 package =="
mkdir -p "$OUT"
cp "$EXE" "$OUT/nyaterm.exe"
cp "$SIDECAR" "$OUT/nyaterm-mcp.exe"
# portable.flag switches the app to self-contained mode: its own identifier (so a
# running installed build cannot swallow the launch through single-instance), its
# own config and logs under <exe_dir>/data, and an unshared database file. That is
# what makes a hand-built test package distinguishable from an installed release.
: > "$OUT/portable.flag"
(
  cd "$REPO/dist-windows"
  rm -f nyaterm-windows-x64.zip
  if command -v zip >/dev/null 2>&1; then
    zip -qr nyaterm-windows-x64.zip nyaterm-windows-x64
  else
    python3 -c "import zipfile,os;root='nyaterm-windows-x64';z=zipfile.ZipFile('nyaterm-windows-x64.zip','w',zipfile.ZIP_DEFLATED);[z.write(os.path.join(root,n),os.path.join(root,n)) for n in sorted(os.listdir(root)) if os.path.isfile(os.path.join(root,n))];z.close()"
  fi
)
ls -l "$OUT" "$REPO/dist-windows/nyaterm-windows-x64.zip"
