# Pececito icon

- `pececito.svg` is the 512 × 512 master.
- `png/pececito-<size>.png` contains the transparent PNG exports at 16, 24, 32, 48, 64, 128, 256, and 512 pixels.
- `pececito.ico` holds the 16 to 256 pixel PNGs; the Windows build embeds it in `pececito.exe`.

Run from the repository root with `rsvg-convert` installed.

```sh
mkdir -p assets/icon/png
for size in 16 24 32 48 64 128 256 512; do
  rsvg-convert -w "$size" -h "$size" \
    -o "assets/icon/png/pececito-$size.png" assets/icon/pececito.svg || exit 1
done
```

Rebuild the Windows icon from the PNGs with ImageMagick.

```sh
magick assets/icon/png/pececito-{16,24,32,48,64,128,256}.png assets/icon/pececito.ico
```
