# sixpack `.6` Format Specification

**Status:** draft background reference. Current file/storage decisions live in
`sixpack_storage_spec.md`; current product/backend decisions live in
`sixpack_book.md`.

**Document version:** Draft v0.1  
**Format name:** SIX - sixpack Entity Notation  
**Primary file extensions:** `.6`, `.6b`, `.6x`  
**Primary design decision:** `.6` is the only canonical human-readable source format. `.6b` and `.6x` are generated caches.

---

## 1. Executive summary

sixpack should not use verbose JSONL as its canonical human-readable data format. JSONL is structurally convenient, but for high-volume chat/entity data it repeats keys such as `role`, `content`, `messages`, `metadata`, `source`, and `domain` on every object. That is avoidable overhead.

sixpack should also avoid exposing plain `.tsv` as the brand-level format. The underlying parsing model can still use tab-separated fields because tabs are fast and simple, but the user-facing format should be a sixpack-specific `.6` specification with strict rules, typed records, block structure, escaping, validation, and generated acceleration caches.

The simplified architecture is:

```text
.6   = canonical human-readable data
.6b  = generated opaque binary cache, including indexes and fast-read layout
.6x  = optional generated full-text search index
```

The key rule is:

```text
.6 is truth.
.6b and .6x are disposable generated acceleration files.
```

If `.6b` or `.6x` is missing, stale, corrupted, or incompatible, sixpack must rebuild it from `.6`.

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

JSONL may remain an import/export compatibility format, but it should not be the canonical sixpack source format.

### 2.2 Do not expose generic `.tsv`

The format should not be branded or documented as `.tsv`. sixpack should define `.6` as its own format.

Internally, `.6` uses literal tab bytes as structural separators because they are fast to parse. But `.6` is not generic TSV. It has:

```text
magic headers
record tags
block structure
tail fields
strict escaping
typed numeric fields
role/source/domain/tag dictionaries
validation rules
generated .6b caches
optional .6x search indexes
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

### 2.4 Merge all indexing into `.6b`

Do not create separate visible index files such as `.6i`.

The generated `.6b` file should contain all fast-read structures:

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

### 2.5 Keep `.6x` optional

Only create `.6x` if full-text search matters. Exact lookup, metadata filtering, and normal reads should use `.6b`.

---

## 3. File roles

### 3.1 `.6`

`.6` is the canonical human-readable source file. It is editable, reviewable, diffable, and versionable.

Rules:

```text
A valid sixpack dataset must be recoverable from .6 alone.
.6b and .6x must never contain irreplaceable source data.
```

### 3.2 `.6b`

`.6b` is an opaque generated binary cache. It is used for speed.

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

It may be deleted at any time. sixpack must rebuild it from `.6`.

### 3.3 `.6x`

`.6x` is an optional generated search index.

It should contain:

```text
term dictionary
postings lists
content/document references
source hash
schema hash/version
```

It may be custom, SQLite FTS-based, or another internal search representation. The extension remains `.6x` so the implementation is abstracted from users.

---

## 4. High-level `.6` syntax outline

Visual notation in this document:

```text
<TAB> means one literal horizontal tab byte, U+0009.
<LF> means one literal line feed byte, U+000A.
```

A minimal chatpack file has this shape:

```text
SIX<TAB>1<TAB>chatpack<TAB>sixpack-chat-v1
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
magic line       SIX<TAB>version<TAB>profile<TAB>schema_id
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

Every `.6` file must use:

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

A `.6` file is made of physical lines. One physical line is one record, directive, comment, blank line, or magic line.

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

Stored in `.6`:

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

Every `.6` file must begin with a magic line.

Form:

```text
SIX<TAB><version><TAB><profile><TAB><schema_id>
```

For the first chat format:

```text
SIX<TAB>1<TAB>chatpack<TAB>sixpack-chat-v1
```

Fields:

```text
SIX        literal magic token
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
@meta<TAB>created_by<TAB>sixpack
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

The `args_tail` field is application-defined. It can use compact key-value syntax, a custom sixpack subformat, or JSON inside the field if absolutely necessary. The outer `.6` container should remain line-oriented and non-JSON.

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

Canonical `.6` files can be immutable block files. If update logs are needed, use an update-log profile or append region.

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

For v1, update logs should be optional. A simple implementation can rewrite or compact `.6` files instead of supporting `U` and `D` immediately.

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

## 15. `.6b` generated binary cache

`.6b` is the fast runtime representation generated from `.6`.

A `.6b` file should be opaque to users. It is not the canonical format.

Minimum required semantics:

```text
1. .6b must identify the source .6 content using a strong source hash.
2. .6b must identify the SIX version/profile/schema it was built from.
3. .6b must be safely discardable.
4. sixpack must rebuild .6b when it is missing, stale, corrupt, or incompatible.
5. Runtime reads should prefer .6b over reparsing .6 repeatedly.
```

Recommended internal layout:

```text
SIXB header
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
1. Look up cid in .6b.
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

## 16. `.6x` optional search index

`.6x` is optional and generated.

Use `.6x` only for full-text search. Do not make normal reads depend on it.

A `.6x` file should contain:

```text
source_hash
schema_hash
term dictionary
postings lists
references to cid/turn/content positions
```

Query paths:

```text
exact id lookup        -> .6b
metadata filter        -> .6b
full-text word search  -> .6x
substring search       -> .6x if implemented with trigrams or equivalent
```

`.6x` can internally be custom, SQLite FTS-based, or another implementation. Users should only see the `.6x` extension and should be able to delete it safely.

---

## 17. Runtime behavior

Opening a `.6` file should follow this behavior:

```text
1. Locate data.6.
2. Check for data.6b.
3. If data.6b exists, verify source hash, version, and schema.
4. If valid, open data.6b.
5. If missing/stale/invalid, rebuild data.6b from data.6.
6. Use data.6b for normal reads.
```

Search behavior:

```text
1. If query is exact id or metadata filter, use .6b.
2. If query is full-text, check for .6x.
3. If .6x is valid, use it.
4. If .6x is missing/stale/invalid, rebuild it from .6 or .6b.
```

The user-facing mental model remains simple:

```text
Edit data.6.
sixpack handles caches automatically.
```

---

## 18. Sharding

Small projects can use one file:

```text
data.6
data.6b
data.6x
```

Large projects may use multiple `.6` shards:

```text
data-000000.6
data-000001.6
data-000002.6
```

Rules:

```text
1. A C block must not span shards.
2. Each shard must be independently valid.
3. Each shard can have its own .6b cache.
4. Higher-level project tooling may abstract multiple shards as one dataset.
```

Sharding is an implementation/project organization choice, not a change to syntax.

---

## 19. Example: minimal valid file

Visualized with `<TAB>` markers:

```text
SIX<TAB>1<TAB>chatpack<TAB>sixpack-chat-v1
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

Stored in `.6`:

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
SIX<TAB>1<TAB>chatpack<TAB>sixpack-chat-v1
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
SIX<TAB>1<TAB>chatpack<TAB>sixpack-chat-v1
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
10. Produce compact in-memory structs or .6b output.
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

### 23.3 V1 `.6b` builder

Builder tasks:

```text
1. Strict-parse .6.
2. Hash source content.
3. Build entity arrays.
4. Build message arrays.
5. Decode content into content_blob.
6. Build sorted cid lookup.
7. Write .6b atomically.
8. Verify .6b after write.
```

### 23.4 V1 runtime reader

Runtime tasks:

```text
1. Prefer .6b.
2. Validate .6b source hash/version/schema.
3. Rebuild .6b if stale.
4. Serve reads from .6b.
5. Fall back to .6 only when building or debugging.
```

### 23.5 V1 search

Search tasks:

```text
1. Do not implement .6x until full-text search is required.
2. Start with .6b scans for small data.
3. Add .6x when scan search becomes too slow.
4. Keep .6x disposable and source-hash validated.
```

---

## 24. CLI outline

Recommended CLI commands:

```text
sixpack check data.6
sixpack build data.6
sixpack inspect data.6
sixpack dump data.6b
sixpack search data.6 "ATP"
sixpack compact data.6
```

Command meanings:

```text
sixpack check    strict validation of .6
sixpack build    generate or refresh .6b
sixpack inspect  human summary of headers, counts, dictionaries, and blocks
sixpack dump     debug view of .6b contents
sixpack search   use .6x if available, build if needed
sixpack compact  apply update logs/tombstones and write clean .6
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
12. Build .6b once and use it for repeated reads.
13. Put all visible complexity behind .6b and .6x.
```

The hot path should not repeatedly reparse source `.6` if `.6b` is valid.

---

## 26. Compatibility and extension rules

Versioning should be explicit:

```text
SIX<TAB>1<TAB>chatpack<TAB>sixpack-chat-v1
```

Forward-compatibility rules:

```text
1. Unknown core record tags are errors in strict mode.
2. Unknown directives are errors unless they begin with @x-.
3. Experimental record tags should use X or an explicitly declared extension profile.
4. A schema/profile bump is required for incompatible meaning changes.
5. .6b and .6x must include the source schema/version they were built from.
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
1. Never trust .6b if source hash does not match .6.
2. Never trust .6x if source hash does not match .6.
3. Write generated files atomically: write temp file, fsync if needed, then rename.
4. Treat .6b and .6x as untrusted binary input if loaded from external sources.
5. Prefer rebuilding generated caches from .6 when in doubt.
```

---

## 28. Final V1 decision

Use only these user-visible/generation-level extensions:

```text
.6   canonical human-readable source
.6b  generated binary cache containing fast-read structures and indexes
.6x  optional generated search index
```

Use `.6` syntax based on:

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

The core `.6` record pattern is:

```text
SIX<TAB>1<TAB>chatpack<TAB>sixpack-chat-v1

C<TAB>cid<TAB>sid<TAB>did<TAB>flags
M<TAB>turn<TAB>role<TAB>content_tail
M<TAB>turn<TAB>role<TAB>content_tail
M<TAB>turn<TAB>role<TAB>content_tail
```

That is the foundation for sixpack's human-readable data format.
