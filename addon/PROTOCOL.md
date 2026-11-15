# How the addon talks to the app

An addon can't open a socket or write a file the app can read while the game runs. It can draw. The
Pececito addon draws a strip of small squares in the top-left corner of the game window, and the app
captures that corner and reads the squares back.

`Pececito/Pececito.lua` draws the strip. `crates/game/src/addon.rs` reads it.

## Cells

Each square is a cell: one flat color, 3 by 3 screen pixels. A cell carries three bits, one per color
channel, each fully on or off:

| Bit | Channel |
| --- | ------- |
| 4   | red     |
| 2   | green   |
| 1   | blue    |

So a cell holds a number from 0 (black) to 7 (white). The app counts a channel as on when it's over
half, which leaves room for color drift from scaling or filters. It reads each cell at its middle
pixel.

A number larger than 7 takes several cells, most significant first, in base 8. Two cells hold 0 to
63.

## The strip

Cells run left to right from the corner of the window:

| Cells | Holds         | Notes                                                    |
| ----- | ------------- | -------------------------------------------------------- |
| 1     | Magenta (5)   | Marks the strip. Its width tells the app the cell size.  |
| 1     | Cyan (3)      | Marks the strip.                                         |
| 2     | Addon version | The `## Version` in `Pececito.toc`.                      |
| 1     | Player state  | 0 idle, 1 no fishing pole, 2 other cast, 3 fishing.      |
| 1     | Moves         | Goes up by one each time the player starts moving.       |
| 1     | Loot counter  | Goes up by one with each item looted from fishing.       |
| 7     | Loot item     | The id of the last item looted.                          |
| 2     | Loot count    | How many of it, up to 63.                                |
| 2     | Checksum      | See below.                                               |

The two markers and the version never move. Everything after the version belongs to that version:
the app reads it only when the version is the one it ships, and otherwise treats the addon as not
loaded. That is what you see between updating the addon and typing `/reload`.

The checksum covers every cell between the markers and the checksum itself. Number those cells from
1; the checksum is the sum of each cell times its number, modulo 64. The app drops a strip whose
checksum doesn't match, such as one captured while the game was drawing over it.

## Adding a field

1. Add the field's cells before the checksum in `show` in `Pececito.lua`, and raise the number of
   cells the strip creates.
2. Read it at the same place in `decode` in `addon.rs`, and add it to `Frame`.
3. Raise `## Version` in `Pececito.toc` and `VERSION` in `Pececito.lua`. A test in `addon.rs` fails
   until the addon's version and fingerprint are updated.
4. Add the row to the table above.

A value that describes the present, like the player state, needs nothing more. An event, like a
loot, happens once and the app reads the strip ten times a second, so it could miss the event or
read it twice. Send an event as a counter that goes up by one, next to the event's data, and keep
both on the strip until the next event. The app acts when the counter changes.

## Limits

- Text doesn't fit at a useful rate. Send ids and let the app look names up. For item names, the
  addon keeps a table of the items it has seen in its saved variables (`PececitoItems`), which the
  game writes to `WTF/Account/<account>/SavedVariables/Pececito.lua` on `/reload` and logout. The
  app reads that file when a session starts.
- The colors were measured as exact on one setup, Proton on Linux at native resolution. Render
  scaling below 100% blurs 3 pixel cells and hasn't been tested.
- The app can't read the strip while the game window is minimized or its corner is off-screen.
