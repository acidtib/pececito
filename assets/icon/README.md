# Pececito icon

- `pececito.svg` is the 512 × 512 master.
- `png/pececito-<size>.png` contains the transparent PNG exports at 16, 24, 32, 48, 64, 128, 256, and 512 pixels.

Run from the repository root with `rsvg-convert` installed.

```sh
mkdir -p assets/icon/png
for size in 16 24 32 48 64 128 256 512; do
  rsvg-convert -w "$size" -h "$size" \
    -o "assets/icon/png/pececito-$size.png" assets/icon/pececito.svg || exit 1
done
```
