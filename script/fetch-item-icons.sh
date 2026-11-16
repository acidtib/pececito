#!/usr/bin/env sh
# Looks up the Wowhead icon of each item in crates/game/src/fishing_loot.tsv, writes its name as
# the third column and downloads each icon once into assets/item-icons. Run from the repo root
# after adding items to the list:
#   script/fetch-item-icons.sh
set -eu

list="crates/game/src/fishing_loot.tsv"
icons="assets/item-icons"
tooltip="https://nether.wowhead.com/forever/tooltip/item"
images="https://wow.zamimg.com/images/wow/icons/large"

main() {
    for tool in curl jq; do
        if ! command -v "$tool" >/dev/null 2>&1; then
            echo "This script needs $tool."
            exit 1
        fi
    done
    mkdir -p "$icons"
    updated="$(mktemp)"
    trap 'rm -f "$updated"' EXIT

    while IFS= read -r line; do
        case "$line" in
            "#"* | "")
                printf '%s\n' "$line" >>"$updated"
                continue
                ;;
        esac
        item="$(printf '%s' "$line" | cut -f1)"
        name="$(printf '%s' "$line" | cut -f2)"
        icon="$(curl -fsS "$tooltip/$item" | jq -r '.icon // empty')"
        # The name becomes a file name, so take only what Wowhead's icon names look like.
        if ! printf '%s' "$icon" | grep -Eq '^[a-z0-9_-]+$'; then
            echo "No icon for item $item ($name)."
            printf '%s\t%s\n' "$item" "$name" >>"$updated"
            continue
        fi
        if [ ! -f "$icons/$icon.jpg" ]; then
            curl -fsS -o "$icons/$icon.jpg" "$images/$icon.jpg"
        fi
        printf '%s\t%s\t%s\n' "$item" "$name" "$icon" >>"$updated"
    done <"$list"

    mv "$updated" "$list"
    trap - EXIT
}

main "$@"
