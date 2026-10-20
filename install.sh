#!/bin/sh
# Installs the prebuilt saaio binary for this machine from its GitHub release:
#
#   curl -fsSL https://github.com/JanSuthacheeva/saaio/releases/latest/download/install.sh | sh
#
# Environment:
#   SAAIO_VERSION       the tag to install, such as v0.1.0; default: the latest release
#   SAAIO_INSTALL_DIR   the folder saaio goes in; default: ~/.local/bin
#   SAAIO_DOWNLOAD_URL  the folder holding the release assets; default: the release on GitHub
#
# It downloads saaio-<target>.tar.gz and SHA256SUMS, checks the tarball against its checksum and
# replaces saaio in the folder. It edits no shell startup file.

set -eu

REPO=https://github.com/JanSuthacheeva/saaio

fail() {
    echo "saaio: $*" >&2
    exit 1
}

target() {
    os=$(uname -s)
    arch=$(uname -m)
    case "$os $arch" in
    "Darwin arm64") echo aarch64-apple-darwin ;;
    "Darwin x86_64")
        # An arm64 Mac running this shell under Rosetta still gets the native binary.
        if [ "$(sysctl -n sysctl.proc_translated 2>/dev/null)" = 1 ]; then
            echo aarch64-apple-darwin
        else
            echo x86_64-apple-darwin
        fi
        ;;
    "Linux x86_64" | "Linux amd64") echo x86_64-unknown-linux-musl ;;
    "Linux aarch64" | "Linux arm64") echo aarch64-unknown-linux-musl ;;
    *) fail "no prebuilt binary for $os $arch. Build it with cargo install (docs/install.md)." ;;
    esac
}

base_url() {
    if [ -n "${SAAIO_DOWNLOAD_URL:-}" ]; then
        echo "$SAAIO_DOWNLOAD_URL"
    elif [ -n "${SAAIO_VERSION:-}" ]; then
        echo "$REPO/releases/download/$SAAIO_VERSION"
    else
        echo "$REPO/releases/latest/download"
    fi
}

fetch() {
    if [ -n "${SAAIO_DOWNLOAD_URL:-}" ]; then
        curl -fsSL -o "$2" "$1" || fail "could not download $1"
    else
        curl --proto =https --tlsv1.2 -fsSL -o "$2" "$1" || fail "could not download $1"
    fi
}

sha256() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | cut -d ' ' -f 1
    else
        shasum -a 256 "$1" | cut -d ' ' -f 1
    fi
}

verify() {
    name=$(basename "$1")
    expected=$(grep "  $name\$" "$2" | cut -d ' ' -f 1)
    [ -n "$expected" ] && [ "$(sha256 "$1")" = "$expected" ] || fail "checksum mismatch for $name"
}

# Copies beside the old binary and renames over it, so a running saaio server keeps its own.
place() {
    mkdir -p "$2"
    cp "$1" "$2/.saaio.$$"
    chmod 755 "$2/.saaio.$$"
    mv -f "$2/.saaio.$$" "$2/saaio"
}

# The path with the home folder as ~.
shown() {
    case "$1" in
    "$HOME"/*) echo "~${1#"$HOME"}" ;;
    *) echo "$1" ;;
    esac
}

path_hint() {
    case ":$PATH:" in
    *":$1:"*) ;;
    *)
        echo
        echo "$(shown "$1") is not on your PATH. Add this line to your shell's startup file"
        echo "(~/.zshrc, ~/.bashrc, ...) and open a new terminal:"
        echo
        echo "  export PATH=\"$1:\$PATH\""
        ;;
    esac
}

main() {
    command -v curl >/dev/null 2>&1 || fail "install.sh needs curl"
    dir=${SAAIO_INSTALL_DIR:-$HOME/.local/bin}
    tarball="saaio-$(target).tar.gz"
    base=$(base_url)

    tmp=$(mktemp -d)
    trap 'rm -rf "$tmp"' EXIT
    fetch "$base/$tarball" "$tmp/$tarball"
    fetch "$base/SHA256SUMS" "$tmp/SHA256SUMS"
    verify "$tmp/$tarball" "$tmp/SHA256SUMS"
    tar xzf "$tmp/$tarball" -C "$tmp" saaio
    place "$tmp/saaio" "$dir"

    version=$("$dir/saaio" --version) || fail "$(shown "$dir/saaio") does not run on this machine"
    echo "installed $version to $(shown "$dir/saaio")"
    path_hint "$dir"
}

main "$@"
