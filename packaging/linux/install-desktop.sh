#!/usr/bin/env bash
# Add Open Ultima8 to your desktop's app menu (for you only, from this folder): its launcher and
# icons in ~/.local/share. Run it again if you move the folder; --remove takes them out.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
data="${XDG_DATA_HOME:-$HOME/.local/share}"
apps="$data/applications"
if [[ ${1:-} == --remove ]]; then
    rm -f "$apps/open-ultima8.desktop"
    find "$data/icons/hicolor" -name open-ultima8.png -delete 2>/dev/null || true
    echo "Removed."
    exit
fi
mkdir -p "$apps"
for d in "$here"/icons/hicolor/*/apps; do
    size="$(basename "$(dirname "$d")")"
    mkdir -p "$data/icons/hicolor/$size/apps"
    cp "$d/open-ultima8.png" "$data/icons/hicolor/$size/apps/"
done
# (Started from this folder, so the game finds U8/ next to it.)
sed -e "s|^Exec=.*|Exec=\"$here/u8view\"|" -e "s|^Icon=.*|Icon=open-ultima8|" \
    -e "/^Exec=/a Path=$here" "$here/open-ultima8.desktop" > "$apps/open-ultima8.desktop"
command -v update-desktop-database >/dev/null && update-desktop-database "$apps" 2>/dev/null || true
command -v gtk-update-icon-cache >/dev/null && gtk-update-icon-cache -q "$data/icons/hicolor" 2>/dev/null || true
echo "Open Ultima8 is in your app menu (launches $here/u8view)."
