# sixpack Chunk Naming: Reverse-Sorted 3-Character Chunk Files

## Purpose

This document defines a very small chunk naming system for sixpack-style `.6` files.

The goal is simple:

- Chunk filenames are always **3 characters**
- Chunk names use only **lowercase letters and numbers**
- No capitals are used
- Newer chunks should appear **above older chunks** when a folder is sorted normally
- The logic should be driven by one normal integer counter
- The row format inside each `.6` file is not part of this system

This is only about naming chunk files and generation folders.

---

## Core Idea

Use a normal increasing integer counter internally:

```txt
0, 1, 2, 3, 4, ...
```

But encode the visible folder and file names in reverse.

So instead of this:

```txt
000.6
001.6
002.6
003.6
```

We do this:

```txt
zzz.6
zzy.6
zzx.6
zzw.6
```

That means when a file browser sorts names ascending, the newest file naturally appears near the top.

Example:

```txt
table/
  zzw.6   newest
  zzx.6
  zzy.6
  zzz.6   oldest
```

The internal counter still only goes upward.

---

## Character Set

Use lowercase base-36:

```txt
0123456789abcdefghijklmnopqrstuvwxyz
```

That gives:

```txt
36 symbols
```

For a 3-character filename:

```txt
36^3 = 46,656 possible chunk names
```

So each generation folder can hold:

```txt
46,656 chunks
```

No uppercase characters are needed.

---

## Why Reverse Encoding Works

A normal base-36 counter would produce:

```txt
000
001
002
003
...
zzz
```

That is chronological, but old files appear first in ascending folder order.

Instead, we reverse the encoded number:

```txt
visible = max_value - counter
```

For 3-character base-36:

```txt
max_value = 36^3 - 1 = 46,655
```

So:

```txt
counter 0 -> encode(46,655) -> zzz
counter 1 -> encode(46,654) -> zzy
counter 2 -> encode(46,653) -> zzx
counter 3 -> encode(46,652) -> zzw
```

This makes newer chunks get alphabetically smaller names.

---

## Single-Folder Version

The simplest possible system is:

```txt
table/
  zzz.6
  zzy.6
  zzx.6
  zzw.6
  ...
  000.6
```

Translation:

```txt
counter     filename
0           zzz.6
1           zzy.6
2           zzx.6
3           zzw.6
4           zzv.6
5           zzu.6
...
35          zz0.6
36          zyz.6
37          zyy.6
...
46,655      000.6
```

This gives one folder a maximum of:

```txt
46,656 chunks
```

For many use cases, that is already plenty.

But if the table eventually needs more than 46,656 chunks, do not introduce capitals. Use generation folders.

---

## Generation Folders

To keep chunk filenames exactly 3 characters forever, add a folder layer.

Each folder is also reverse-encoded.

Recommended layout:

```txt
table/
  zz/
    zzz.6
    zzy.6
    zzx.6
    ...
    000.6

  zy/
    zzz.6
    zzy.6
    zzx.6
    ...
    000.6

  zx/
    zzz.6
    zzy.6
    zzx.6
    ...
```

The folder name is the generation.

The file name is the local chunk inside that generation.

Both count backward visually.

---

## Recommended Final Scheme

Use:

```txt
2-character generation folder
3-character chunk filename
```

So the path format is:

```txt
<generation>/<chunk>.6
```

Example:

```txt
zz/zzz.6
zz/zzy.6
zz/zzx.6
...
zz/000.6
zy/zzz.6
zy/zzy.6
...
```

The internal counter still only goes:

```txt
0, 1, 2, 3, ...
```

---

## Capacity

Using lowercase base-36:

```txt
alphabet size = 36
generation width = 2
chunk width = 3
```

Chunks per generation:

```txt
36^3 = 46,656
```

Number of generation folders:

```txt
36^2 = 1,296
```

Total chunks:

```txt
1,296 × 46,656 = 60,466,176 chunks
```

So the 2-folder + 3-file scheme gives:

```txt
60,466,176 total chunks
```

without uppercase letters and without widening chunk filenames.

---

## Encoding Model

Given one global chunk counter:

```txt
global_chunk_counter = 0, 1, 2, 3, ...
```

Compute:

```txt
generation = floor(global_chunk_counter / chunks_per_generation)
local_chunk = global_chunk_counter % chunks_per_generation
```

Where:

```txt
chunks_per_generation = 36^3 = 46,656
```

Then reverse-encode both:

```txt
folder = reverse_base36(generation, width = 2)
file = reverse_base36(local_chunk, width = 3)
```

Final path:

```txt
folder/file.6
```

---

## Translation Table

```txt
global counter     generation     local chunk     path
0                  0              0               zz/zzz.6
1                  0              1               zz/zzy.6
2                  0              2               zz/zzx.6
3                  0              3               zz/zzw.6
4                  0              4               zz/zzv.6

46,654             0              46,654          zz/001.6
46,655             0              46,655          zz/000.6

46,656             1              0               zy/zzz.6
46,657             1              1               zy/zzy.6
46,658             1              2               zy/zzx.6

93,312             2              0               zx/zzz.6
93,313             2              1               zx/zzy.6
```

Sorted normally, this puts newer generations above older generations:

```txt
table/
  zx/   newer generation
  zy/
  zz/   older generation
```

Inside each folder, newer chunks also appear above older chunks:

```txt
zx/
  zzy.6   newer
  zzz.6   older
```

Once `zx/zzz.6` exists, `zx/` sorts above `zy/` and `zz/`.

---

## Important Sorting Assumption

This scheme assumes normal ASCII-like ascending sorting where:

```txt
0 < 1 < 2 < ... < 9 < a < b < ... < z
```

Because the names are fixed-width and lowercase-only, this is much safer than mixing uppercase and lowercase.

Avoid uppercase letters because some file explorers and filesystems sort or compare names case-insensitively. That can make ordering less predictable.

---

## Why Not Use Capitals Later?

Using capitals later sounds good, but it introduces sorting problems.

If the system starts with lowercase/digits:

```txt
zzz.6
zzy.6
...
000.6
```

then eventually there is no clean place to add uppercase names above the newest files without depending on case-sensitive sorting behavior.

Also, file explorers may sort uppercase and lowercase differently.

So the clean answer is:

```txt
Do not use capitals.
Use another generation folder instead.
```

This keeps the encoding stable forever.

---

## Why Not Use Wider Chunk Names?

Widening chunk names from 3 characters to 4 characters works technically:

```txt
zzz.6
...
000.6
zzzz.6
zzzy.6
...
```

But mixed filename widths can create awkward folder ordering.

If the hard rule is:

```txt
chunk files are always 3 characters
```

then generation folders are cleaner.

---

## Minimal State

Only store one number per table:

```json
{
  "next_chunk": 0
}
```

When creating a chunk:

```txt
path = chunk_path(next_chunk)
next_chunk += 1
```

That is all.

Example after creating three chunks:

```json
{
  "next_chunk": 3
}
```

Existing chunks:

```txt
zz/zzz.6
zz/zzy.6
zz/zzx.6
```

Next chunk:

```txt
zz/zzw.6
```

---

## TypeScript Implementation

```ts
const CHARS = "0123456789abcdefghijklmnopqrstuvwxyz";

const BASE = CHARS.length; // 36

const GEN_WIDTH = 2;
const CHUNK_WIDTH = 3;

const CHUNKS_PER_GEN = BASE ** CHUNK_WIDTH; // 46,656
const MAX_GENS = BASE ** GEN_WIDTH;         // 1,296
const MAX_CHUNKS = CHUNKS_PER_GEN * MAX_GENS;

function encodeFixed(n: number, width: number): string {
  const max = BASE ** width;

  if (!Number.isInteger(n) || n < 0 || n >= max) {
    throw new Error(`value must be between 0 and ${max - 1}`);
  }

  let out = "";

  for (let i = 0; i < width; i++) {
    const digit = n % BASE;
    out = CHARS[digit] + out;
    n = Math.floor(n / BASE);
  }

  return out;
}

function encodeReverse(n: number, width: number): string {
  const max = BASE ** width;

  if (!Number.isInteger(n) || n < 0 || n >= max) {
    throw new Error(`value must be between 0 and ${max - 1}`);
  }

  return encodeFixed(max - 1 - n, width);
}

export function chunkPath(globalChunkCounter: number): string {
  if (
    !Number.isInteger(globalChunkCounter) ||
    globalChunkCounter < 0 ||
    globalChunkCounter >= MAX_CHUNKS
  ) {
    throw new Error(`chunk counter must be between 0 and ${MAX_CHUNKS - 1}`);
  }

  const generation = Math.floor(globalChunkCounter / CHUNKS_PER_GEN);
  const localChunk = globalChunkCounter % CHUNKS_PER_GEN;

  const folder = encodeReverse(generation, GEN_WIDTH);
  const file = encodeReverse(localChunk, CHUNK_WIDTH);

  return `${folder}/${file}.6`;
}
```

Usage:

```ts
chunkPath(0);      // zz/zzz.6
chunkPath(1);      // zz/zzy.6
chunkPath(2);      // zz/zzx.6
chunkPath(3);      // zz/zzw.6

chunkPath(46655);  // zz/000.6
chunkPath(46656);  // zy/zzz.6
chunkPath(46657);  // zy/zzy.6

chunkPath(93312);  // zx/zzz.6
```

---

## Rust Implementation

```rust
const CHARS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";

const BASE: usize = 36;

const GEN_WIDTH: usize = 2;
const CHUNK_WIDTH: usize = 3;

const CHUNKS_PER_GEN: usize = 36usize.pow(CHUNK_WIDTH as u32);
const MAX_GENS: usize = 36usize.pow(GEN_WIDTH as u32);
const MAX_CHUNKS: usize = CHUNKS_PER_GEN * MAX_GENS;

fn encode_fixed(mut n: usize, width: usize) -> Result<String, String> {
    let max = BASE.pow(width as u32);

    if n >= max {
        return Err(format!("value must be between 0 and {}", max - 1));
    }

    let mut out = vec![b'0'; width];

    for i in (0..width).rev() {
        let digit = n % BASE;
        out[i] = CHARS[digit];
        n /= BASE;
    }

    String::from_utf8(out).map_err(|_| "invalid utf8".to_string())
}

fn encode_reverse(n: usize, width: usize) -> Result<String, String> {
    let max = BASE.pow(width as u32);

    if n >= max {
        return Err(format!("value must be between 0 and {}", max - 1));
    }

    encode_fixed(max - 1 - n, width)
}

pub fn chunk_path(global_chunk_counter: usize) -> Result<String, String> {
    if global_chunk_counter >= MAX_CHUNKS {
        return Err(format!(
            "chunk counter must be between 0 and {}",
            MAX_CHUNKS - 1
        ));
    }

    let generation = global_chunk_counter / CHUNKS_PER_GEN;
    let local_chunk = global_chunk_counter % CHUNKS_PER_GEN;

    let folder = encode_reverse(generation, GEN_WIDTH)?;
    let file = encode_reverse(local_chunk, CHUNK_WIDTH)?;

    Ok(format!("{}/{}.6", folder, file))
}
```

Example outputs:

```rust
assert_eq!(chunk_path(0).unwrap(), "zz/zzz.6");
assert_eq!(chunk_path(1).unwrap(), "zz/zzy.6");
assert_eq!(chunk_path(2).unwrap(), "zz/zzx.6");
assert_eq!(chunk_path(46655).unwrap(), "zz/000.6");
assert_eq!(chunk_path(46656).unwrap(), "zy/zzz.6");
assert_eq!(chunk_path(93312).unwrap(), "zx/zzz.6");
```

---

## Recommended Directory Shape

For a table called `messages`:

```txt
data/
  messages/
    zz/
      zzz.6
      zzy.6
      zzx.6

    zy/
      zzz.6
      zzy.6
```

The folder itself is the generation.

The file is the local chunk.

The full chunk path is:

```txt
data/messages/<generation>/<chunk>.6
```

Example:

```txt
data/messages/zz/zzz.6
data/messages/zz/zzy.6
data/messages/zz/zzx.6
```

---

## Final Rule

Use this as the full chunk naming rule:

```txt
global_chunk_counter starts at 0 and increments normally

generation = global_chunk_counter / 46,656
local_chunk = global_chunk_counter % 46,656

folder = reverse lowercase base36 generation, width 2
file = reverse lowercase base36 local chunk, width 3

path = folder + "/" + file + ".6"
```

This gives:

```txt
zz/zzz.6
zz/zzy.6
zz/zzx.6
...
zz/000.6
zy/zzz.6
zy/zzy.6
...
```

The system is:

- Fast
- Tiny
- Deterministic
- Reversible
- Case-safe
- Fixed-width for chunk filenames
- Based on one normal integer counter
- Able to scale to over 60 million chunks per table
- Sorted so recent chunks appear first in normal folder views

