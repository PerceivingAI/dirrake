---
name: dirrake
description: Use DirRake to inspect filesystem metadata recursively, including filename, extension, age, and size searches; largest-file or directory rankings; and tree summaries. Use for read-only metadata questions, not file-content search or filesystem mutation.
---

# DirRake

Use the installed `dirrake` CLI for recursive filesystem metadata inspection.

## Discover the installed contract

Run this before the first scan in a task unless the installed contract is already known from the current conversation:

```text
dirrake capabilities json
```

Build commands from that response. Do not assume that model knowledge matches the installed version. If `dirrake` is unavailable, say so. Install it only when the user has authorized installation.

## Choose the command

| Need | Command |
|---|---|
| Filename contains text | `dirrake word <TEXT>` |
| File extension matches | `dirrake ext <EXT>` |
| File is strictly larger than MiB threshold | `dirrake size <MIB>` |
| File is older or newer than a day threshold | `dirrake older <DAYS>` or `dirrake newer <DAYS>` |
| Zero-byte files | `dirrake empty` |
| Largest files | `dirrake top <N>` |
| Largest recursive directory totals | `dirrake dirs <N>` |
| Tree census | `dirrake info` |

Combine file filters with an explicit `and` before every added filter:

```text
dirrake size 100 and ext mp4 <PATH> limit 100 relative json
```

`top`, `dirs`, and `info` do not accept compound filters. `top N` also rejects a separate `limit`.

## Prefer structured, bounded output

For agent use, prefer `json` with an explicit scan root. Add `relative` for repository-local results. Add `limit N` whenever the result set may be large:

```text
dirrake word camera <PATH> limit 100 relative json
```

Use `jsonl` when line-oriented records help downstream processing. DirRake finishes and sorts the scan before it emits JSONL, so JSONL is not a live discovery stream.

When reading structured results:

- Require the expected `schema_version`; ignore unknown additive fields.
- Compare total and returned counts. Never present returned rows as complete when `truncated` is true.
- Report material filesystem warnings. A successful scan may still contain warnings.
- Treat byte fields as authoritative. Human-readable sizes are presentation only.

## Preserve the filesystem contract

- DirRake is read-only. Do not claim that it can copy, move, rename, edit, or delete files.
- `word` searches filenames only. Use a content-search tool such as `rg` for file contents.
- Hidden and ignored files are included. Ignore files such as `.gitignore` do not filter the scan.
- Symbolic links and reparse points are not followed.
- An omitted path scans the process working directory. Prefer an explicit path when the target matters.
- `depth N` bounds traversal. Directory and census totals then cover only the scanned portion.
- `md` writes a timestamped report in the launch directory. Use it only when the user asks for a report file.

Modifier words such as `json`, `limit`, or `relative` can be directory names. Use an explicit path form such as `./json` when a target name would otherwise be parsed as a modifier. For a numeric directory passed to `dirs`, use a form such as `./20`.

## Handle outcomes

DirRake uses these exit codes:

```text
0  success, including zero matches
2  invalid arguments or query
3  invalid or inaccessible scan root
4  output or report failure
5  internal failure
```

Do not treat zero matches as an error. Distinguish a failed root from warnings encountered below a valid root.
