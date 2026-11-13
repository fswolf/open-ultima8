#!/usr/bin/env bash
# Build release binaries (u8view, u8tool) for Linux and Windows, gathered in dist/<os>/ with an
# archive each. (macOS: build-dist-osx.sh.)
#   ./build-dist.sh           # linux and windows (each skipped with a note if its tools are missing)
#   ./build-dist.sh windows   # just this one
# Linux:   sudo dnf install alsa-lib-devel
# Windows (cross-compiled): sudo dnf install mingw64-gcc mingw64-winpthreads-static, and the target's
#   std: rustup target add x86_64-pc-windows-gnu (rustup's cargo) or
#   sudo dnf install rust-std-static-x86_64-pc-windows-gnu (Fedora's cargo)
set -euo pipefail
cd "$(dirname "$0")"

BINS=(u8view u8tool)
VER=$(sed -n 's/^version *= *"\(.*\)"/\1/p' Cargo.toml | head -1)
DIST=dist
EXTRA=(README.md LICENSE ARCHITECTURE.md crates/u8view/assets/OFL.txt)
declare -A RESULT

have() { command -v "$1" >/dev/null 2>&1; }
# Is the standard library for target $1 installed (by rustup, or Fedora's rust-std-static-* package)?
target_ok() { [[ -d "$(rustc --print sysroot)/lib/rustlib/$1/lib" ]]; }
# How to get it: rustup if cargo is rustup's, else Fedora's package.
get_target() {
    if [[ $(command -v cargo) == */.cargo/bin/* ]] && have rustup; then echo "rustup target add $1"
    else echo "sudo dnf install rust-std-static-$1"; fi
}
bins_args() { for b in "${BINS[@]}"; do printf -- '-p %s ' "$b"; done; }

# Copy the built binaries (with `ext`) from `dir` into dist/<os>/open-ultima8-<ver>-<os>/, then archive.
package() {
    local os=$1 dir=$2 ext=${3:-} name="open-ultima8-$VER-$os"
    local out="$DIST/$os/$name"
    rm -rf "$DIST/$os" && mkdir -p "$out"
    for b in "${BINS[@]}"; do cp "$dir/$b$ext" "$out/"; done
    for f in "${EXTRA[@]}"; do [[ -f $f ]] && cp "$f" "$out/"; done
    if [[ $os == linux ]]; then
        # The app-menu launcher and icons (install-desktop.sh puts them in ~/.local/share).
        cp packaging/linux/open-ultima8.desktop packaging/linux/install-desktop.sh "$out/"
        for s in 16 24 32 48 64 128 256 512; do
            mkdir -p "$out/icons/hicolor/${s}x${s}/apps"
            cp "icon/open-u8-$s.png" "$out/icons/hicolor/${s}x${s}/apps/open-ultima8.png"
        done
    fi
    if [[ $os == windows ]] && have zip; then
        (cd "$DIST/$os" && zip -qr "$name.zip" "$name")
    else
        tar -C "$DIST/$os" -czf "$DIST/$os/$name.tar.gz" "$name"
    fi
    RESULT[$os]="ok → $out"
}

skip() { RESULT[$1]="skipped: $2"; echo "!! $1 skipped: $2" >&2; }

build_linux() {
    echo "== linux"
    if ! pkg-config --exists alsa 2>/dev/null; then
        skip linux "needs ALSA headers for audio (sudo dnf install alsa-lib-devel)"; return
    fi
    cargo build --release $(bins_args)
    package linux target/release
}

build_windows() {
    echo "== windows"
    local t=x86_64-pc-windows-gnu
    target_ok $t || { skip windows "$(get_target $t)"; return; }
    have x86_64-w64-mingw32-gcc || { skip windows "sudo dnf install mingw64-gcc mingw64-winpthreads-static"; return; }
    CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER=x86_64-w64-mingw32-gcc \
        cargo build --release --target $t $(bins_args)
    package windows target/$t/release .exe
}

targets=("$@")
[[ ${#targets[@]} -eq 0 ]] && targets=(linux windows)
for os in "${targets[@]}"; do
    case $os in
        linux) build_linux ;;
        windows) build_windows ;;
        macos | osx) echo "macOS has its own script: ./build-dist-osx.sh" >&2; exit 2 ;;
        *) echo "unknown target: $os (linux, windows)" >&2; exit 2 ;;
    esac
done

echo
echo "== $DIST/"
for os in "${!RESULT[@]}"; do printf '  %-8s %s\n' "$os" "${RESULT[$os]}"; done
