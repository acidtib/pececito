#!/usr/bin/env sh
# Installs the latest Pececito release for the current user:
#   curl -fsSL https://raw.githubusercontent.com/acidtib/pececito/main/script/install.sh | sh
# PECECITO_VERSION=0.2.0 installs that release instead of the latest; PECECITO_BUNDLE=<path to a
# pececito-linux-x86_64.tar.gz> installs a local build.
set -eu

repo="https://github.com/acidtib/pececito"

main() {
    platform="$(uname -s)"
    arch="$(uname -m)"
    version="${PECECITO_VERSION:-latest}"

    case "$arch" in
        x86_64 | amd64) arch="x86_64" ;;
        *)
            echo "There's no Pececito build for $arch yet."
            exit 1
            ;;
    esac

    if command -v curl >/dev/null 2>&1; then
        download() { curl -fL --progress-bar "$1" -o "$2"; }
    elif command -v wget >/dev/null 2>&1; then
        download() { wget -q --show-progress -O "$2" "$1"; }
    else
        echo "Install curl or wget, then run this again."
        exit 1
    fi

    if [ "$version" = "latest" ]; then
        base="$repo/releases/latest/download"
    else
        base="$repo/releases/download/v${version#v}"
    fi

    temp="$(mktemp -d "${TMPDIR:-/tmp}/pececito-XXXXXX")"
    trap 'rm -rf "$temp"' EXIT

    case "$platform" in
        Linux) linux ;;
        Darwin) macos ;;
        *)
            echo "There's no Pececito build for $platform. On Windows, download Pececito-x86_64.exe from $repo/releases."
            exit 1
            ;;
    esac
}

linux() {
    app="$HOME/.local/pececito.app"
    if [ -n "${PECECITO_BUNDLE:-}" ]; then
        cp "$PECECITO_BUNDLE" "$temp/pececito.tar.gz"
    else
        echo "Downloading Pececito ($version)"
        download "$base/pececito-linux-$arch.tar.gz" "$temp/pececito.tar.gz"
    fi

    rm -rf "$app"
    mkdir -p "$HOME/.local"
    tar -xzf "$temp/pececito.tar.gz" -C "$HOME/.local"

    mkdir -p "$HOME/.local/bin" "$HOME/.local/share/applications"
    ln -sf "$app/bin/pececito" "$HOME/.local/bin/pececito"

    # Launchers run Exec with the session's PATH, which often lacks ~/.local/bin; use absolute paths.
    desktop="$HOME/.local/share/applications/pececito.desktop"
    sed -e "s|^Exec=.*|Exec=$app/bin/pececito|" \
        -e "s|^Icon=.*|Icon=$app/share/icons/hicolor/512x512/apps/pececito.png|" \
        "$app/share/applications/pececito.desktop" > "$desktop"
    chmod 644 "$desktop"

    if command -v update-desktop-database >/dev/null 2>&1; then
        update-desktop-database -q "$HOME/.local/share/applications" || echo "The launcher menu wasn't refreshed; it updates on your next login."
    fi
    if command -v kbuildsycoca6 >/dev/null 2>&1; then
        kbuildsycoca6 --noincremental >/dev/null 2>&1 || echo "The KDE menu wasn't refreshed; it updates on your next login."
    fi

    remember_window_position_on_kde

    echo "Pececito is installed. Open it from your app launcher, or run 'pececito'."
    case ":$PATH:" in
        *":$HOME/.local/bin:"*) ;;
        *) echo "To run 'pececito' from a terminal, add ~/.local/bin to your PATH." ;;
    esac
}

# Wayland ignores window positions from apps, so on KDE a window rule has KWin remember Pececito's.
remember_window_position_on_kde() {
    if ! command -v kwriteconfig6 >/dev/null 2>&1 || ! command -v kreadconfig6 >/dev/null 2>&1; then
        return 0
    fi
    rule="pececito-position"
    rules="$(kreadconfig6 --file kwinrulesrc --group General --key rules)"
    case ",$rules," in
        *",$rule,"*) ;;
        *) rules="${rules:+$rules,}$rule" ;;
    esac
    kwriteconfig6 --file kwinrulesrc --group "$rule" --key Description "Pececito: remember position"
    kwriteconfig6 --file kwinrulesrc --group "$rule" --key wmclass "pececito"
    kwriteconfig6 --file kwinrulesrc --group "$rule" --key wmclassmatch 1
    kwriteconfig6 --file kwinrulesrc --group "$rule" --key positionrule 4
    kwriteconfig6 --file kwinrulesrc --group General --key rules "$rules"
    kwriteconfig6 --file kwinrulesrc --group General --key count "$(echo "$rules" | tr ',' '\n' | grep -c .)"
    if command -v qdbus6 >/dev/null 2>&1; then
        qdbus6 org.kde.KWin /KWin reconfigure >/dev/null 2>&1 || echo "KWin didn't reload its window rules; they apply on your next login."
    fi
}

macos() {
    echo "There's no macOS build of Pececito yet."
    exit 1
}

main "$@"
