# Tensack `.ten` Format Specification

**Status:** draft background reference. Current file/storage decisions live in
`TENSACK_STORAGE_SPEC.md`; current product/backend decisions live in
`TENSACK_BOOK.md`.

**Document version:** Draft v0.1  
**Format name:** TEN - Tensack Entity Notation  
**Primary file extensions:** `.ten`, `.tenb`, `.tenx`  
**Primary design decision:** `.ten` is the only canonical human-readable source format. `.tenb` and `.tenx` are generated caches.

---

## 1. Executive summary

Tensack should not use verbose JSONL as its canonical human-readable data format. JSONL is structurally convenient, but for high-volume chat/entity data it repeats keys such as `role`, `content`, `messages`, `metadata`, `source`, and `domain` on every object. That is avoidable overhead.

Tensack should also avoid exposing plain `.tsv` as the brand-level format. The underlying parsing model can still use tab-separated fields because tabs are fast and simple, but the user-facing format should be a Tensack-specific `.ten` specification with strict rules, typed records, block structure, escaping, validation, and generated acceleration caches.

The simplified architecture is:

```text
.ten   = canonical human-readable data
.tenb  = generated opaque binary cache, including indexes and fast-read layout
.tenx  = optional generated full-text search index
```

The key rule is:

```text
.ten is truth.
.tenb and .tenx are disposable generated acceleration files.
```

If `.tenb` or `.tenx` is missing, stale, corrupted, or incompatible, Tensack must rebuild it from `.ten`.

---

## 2. What needs to change

### 2.1 Stop using verbose JSONL as canonical storage

Do not store canonical chat data as repeated object records like this:

```json
{"id":"1","messages":[{"role":"user","content":"What is ATP?"},{"role":"assistant","content":"ATP is the main energy-carrying molecule in cells."}]}
```

This repeats structural strings over and over:

```text
id
messages
role
content
user
assistant
```

JSONL may remain an import/export compatibility format, but it should not be the canonical Tensack source format.

### 2.2 Do not expose generic `.tsv`

The format should not be branded or documented as `.tsv`. Tensack should define `.ten` as its own format.

Internally, `.ten` uses literal tab bytes as structural separators because they are fast to parse. But `.ten` is not generic TSV. It has:

```text
magic headers
record tags
block structure
tail fields
strict escaping
typed numeric fields
role/source/domain/tag dictionaries
validation rules
generated .tenb caches
optional .tenx search indexes
```

### 2.3 Use block-packed records instead of repeated message rows

Do not store every message as a global row that repeats the conversation id:

```text
1<TAB>0<TAB>s<TAB>You are a biology tutor.
1<TAB>1<TAB>u<TAB>What is ATP?
1<TAB>2<TAB>a<TAB>ATP is the main energy-carrying molecule in cells.
```

Use a block-packed structure:

```text
C<TAB>1<TAB>1<TAB>2<TAB>0
M<TAB>0<TAB>s<TAB>You are a biology tutor.
M<TAB>1<TAB>u<TAB>What is ATP?
M<TAB>2<TAB>a<TAB>ATP is the main energy-carrying molecule in cells.
```

Inside a block, the current `cid` is implicit. This saves space and makes reading faster.

### 2.4 Merge all indexing into `.tenb`

Do not create separate visible index files such as `.teni`.

The generated `.tenb` file should contain all fast-read structures:

```text
source hash
schema version
entity offset index
entity metadata arrays
message arrays
content blob
id lookup table
optional compression block directory
```

### 2.5 Keep `.tenx` optional

Only create `.tenx` if full-text search matters. Exact lookup, metadata filtering, and normal reads should use `.tenb`.

---

## 3. File roles

### 3.1 `.ten`

`.ten` is the canonical human-readable source file. It is editable, reviewable, diffable, and versionable.

Rules:

```text
A valid Tensack dataset must be recoverable from .ten alone.
.tenb and .tenx must never contain irreplaceable source data.
```

### 3.2 `.tenb`

`.tenb` is an opaque generated binary cache. It is used for speed.

It should contain:

```text
compiled entity arrays
compiled message arrays
decoded content blobs
lookup indexes
source hash
schema hash/version
optional compression block index
```

It may be deleted at any time. Tensack must rebuild it from `.ten`.

### 3.3 `.tenx`

`.tenx` is an optional generated search index.

It should contain:

```text
term dictionary
postings lists
content/document references
source hash
schema hash/version
```

It may be custom, SQLite FTS-based, or another internal search representation. The extension remains `.tenx` so the implementation is abstracted from users.

---

## 4. High-level `.ten` syntax outline

Visual notation in this document:

```text
<TAB> means one literal horizontal tab byte, U+0009.
<LF> means one literal line feed byte, U+000A.
```

A minimal chatpack file has this shape:

```text
TEN<TAB>1<TAB>chatpack<TAB>tensack-chat-v1
@role<TAB>s<TAB>system
@role<TAB>u<TAB>user
@role<TAB>a<TAB>assistant
@source<TAB>1<TAB>biology_notes
@domain<TAB>1<TAB>science
@domain<TAB>2<TAB>biology

C<TAB>1<TAB>1<TAB>2<TAB>0
M<TAB>0<TAB>s<TAB>You are a biology tutor.
M<TAB>1<TAB>u<TAB>What is ATP?
M<TAB>2<TAB>a<TAB>ATP is the main energy-carrying molecule in cells.
```

The core syntax categories are:

```text
magic line       TEN<TAB>version<TAB>profile<TAB>schema_id
directive line   @name<TAB>fields...
comment line     # text ignored by parser
blank line       ignored
block start      C<TAB>cid<TAB>sid<TAB>did<TAB>flags
message record   M<TAB>turn<TAB>role<TAB>content_tail
tag record       G<TAB>tag_id
property record  P<TAB>key<TAB>value_tail
tool record      T<TAB>turn<TAB>call<TAB>tool_id<TAB>args_tail
result record    R<TAB>turn<TAB>call<TAB>content_tail
```

---

## 5. Core lexical rules

### 5.1 Encoding

Every `.ten` file must use:

```text
UTF-8 encoding
LF line endings
No BOM
```

A strict parser must reject:

```text
invalid UTF-8
unescaped raw CR characters
unknown escape sequences
malformed numeric fields
malformed record fields
```

### 5.2 Physical lines

A `.ten` file is made of physical lines. One physical line is one record, directive, comment, blank line, or magic line.

Raw line feeds are record boundaries. User content must not contain raw LF bytes. Store line feeds inside content as `\n`.

### 5.3 Tabs

Literal tabs are structural separators between fields.

Regular fields cannot contain raw tabs. If a regular field must represent a tab, it must use `\t`.

Tail fields are different: a tail field is the final field of a record. Tail fields may contain raw tabs because parsing has already consumed the fixed prefix fields.

Example:

```text
M<TAB>0<TAB>u<TAB>This tail field may contain<TAB>a raw tab.
```

The parser reads:

```text
tag     = M
turn    = 0
role    = u
content = This tail field may contain<TAB>a raw tab.
```

### 5.4 Whitespace

A parser must not trim field values.

These are different content values:

```text
hello
 hello
hello 
```

Spaces are data. Tabs are delimiters except inside tail fields.

### 5.5 Comments and blank lines

A line whose first byte is `#` is a comment and is ignored.

A blank line is ignored.

A `#` inside a tail content field is not a comment because the record starts with `M`, `P`, `T`, or `R`, not `#`.

---

## 6. Escaping rules

Escaping is backslash-based.

Supported escape sequences:

```text
\\   literal backslash
\n   line feed
\r   carriage return
\t   horizontal tab
\0   NUL byte, optional support; strict implementations may reject NUL
\N   null value, only when the entire raw field is exactly \N
```

Important null rule:

```text
raw field exactly \N  = null
raw field exactly \\N = literal string "\N"
```

Unknown escape sequences are invalid in strict mode.

The parser must split fields before unescaping. Never unescape before finding structural tabs.

Recommended unescape algorithm:

```text
1. If the raw field is exactly \N, return null.
2. If the raw field contains no backslash, return the raw field as-is.
3. Otherwise scan left to right.
4. Convert only known escape sequences.
5. Reject dangling backslashes and unknown escapes.
```

Example original content:

```text
Line one
Line two
```

Stored in `.ten`:

```text
Line one\nLine two
```

Example original code block with indentation:

~~~~text
Can you fix this?

```python
def add(a, b):
    return a + b
```
~~~~

Stored as a message tail:

```text
M<TAB>0<TAB>u<TAB>Can you fix this?\n\n```python\ndef add(a, b):\n    return a + b\n```
```

---

## 7. Magic line

Every `.ten` file must begin with a magic line.

Form:

```text
TEN<TAB><version><TAB><profile><TAB><schema_id>
```

For the first chat format:

```text
TEN<TAB>1<TAB>chatpack<TAB>tensack-chat-v1
```

Fields:

```text
TEN        literal magic token
version    unsigned integer format version
profile    profile name, such as chatpack
schema_id  schema identifier string
```

A parser must reject files whose first non-empty line is not a valid magic line.

---

## 8. Directives

Directive lines start with `@`.

Directives define small dictionaries and file-level metadata. They are usually near the top of the file.

### 8.1 `@meta`

File-level metadata.

Form:

```text
@meta<TAB><key><TAB><value_tail>
```

Example:

```text
@meta<TAB>name<TAB>biology-chat-sample
@meta<TAB>created_by<TAB>tensack
@meta<TAB>description<TAB>Small sample dataset for parser tests.
```

### 8.2 `@role`

Role dictionary entry.

Form:

```text
@role<TAB><role_code><TAB><role_name>
```

Recommended defaults:

```text
@role<TAB>s<TAB>system
@role<TAB>u<TAB>user
@role<TAB>a<TAB>assistant
@role<TAB>t<TAB>tool
@role<TAB>d<TAB>developer
```

### 8.3 `@source`

Source dictionary entry.

Form:

```text
@source<TAB><sid><TAB><source_name_tail>
```

Example:

```text
@source<TAB>1<TAB>biology_notes
@source<TAB>2<TAB>physics_notes
```

### 8.4 `@domain`

Domain dictionary entry.

Form:

```text
@domain<TAB><did><TAB><domain_name_tail>
```

Example:

```text
@domain<TAB>1<TAB>science
@domain<TAB>2<TAB>biology
@domain<TAB>3<TAB>physics
```

### 8.5 `@tag`

Tag dictionary entry.

Form:

```text
@tag<TAB><tag_id><TAB><tag_name_tail>
```

Example:

```text
@tag<TAB>1<TAB>biology
@tag<TAB>2<TAB>cell_energy
@tag<TAB>3<TAB>education
```

### 8.6 `@tool`

Tool dictionary entry for tool/subrecord data.

Form:

```text
@tool<TAB><tool_id><TAB><tool_name_tail>
```

Example:

```text
@tool<TAB>1<TAB>calculator
@tool<TAB>2<TAB>search
```

---

## 9. Chatpack record syntax

The `chatpack` profile stores one conversation/entity as one contiguous block.

A block starts with `C`. Child records following it belong to that block until the next `C` record or EOF.

### 9.1 `C` - conversation/entity block header

Form:

```text
C<TAB><cid><TAB><sid><TAB><did><TAB><flags>
```

Fields:

```text
cid    unsigned 64-bit entity/conversation id
sid    unsigned 32-bit source id, declared by @source; 0 means unknown/none
did    unsigned 32-bit domain id, declared by @domain; 0 means unknown/none
flags  unsigned 32-bit bitset; 0 means no flags
```

Example:

```text
C<TAB>1<TAB>1<TAB>2<TAB>0
```

### 9.2 `M` - message/content record

Form:

```text
M<TAB><turn><TAB><role><TAB><content_tail>
```

Fields:

```text
turn          unsigned 32-bit turn number inside the current block
role          role code declared by @role
content_tail  escaped text tail; may contain raw tabs; must not contain raw LF
```

Example:

```text
M<TAB>0<TAB>u<TAB>What is ATP?
M<TAB>1<TAB>a<TAB>ATP is the main energy-carrying molecule in cells.
```

### 9.3 `G` - tag link

Associates the current block with a declared tag.

Form:

```text
G<TAB><tag_id>
```

Example:

```text
G<TAB>1
G<TAB>2
```

### 9.4 `P` - property record

Adds arbitrary metadata to the current block.

Form:

```text
P<TAB><key><TAB><value_tail>
```

Example:

```text
P<TAB>license<TAB>internal
P<TAB>difficulty<TAB>2
P<TAB>review_status<TAB>verified
```

`P` is intentionally low-volume. High-volume, frequently filtered fields should become numeric fields on `C` or dictionary references, not repeated `P` strings.

### 9.5 `T` - tool/subrecord call

Stores a tool call or structured subrecord linked to the current block.

Form:

```text
T<TAB><turn><TAB><call><TAB><tool_id><TAB><args_tail>
```

Fields:

```text
turn       turn number associated with the call
call       call index inside that turn
tool_id    declared by @tool
args_tail  opaque escaped text tail
```

Example:

```text
T<TAB>1<TAB>0<TAB>1<TAB>expression=12*19
```

The `args_tail` field is application-defined. It can use compact key-value syntax, a custom Tensack subformat, or JSON inside the field if absolutely necessary. The outer `.ten` container should remain line-oriented and non-JSON.

### 9.6 `R` - tool/subrecord result

Stores a result associated with a tool/subrecord call.

Form:

```text
R<TAB><turn><TAB><call><TAB><content_tail>
```

Example:

```text
R<TAB>2<TAB>0<TAB>228
```

---

## 10. Optional update-log records

Canonical `.ten` files can be immutable block files. If update logs are needed, use an update-log profile or append region.

### 10.1 `D` - tombstone/delete

Form:

```text
D<TAB><cid>
```

Meaning:

```text
Mark entity/conversation cid as deleted in the current log layer.
```

### 10.2 `U` - field update

Form:

```text
U<TAB><cid><TAB><path><TAB><value_tail>
```

Example:

```text
U<TAB>100<TAB>flags<TAB>4
U<TAB>100<TAB>P.review_status<TAB>verified
```

For v1, update logs should be optional. A simple implementation can rewrite or compact `.ten` files instead of supporting `U` and `D` immediately.

---

## 11. Reserved record tags

Recommended tag allocation:

```text
C  block/entity/conversation header
M  message/content child record
G  tag link child record
P  property/metadata child record
T  tool/subrecord call
R  tool/subrecord result
D  delete/tombstone, optional update-log profile
U  update/patch, optional update-log profile
H  hash/checkpoint, reserved
A  attachment reference, reserved
B  binary reference, reserved
X  experimental extension namespace
```

Strict parsers should reject unknown record tags unless the active profile or schema explicitly allows them.

---

## 12. Numeric field rules

Numeric fields should be decimal ASCII integers.

Strict rules:

```text
No signs.
No commas.
No underscores.
No decimal points.
No exponent notation.
No leading plus sign.
No whitespace trimming.
```

Recommended leading-zero rule:

```text
0 is valid.
123 is valid.
00123 should be rejected in strict mode unless a field explicitly allows fixed-width ids.
```

Use integers instead of floats in hot records.

Bad:

```text
quality = 0.98
```

Better:

```text
q_milli = 980
```

For v1, keep `C` minimal and place optional quality or scoring fields in `P` until they are proven to be hot query fields.

---

## 13. Block validity rules

A strict `chatpack` validator must enforce:

```text
1. The first non-empty line is a valid magic line.
2. Every record has the correct number of fixed fields.
3. Every C cid is unique within the file or resolved by a defined update-log rule.
4. M, G, P, T, and R records must appear inside a current C block.
5. Every M role exists in @role.
6. Every C sid exists in @source unless sid is 0.
7. Every C did exists in @domain unless did is 0.
8. Every G tag_id exists in @tag.
9. Every T tool_id exists in @tool.
10. M turns must be unique inside a block.
11. M turns should be ascending.
12. M turns should start at 0 and be contiguous in strict chat mode.
13. Tail content must not contain raw LF.
14. Escapes must be valid.
15. Numeric fields must parse exactly and fit their declared ranges.
```

Recommended warning-level checks:

```text
1. Empty assistant messages not linked to tool calls.
2. Very large message content.
3. Suspicious repeated identical blocks.
4. Missing source/domain dictionaries.
5. Too many P records used for fields that should be promoted to C fields.
```

---

## 14. Parser algorithm

The parser should operate on bytes, not high-level string tokenization.

Recommended algorithm:

```text
1. Read a large byte buffer or memory-map the file.
2. Locate LF boundaries.
3. For each line, inspect the first byte.
4. If blank or comment, skip.
5. If magic line, parse fixed fields.
6. If directive line, dispatch by directive name.
7. If record line, dispatch by record tag.
8. Split only the required fixed prefix fields.
9. Treat the final tail field as an unsplit byte slice.
10. Unescape fields only if they contain backslash.
11. Validate and emit compact internal structs.
```

Avoid in the hot parser:

```text
regular expressions
generic CSV parsers
JSON parsers
YAML parsers
splitting every line into unlimited fields
allocating strings for every field
trimming fields implicitly
recursive parsing
```

Use:

```text
single-byte record dispatch
limited tab scanning
custom integer parsing
zero-copy slices where possible
large buffered reads or memory mapping
strict build-time validation
trusted fast read mode after validation
```

---

## 15. `.tenb` generated binary cache

`.tenb` is the fast runtime representation generated from `.ten`.

A `.tenb` file should be opaque to users. It is not the canonical format.

Minimum required semantics:

```text
1. .tenb must identify the source .ten content using a strong source hash.
2. .tenb must identify the TEN version/profile/schema it was built from.
3. .tenb must be safely discardable.
4. Tensack must rebuild .tenb when it is missing, stale, corrupt, or incompatible.
5. Runtime reads should prefer .tenb over reparsing .ten repeatedly.
```

Recommended internal layout:

```text
TENB header
version
source_hash
schema_hash
section_directory

entity arrays:
  cid[]
  sid[]
  did[]
  flags[]
  msg_start[]
  msg_count[]

message arrays:
  role[]
  content_start[]
  content_len[]

content_blob:
  decoded UTF-8 bytes

lookup:
  sorted cid[] or hash table cid -> entity row
```

Read path for one entity:

```text
1. Look up cid in .tenb.
2. Read msg_start and msg_count.
3. Slice message arrays.
4. Slice content_blob.
5. Materialize objects only if the caller needs object form.
```

Fast v1 lookup strategy:

```text
Start with sorted cid[] + binary search.
Add an internal hash table later only if benchmarks show it matters.
```

---

## 16. `.tenx` optional search index

`.tenx` is optional and generated.

Use `.tenx` only for full-text search. Do not make normal reads depend on it.

A `.tenx` file should contain:

```text
source_hash
schema_hash
term dictionary
postings lists
references to cid/turn/content positions
```

Query paths:

```text
exact id lookup        -> .tenb
metadata filter        -> .tenb
full-text word search  -> .tenx
substring search       -> .tenx if implemented with trigrams or equivalent
```

`.tenx` can internally be custom, SQLite FTS-based, or another implementation. Users should only see the `.tenx` extension and should be able to delete it safely.

---

## 17. Runtime behavior

Opening a `.ten` file should follow this behavior:

```text
1. Locate data.ten.
2. Check for data.tenb.
3. If data.tenb exists, verify source hash, version, and schema.
4. If valid, open data.tenb.
5. If missing/stale/invalid, rebuild data.tenb from data.ten.
6. Use data.tenb for normal reads.
```

Search behavior:

```text
1. If query is exact id or metadata filter, use .tenb.
2. If query is full-text, check for .tenx.
3. If .tenx is valid, use it.
4. If .tenx is missing/stale/invalid, rebuild it from .ten or .tenb.
```

The user-facing mental model remains simple:

```text
Edit data.ten.
Tensack handles caches automatically.
```

---

## 18. Sharding

Small projects can use one file:

```text
data.ten
data.tenb
data.tenx
```

Large projects may use multiple `.ten` shards:

```text
data-000000.ten
data-000001.ten
data-000002.ten
```

Rules:

```text
1. A C block must not span shards.
2. Each shard must be independently valid.
3. Each shard can have its own .tenb cache.
4. Higher-level project tooling may abstract multiple shards as one dataset.
```

Sharding is an implementation/project organization choice, not a change to syntax.

---

## 19. Example: minimal valid file

Visualized with `<TAB>` markers:

```text
TEN<TAB>1<TAB>chatpack<TAB>tensack-chat-v1
@role<TAB>s<TAB>system
@role<TAB>u<TAB>user
@role<TAB>a<TAB>assistant
@source<TAB>1<TAB>biology_notes
@domain<TAB>1<TAB>science
@domain<TAB>2<TAB>biology
@tag<TAB>1<TAB>biology
@tag<TAB>2<TAB>cell_energy

C<TAB>1<TAB>1<TAB>2<TAB>0
G<TAB>1
G<TAB>2
M<TAB>0<TAB>s<TAB>You are a biology tutor.
M<TAB>1<TAB>u<TAB>What is ATP?
M<TAB>2<TAB>a<TAB>ATP is the main energy-carrying molecule in cells.
```

Logical parse result:

```text
entity/conversation 1
  source: biology_notes
  domain: biology
  tags: biology, cell_energy
  messages:
    0 system: You are a biology tutor.
    1 user: What is ATP?
    2 assistant: ATP is the main energy-carrying molecule in cells.
```

---

## 20. Example: content with newlines and tabs

Original message content:

~~~~text
Can you fix this?

```python
def add(a, b):
	return a + b
```
~~~~

Stored in `.ten`:

```text
M<TAB>0<TAB>u<TAB>Can you fix this?\n\n```python\ndef add(a, b):\n<TAB>return a + b\n```
```

Explanation:

```text
The newlines are escaped as \n.
The indentation tab can be a raw tab because content_tail is the final field.
The parser stops structural tab splitting after the role field.
```

---

## 21. Example: tool call

```text
TEN<TAB>1<TAB>chatpack<TAB>tensack-chat-v1
@role<TAB>u<TAB>user
@role<TAB>a<TAB>assistant
@role<TAB>t<TAB>tool
@source<TAB>1<TAB>synthetic
@domain<TAB>1<TAB>math
@tool<TAB>1<TAB>calculator

C<TAB>10<TAB>1<TAB>1<TAB>0
M<TAB>0<TAB>u<TAB>What is 12 * 19?
T<TAB>1<TAB>0<TAB>1<TAB>expression=12*19
R<TAB>2<TAB>0<TAB>228
M<TAB>3<TAB>a<TAB>12 * 19 = 228.
```

Logical parse result:

```text
conversation 10
  user message: What is 12 * 19?
  tool call: calculator(expression=12*19)
  tool result: 228
  assistant message: 12 * 19 = 228.
```

---

## 22. Validation test vectors

### 22.1 Valid: raw tab in tail field

```text
M<TAB>0<TAB>u<TAB>hello<TAB>world
```

Expected content:

```text
hello<TAB>world
```

### 22.2 Valid: escaped newline

```text
M<TAB>0<TAB>u<TAB>line one\nline two
```

Expected content:

```text
line one
line two
```

### 22.3 Invalid: message before block

```text
TEN<TAB>1<TAB>chatpack<TAB>tensack-chat-v1
M<TAB>0<TAB>u<TAB>hello
```

Reason:

```text
M requires a current C block.
```

### 22.4 Invalid: unknown escape

```text
M<TAB>0<TAB>u<TAB>hello\qworld
```

Reason:

```text
\q is not a valid escape sequence.
```

### 22.5 Invalid: missing field

```text
C<TAB>1<TAB>1<TAB>2
```

Reason:

```text
C requires cid, sid, did, and flags.
```

### 22.6 Invalid: duplicate turn

```text
C<TAB>1<TAB>1<TAB>2<TAB>0
M<TAB>0<TAB>u<TAB>first
M<TAB>0<TAB>a<TAB>second
```

Reason:

```text
M turn numbers must be unique inside a block.
```

---

## 23. Implementation checklist

### 23.1 V1 parser

Build a native parser if maximum speed matters.

Recommended implementation language:

```text
Rust, Zig, C, or C++
```

Recommended first choice:

```text
Rust
```

Parser tasks:

```text
1. Read bytes.
2. Validate magic line.
3. Dispatch by first byte.
4. Parse directives.
5. Parse C blocks.
6. Parse M/G/P/T/R child records.
7. Implement exact numeric parsing.
8. Implement strict escaping.
9. Validate block structure.
10. Produce compact in-memory structs or .tenb output.
```

### 23.2 V1 writer

Writer tasks:

```text
1. Emit magic line.
2. Emit dictionaries.
3. Emit C blocks.
4. Emit child records in stable order.
5. Escape only what must be escaped.
6. Preserve human readability.
7. Sort or validate turns.
```

### 23.3 V1 `.tenb` builder

Builder tasks:

```text
1. Strict-parse .ten.
2. Hash source content.
3. Build entity arrays.
4. Build message arrays.
5. Decode content into content_blob.
6. Build sorted cid lookup.
7. Write .tenb atomically.
8. Verify .tenb after write.
```

### 23.4 V1 runtime reader

Runtime tasks:

```text
1. Prefer .tenb.
2. Validate .tenb source hash/version/schema.
3. Rebuild .tenb if stale.
4. Serve reads from .tenb.
5. Fall back to .ten only when building or debugging.
```

### 23.5 V1 search

Search tasks:

```text
1. Do not implement .tenx until full-text search is required.
2. Start with .tenb scans for small data.
3. Add .tenx when scan search becomes too slow.
4. Keep .tenx disposable and source-hash validated.
```

---

## 24. CLI outline

Recommended CLI commands:

```text
ten check data.ten
ten build data.ten
ten inspect data.ten
ten dump data.tenb
ten search data.ten "ATP"
ten compact data.ten
```

Command meanings:

```text
ten check    strict validation of .ten
ten build    generate or refresh .tenb
ten inspect  human summary of headers, counts, dictionaries, and blocks
ten dump     debug view of .tenb contents
ten search   use .tenx if available, build if needed
ten compact  apply update logs/tombstones and write clean .ten
```

---

## 25. Performance rules

For maximum speed:

```text
1. Use single-byte record tags.
2. Use integer ids instead of repeated strings.
3. Use dictionaries for roles, sources, domains, tags, and tools.
4. Store one entity/conversation as one contiguous C block.
5. Make large text content a tail field.
6. Allow raw tabs in tail fields.
7. Escape raw newlines as \n.
8. Split only the fixed prefix fields.
9. Unescape only fields that contain backslash.
10. Parse bytes directly.
11. Avoid regex, JSON parsers, CSV parsers, and YAML.
12. Build .tenb once and use it for repeated reads.
13. Put all visible complexity behind .tenb and .tenx.
```

The hot path should not repeatedly reparse source `.ten` if `.tenb` is valid.

---

## 26. Compatibility and extension rules

Versioning should be explicit:

```text
TEN<TAB>1<TAB>chatpack<TAB>tensack-chat-v1
```

Forward-compatibility rules:

```text
1. Unknown core record tags are errors in strict mode.
2. Unknown directives are errors unless they begin with @x-.
3. Experimental record tags should use X or an explicitly declared extension profile.
4. A schema/profile bump is required for incompatible meaning changes.
5. .tenb and .tenx must include the source schema/version they were built from.
```

Recommended extension directive:

```text
@extension<TAB><name><TAB><version>
```

Example:

```text
@extension<TAB>attachments<TAB>1
```

---

## 27. Security and corruption rules

A strict implementation should protect against malformed or hostile files.

Recommended limits:

```text
maximum line length
maximum content tail length
maximum messages per block
maximum blocks per file
maximum dictionary entries
maximum property records per block
maximum recursion depth, if any future nested profile adds recursion
```

Required cache safety:

```text
1. Never trust .tenb if source hash does not match .ten.
2. Never trust .tenx if source hash does not match .ten.
3. Write generated files atomically: write temp file, fsync if needed, then rename.
4. Treat .tenb and .tenx as untrusted binary input if loaded from external sources.
5. Prefer rebuilding generated caches from .ten when in doubt.
```

---

## 28. Final V1 decision

Use only these user-visible/generation-level extensions:

```text
.ten   canonical human-readable source
.tenb  generated binary cache containing fast-read structures and indexes
.tenx  optional generated search index
```

Use `.ten` syntax based on:

```text
magic header
line-oriented records
single-byte tags
literal tab separators
block-packed C/M structure
tail text fields
minimal escaping
integer ids
directive dictionaries
strict validation
```

Do not use:

```text
verbose JSONL as canonical source
plain .tsv as the branded format
separate visible index files
training/tokenization-specific layers
unnecessary generated file types
```

The core `.ten` record pattern is:

```text
TEN<TAB>1<TAB>chatpack<TAB>tensack-chat-v1

C<TAB>cid<TAB>sid<TAB>did<TAB>flags
M<TAB>turn<TAB>role<TAB>content_tail
M<TAB>turn<TAB>role<TAB>content_tail
M<TAB>turn<TAB>role<TAB>content_tail
```

That is the foundation for Tensack's human-readable data format.
