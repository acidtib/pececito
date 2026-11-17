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

    # Unpack first, so a broken download leaves the installed app alone.
    mkdir -p "$temp/unpacked" "$HOME/.local"
    tar -xzf "$temp/pececito.tar.gz" -C "$temp/unpacked"
    rm -rf "$app"
    mv "$temp/unpacked/pececito.app" "$app"

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

    remove_kde_window_rule || echo "The old KDE window rule 'Pececito: remember position' wasn't removed; delete it in System Settings, Window Rules, so Pececito reopens where you left it."

    echo "Pececito is installed. Open it from your app launcher, or run 'pececito'."
    case ":$PATH:" in
        *":$HOME/.local/bin:"*) ;;
        *) echo "To run 'pececito' from a terminal, add ~/.local/bin to your PATH." ;;
    esac
}

# Earlier installs added a KWin rule to remember the window's position. It kept the window
# centered instead, and Pececito now saves its own position.
remove_kde_window_rule() {
    if ! command -v kwriteconfig6 >/dev/null 2>&1 || ! command -v kreadconfig6 >/dev/null 2>&1; then
        return 0
    fi
    rule="pececito-position"
    found=""
    # KWin 6 keeps the rule list in Order; older versions in rules and count.
    for key in Order rules; do
        list="$(kreadconfig6 --file kwinrulesrc --group General --key "$key")" || return 1
        case ",$list," in
            *",$rule,"*) ;;
            *) continue ;;
        esac
        found=1
        list="$(echo "$list" | tr ',' '\n' | grep -vx "$rule" | paste -sd, -)"
        kwriteconfig6 --file kwinrulesrc --group General --key "$key" "$list" || return 1
        if [ "$key" = "rules" ]; then
            kwriteconfig6 --file kwinrulesrc --group General --key count "$(echo "$list" | tr ',' '\n' | grep -c .)" || return 1
        fi
    done
    [ -n "$found" ] || return 0
    for key in Description wmclass wmclassmatch positionrule position; do
        kwriteconfig6 --file kwinrulesrc --group "$rule" --key "$key" --delete || return 1
    done
    if command -v qdbus6 >/dev/null 2>&1; then
        qdbus6 org.kde.KWin /KWin reconfigure >/dev/null 2>&1 || true
    fi
}

macos() {
    echo "There's no macOS build of Pececito yet."
    exit 1
}

main "$@"
