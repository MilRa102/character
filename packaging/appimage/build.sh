#!/usr/bin/env bash
# Builds dist/Character-<version>-x86_64.AppImage.
#
# The same script runs in CI (.github/workflows/appimage.yml) and locally.
# Needs: cargo, curl, sha256sum, file (required by appimagetool);
# desktop-file-validate is used when available.
# The packaging tools are pinned by release tag and sha256 and cached in target/appimage/tools.
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/../.."

if [[ "$(uname -m)" != x86_64 ]]; then
    echo "only x86_64 is supported" >&2
    exit 1
fi

LINUXDEPLOY_URL=https://github.com/linuxdeploy/linuxdeploy/releases/download/1-alpha-20251107-1/linuxdeploy-x86_64.AppImage
LINUXDEPLOY_SHA256=c20cd71e3a4e3b80c3483cef793cda3f4e990aca14014d23c544ca3ce1270b4d
APPIMAGETOOL_URL=https://github.com/AppImage/appimagetool/releases/download/1.9.1/appimagetool-x86_64.AppImage
APPIMAGETOOL_SHA256=ed4ce84f0d9caff66f50bcca6ff6f35aae54ce8135408b3fa33abfc3cb384eb0
# Statically linked runtime: the AppImage does not need libfuse2 on the target system.
RUNTIME_URL=https://github.com/AppImage/type2-runtime/releases/download/20251108/runtime-x86_64
RUNTIME_SHA256=2fca8b443c92510f1483a883f60061ad09b46b978b2631c807cd873a47ec260d

work=target/appimage
tools=$work/tools
appdir=$work/AppDir
dist=dist
desktop_file=packaging/appimage/character.desktop
icon_file=packaging/appimage/character.svg

fetch() {
    local url=$1 sha256=$2 dest=$3
    if [[ -f $dest ]] && echo "$sha256  $dest" | sha256sum --check --status; then
        return
    fi
    curl --fail --silent --show-error --location --retry 3 --output "$dest" "$url"
    if ! echo "$sha256  $dest" | sha256sum --check --status; then
        echo "checksum mismatch for $url" >&2
        exit 1
    fi
    chmod +x "$dest"
}

mkdir -p "$tools" "$dist"
fetch "$LINUXDEPLOY_URL" "$LINUXDEPLOY_SHA256" "$tools/linuxdeploy-x86_64.AppImage"
fetch "$APPIMAGETOOL_URL" "$APPIMAGETOOL_SHA256" "$tools/appimagetool-x86_64.AppImage"
fetch "$RUNTIME_URL" "$RUNTIME_SHA256" "$tools/runtime-x86_64"

if command -v desktop-file-validate >/dev/null; then
    desktop-file-validate "$desktop_file"
else
    echo "warning: desktop-file-validate not found, skipping validation" >&2
fi

cargo build --release --locked --package character
binary=target/release/character
version=$("$binary" --version | cut -d' ' -f2)
output=$dist/Character-$version-x86_64.AppImage

# The tools are AppImages themselves; extract-and-run works without FUSE (containers, CI).
export APPIMAGE_EXTRACT_AND_RUN=1
# The release profile already strips the binary, and linuxdeploy's bundled strip is old.
export NO_STRIP=1
export ARCH=x86_64

rm -rf "$appdir"
"$tools/linuxdeploy-x86_64.AppImage" \
    --appdir "$appdir" \
    --executable "$binary" \
    --desktop-file "$desktop_file" \
    --icon-file "$icon_file"

"$tools/appimagetool-x86_64.AppImage" \
    --runtime-file "$tools/runtime-x86_64" \
    --no-appstream \
    "$appdir" "$output"

echo "built $output"
