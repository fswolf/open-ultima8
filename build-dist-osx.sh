#!/usr/bin/env bash
# Build u8view and u8tool for macOS from Linux, as one universal binary each (Intel and Apple
# Silicon), ad-hoc signed so Apple Silicon will run them: dist/macos/ with "Open Ultima8.app"
# (the game, with its icon), u8tool, and a .tar.gz.
# Needs:
#   rustup target add x86_64-apple-darwin aarch64-apple-darwin
#   zig on PATH, and cargo install cargo-zigbuild
#   SDKROOT=/path/to/MacOSX.sdk   (Apple's SDK, from Xcode; wgpu/winit link its Metal/AppKit)
#   cargo install apple-codesign  (rcodesign: the ad-hoc signature)
# Unnotarized: Mac users right-click → Open the first time.
set -euo pipefail
cd "$(dirname "$0")"

BINS=(u8view u8tool)
VER=$(sed -n 's/^version *= *"\(.*\)"/\1/p' Cargo.toml | head -1)
T=universal2-apple-darwin
NAME="open-ultima8-$VER-macos"
OUT="dist/macos/$NAME"

die() { echo "!! $*" >&2; exit 1; }
have() { command -v "$1" >/dev/null 2>&1; }

# (Fedora's packaged Rust has no Apple targets: these need rustup's toolchain.)
for a in x86_64-apple-darwin aarch64-apple-darwin; do
    [[ -d "$(rustc --print sysroot)/lib/rustlib/$a/lib" ]] || die "needs rustup's Rust with: rustup target add x86_64-apple-darwin aarch64-apple-darwin"
done
have zig || die "needs zig on PATH (sudo dnf install zig)"
have cargo-zigbuild || die "needs cargo-zigbuild (cargo install cargo-zigbuild)"
[[ -n ${SDKROOT:-} && -d ${SDKROOT:-} ]] || die "set SDKROOT to a MacOSX.sdk"
have rcodesign || die "needs rcodesign to sign (cargo install apple-codesign)"

cargo zigbuild --release --target $T $(for b in "${BINS[@]}"; do printf -- '-p %s ' "$b"; done)

rm -rf dist/macos && mkdir -p "$OUT"
# The game as an app (with its icon); put U8/ next to it. u8tool stays a plain command.
APP="$OUT/Open Ultima8.app"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "target/$T/release/u8view" "$APP/Contents/MacOS/"
cp icon/open-u8.icns "$APP/Contents/Resources/"
sed "s/@VERSION@/$VER/" packaging/macos/Info.plist > "$APP/Contents/Info.plist"
rcodesign sign "$APP" >/dev/null
cp "target/$T/release/u8tool" "$OUT/"
rcodesign sign "$OUT/u8tool" >/dev/null
for f in README.md LICENSE ARCHITECTURE.md crates/u8view/assets/OFL.txt; do [[ -f $f ]] && cp "$f" "$OUT/"; done
tar -C dist/macos -czf "dist/macos/$NAME.tar.gz" "$NAME"
echo "== ok → $OUT (and $NAME.tar.gz)"
