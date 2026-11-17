#!/bin/sh
# BetterGameExperience installer for Linux (Steam / Proton).
#
#   curl -fsSL https://github.com/etbala/Civ6-BetterGameExperience/releases/latest/download/install.sh | sh
#
# Downloads the static Linux binary to a temporary directory and runs its `install`
# command. The binary copies itself to ${XDG_DATA_HOME:-~/.local/share}/BetterGameExperience
# and verifies the signed manifest and every file it installs; this script does nothing
# security-relevant beyond fetching it over HTTPS. The temporary download is deleted afterwards.
#
# Extra arguments are passed to the binary:   sh install.sh --game-dir /path/to/game
# Environment:
#   BGE_CHANNEL=prerelease   install from the pre-release channel instead of the latest release
set -eu

REPO="etbala/Civ6-BetterGameExperience"
ASSET="BetterGameExperience-linux-x86_64"

die() {
    printf 'install.sh: %s\n' "$*" >&2
    exit 1
}

[ "$(uname -s)" = "Linux" ] || die "Linux only (found $(uname -s))."
case "$(uname -m)" in
    x86_64 | amd64) ;;
    *) die "x86_64 only (found $(uname -m))." ;;
esac

if command -v curl >/dev/null 2>&1; then
    fetch() { curl -fsSL --retry 3 "$@"; }
elif command -v wget >/dev/null 2>&1; then
    fetch() { wget -qO- "$@"; }
else
    die "curl or wget is required."
fi

channel="${BGE_CHANNEL:-release}"
case "$channel" in
    release) url="https://github.com/$REPO/releases/latest/download/$ASSET" ;;
    prerelease)
        # The newest release of any kind: the first matching asset in the API list.
        url=$(fetch "https://api.github.com/repos/$REPO/releases?per_page=10" |
            sed -n 's/.*"browser_download_url": *"\([^"]*\/'"$ASSET"'\)".*/\1/p' | head -n 1)
        [ -n "$url" ] || die "no pre-release found with a Linux binary."
        ;;
    *) die "BGE_CHANNEL must be 'release' or 'prerelease'." ;;
esac

tmp=$(mktemp -d "${TMPDIR:-/tmp}/bge-install.XXXXXX")
trap 'rm -rf "$tmp"' EXIT HUP INT TERM
bin="$tmp/BetterGameExperience"

printf 'Downloading %s\n' "$url"
fetch "$url" >"$bin" || die "download failed."
[ -s "$bin" ] || die "downloaded file is empty."
chmod 700 "$bin"

# Not exec: the trap must still remove the temporary directory.
if [ "$channel" = "prerelease" ]; then
    "$bin" channel prerelease
fi
"$bin" install "$@"
