# DirRake Agent API

DirRake exposes a compact human CLI and a stable machine-readable interface. Agents should prefer `json` or `jsonl` when consuming results programmatically and should use `capabilities json` for runtime discovery rather than assuming features from model knowledge.

## Discovery

```text
dirrake --help
dirrake capabilities json
```

`capabilities json` reports the installed DirRake version, agent schema version, commands, output modes, path modes, filesystem guarantees, and stable exit codes.

## Schema version

All structured output contains:

```json
{"schema_version": 1}
```

Schema policy:

- Consumers should ignore unknown additive fields.
- Existing field meanings/types will not be changed within schema version 1.
- Removing/renaming fields or changing their meaning/type requires a schema-version increment.
- Tool versions and agent-schema versions are independent.
- JSON object member order is not part of the contract; consumers must address fields by name.

## Recommended agent pattern

Use bounded, relative, structured output when the expected result set may be large:

```text
dirrake word camera /repo limit 100 relative json
```

Inspect:

```text
stats.matches_total
stats.matches_returned
stats.truncated
warnings.count
```

Do not infer that only `matches_returned` files exist when `truncated` is true.

## File query JSON

Commands:

```text
size
word
ext
older
newer
empty
top
```

Example:

```text
dirrake word camera and ext jpg /repo limit 2 relative json
```

Shape:

```json
{
  "schema_version": 1,
  "type": "file_results",
  "root": "/repo",
  "operation": {
    "type": "filters",
    "operator": "and",
    "description": "...",
    "filters": [
      {
        "type": "word",
        "text": "camera",
        "case_sensitive": false,
        "target": "filename"
      },
      {
        "type": "ext",
        "extension": "jpg",
        "case_sensitive": false
      }
    ]
  },
  "controls": {
    "path_mode": "relative",
    "max_depth": null,
    "limit": 2
  },
  "results": [
    {
      "path": "images/camera.jpg",
      "size_bytes": 1234
    }
  ],
  "stats": {
    "files_seen": 100,
    "directories_seen": 12,
    "matches_total": 1,
    "matches_returned": 1,
    "matched_bytes_total": 1234,
    "returned_bytes": 1234,
    "truncated": false,
    "warning_count": 0,
    "elapsed_ms": 8
  },
  "warnings": {
    "count": 0,
    "samples": [],
    "samples_truncated": false
  }
}
```

The exact numeric values above are illustrative; the field contract is normative.

### Filter records

`size`:

```json
{"type":"size","operator":"gt","mib":100,"bytes":104857600}
```

`word`:

```json
{"type":"word","text":"camera","case_sensitive":false,"target":"filename"}
```

`ext`:

```json
{"type":"ext","extension":"mp4","case_sensitive":false}
```

`older` / `newer`:

```json
{"type":"older","days":90}
{"type":"newer","days":7}
```

`empty`:

```json
{"type":"empty"}
```

`top` uses an operation record instead of filters:

```json
{"type":"top","count":20}
```

## JSONL

Use `jsonl` when line-oriented consumption is easier:

```text
dirrake word camera /repo limit 100 relative jsonl
```

File-query streams are ordered as:

1. one `meta` record;
2. zero or more deterministic `match` records;
3. one final `summary` record.

Example records:

```json
{"schema_version":1,"type":"meta","report_type":"file_results","root":"/repo","operation":{},"controls":{}}
{"schema_version":1,"type":"match","result":{"path":"camera.jpg","size_bytes":1234}}
{"schema_version":1,"type":"summary","report_type":"file_results","stats":{},"warnings":{}}
```

DirRake intentionally preserves deterministic result order. It therefore completes parallel discovery before emitting sorted JSONL result rows; JSONL is line-oriented output, not nondeterministic live discovery telemetry.

## Directory-size JSON

```text
dirrake dirs 20 /repo relative json
```

Top-level type:

```text
directory_results
```

Each result contains:

```json
{
  "path": "target",
  "size_bytes": 123456,
  "file_count": 42
}
```

Stats distinguish `directories_total` from `directories_returned` and expose `truncated`.

## Info JSON

```text
dirrake info /repo relative json
```

Top-level type:

```text
info
```

It contains:

- `largest_file`;
- `largest_directory`;
- extension groups with file count and bytes;
- files/directories seen;
- total file bytes;
- extension total/returned/truncation metadata;
- warnings and elapsed time.

`info limit N` limits the number of extension groups returned while preserving the total extension-group count.

## Controls

### `limit N`

Supported by filtered file queries, `dirs`, and `info`.

For file queries, DirRake still counts every match and matched byte while retaining only the rows necessary for the bounded deterministic result.

`top N` already defines its own bound and rejects a separate `limit` modifier.

### `depth N`

`root` is depth 0; immediate children are depth 1.

Depth applies to all scanning/analysis commands. Directory totals and `info` totals describe only the scanned portion of the tree.

### Path mode

```text
absolute   default
relative   relative to scan root
```

The top-level JSON `root` remains the rooted scan location. Result `path` fields follow the selected path mode.

Valid Unicode paths are emitted normally. On Unix, an OS path containing invalid UTF-8 bytes is represented explicitly with a `<non-utf8>:` prefix and `\xNN` byte escapes instead of lossy replacement characters. ASCII `word`/`ext` matching still operates on the underlying filename bytes. Windows uses an analogous `<non-unicode>:` escaped representation only for an abnormal unpaired UTF-16 OS string.

JSON escaping remains standard JSON escaping. Valid control characters in filenames are therefore preserved in structured output; human terminal/Markdown renderers show them as visible escapes.

## Compound query grammar

Only the file-filter commands accept `and`:

```text
dirrake <primary-filter> <value?> [and <filter> <value?> ...] [PATH] [MODIFIERS...]
```

Filters:

```text
size <MIB>
word <TEXT>
ext <EXT>
older <DAYS>
newer <DAYS>
empty
```

Examples:

```text
dirrake size 100 and ext mp4 json
dirrake word camera and ext jpg and newer 7 /repo limit 50 relative json
```

`and` is mandatory before every additional filter.

## Reserved positional tokens

The compact grammar recognizes these as modifiers:

```text
and
terminal
md
json
jsonl
limit
depth
relative
absolute
```

If the target directory literally has one of those names, use an explicit path form such as `./json`.

`dirs` also treats an initial all-digit token as its optional count shorthand; use `./20` to target a directory literally named `20`.

## Warnings versus failure

An inaccessible file/directory encountered *inside* an otherwise valid scan is recorded as a warning and the scan continues. Structured output reports:

```text
warnings.count
warnings.samples
warnings.samples_truncated
```

`warnings.count` is the total number of warning events observed, including repeated events. `warnings.samples` contains at most 100 unique warning strings, selected deterministically as the lexicographically smallest samples and returned in sorted order. `warnings.samples_truncated` is true whenever the event count exceeds the number of retained unique samples.

This is distinct from an invalid/inaccessible scan root, which is a command failure.

## Stable exit codes

```text
0  success, including zero matches
2  invalid arguments or query
3  invalid or inaccessible scan root
4  output/report failure
5  internal failure
```

Clap syntax errors and DirRake semantic usage errors both use exit code 2.

A downstream consumer intentionally closing stdout early is not an output failure. Terminal, JSON, JSONL, and the Markdown confirmation message treat an OS `BrokenPipe` as successful completion (`0`). Other output/serialization/report failures remain exit code `4`. Runtime discovery exposes this as `broken_pipe_is_success: true` in `capabilities json`.

## Division of responsibility

DirRake is for filesystem metadata/questions. It deliberately does not search file contents or mutate files. Agents should use a content-search specialist such as ripgrep for content queries and should not expect delete/move/copy operations from DirRake.
