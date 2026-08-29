# DirRake

DirRake is a fast, read-only filesystem inspection CLI for humans and agents. It recursively scans directory trees in parallel, keeps its command language small, and provides a stable machine-readable output contract.

## Quick start

```text
dirrake size 100
dirrake word camera
dirrake ext mp4
dirrake top 20
dirrake dirs 20
dirrake info
```

An omitted path means the current directory. Add another directory to scan it instead:

```text
dirrake size 100 "D:\Videos"
dirrake word camera /srv/projects
```

For agent discovery:

```text
dirrake --help
dirrake capabilities
dirrake capabilities json
```

## Install

With a current stable Rust/Cargo toolchain installed, run from the repository root:

```text
cargo install --path . --locked
```

Verify:

```text
dirrake --version
dirrake --help
dirrake capabilities json
```

See [INSTALL.md](INSTALL.md) for build paths, PATH setup, updating, and uninstalling.

## Commands

| Command | Purpose | Example |
|---|---|---|
| `size <MIB>` | Files strictly larger than N MiB | `dirrake size 100` |
| `word <TEXT>` | Filename contains text, case-insensitive | `dirrake word camera` |
| `ext <EXT>` | Files with an extension, case-insensitive | `dirrake ext .mp4` |
| `older <DAYS>` | Files older than N days | `dirrake older 90` |
| `newer <DAYS>` | Files modified within N days | `dirrake newer 7` |
| `empty` | Zero-byte files | `dirrake empty` |
| `top <N>` | N largest files | `dirrake top 20` |
| `dirs [N]` | Recursive directory sizes, largest first | `dirrake dirs 20` |
| `info` | One-pass directory-tree census | `dirrake info` |
| `capabilities` | Discover commands, outputs, guarantees, schema, exit codes | `dirrake capabilities json` |

`dirs 20` is shorthand for `dirs limit 20`. If a directory is literally named with a reserved modifier such as `json` or a numeric `dirs` shorthand, use an explicit path form such as `./json` or `./20`.

## Compound filters

The file-search commands (`size`, `word`, `ext`, `older`, `newer`, `empty`) can combine filters with explicit `and`:

```text
dirrake size 100 and ext mp4
dirrake word camera and ext json
dirrake ext zip and older 90 and size 500 "D:\Backups"
```

All filters are evaluated during the same recursive scan.

## Common modifiers

Modifiers may appear after the command query and are order-independent:

```text
PATH          directory to scan; current directory when omitted
md            write a timestamped Markdown report in the launch directory
json          schema-versioned JSON to stdout
jsonl         schema-versioned JSON Lines to stdout
limit N       return at most N rows while still counting all matches
depth N       scan at most N levels below the root; root is depth 0
relative      show result paths relative to the scan root
absolute      show rooted paths; default
```

Examples:

```text
dirrake word camera "D:\Projects" md
dirrake size 100 /data limit 50 relative json
dirrake word camera and ext jpg and newer 7
```

The explicit `and` grammar is deliberate: an agent does not need to infer whether a token is another filter.

## Agent output

`json` and `jsonl` use **agent schema version 1**. Structured output includes:

- the scan root and controls;
- the exact operation/filter description;
- byte-precise result sizes;
- total matches versus returned matches;
- an explicit `truncated` flag;
- files/directories seen;
- warning counts and bounded warning samples;
- elapsed scan time.

For example:

```text
dirrake size 100 and ext zip /data limit 25 relative json
```

A bounded search still scans/counts every matching file. It retains only the rows required for the deterministic final result, so `matches_total` remains authoritative while `matches_returned` and `truncated` make output bounds explicit.

`jsonl` emits deterministic line-oriented records: a `meta` record, result records, then a final `summary`. DirRake completes the scan before emitting the sorted JSONL rows; it does not expose nondeterministic parallel discovery order.

The complete machine contract is documented in [AGENT_API.md](AGENT_API.md).

## Filesystem behavior

- Recursive and parallel by default.
- Hidden files are included.
- `.gitignore` and other ignore files do **not** hide files from DirRake.
- Symbolic links/reparse points are not followed.
- Individual filesystem failures become warnings; the rest of the tree continues scanning.
- Results are deterministic even though discovery is parallel.
- DirRake does not modify, move, rename, copy, or delete scanned files.
- `size N` uses MiB (`1 MiB = 1,048,576 bytes`) and is strictly greater-than.
- `word` searches filenames only, not file contents.
- `dirs` sizes are recursive within the portion of the tree actually scanned.
- `depth N` can therefore intentionally produce partial subtree totals.
- Valid Unicode paths are preserved exactly; Unix non-UTF8 paths use an explicit `<non-utf8>:` escaped representation instead of silent replacement characters.
- Human-readable output renders filename control characters visibly; JSON remains valid structured data.

For the platform/path edge-case contract—including Unicode, Unix non-UTF8 names, long paths, sparse files, permissions, symlinks/junctions, and control-character rendering—see [FILESYSTEM_CORRECTNESS.md](FILESYSTEM_CORRECTNESS.md).

## Markdown reports

Appending `md` writes:

```text
dirrake_<timestamp>.md
```

The report is always created in the directory where DirRake was launched, not the target scan directory.

## Stable exit codes

```text
0  success, including zero matches
2  invalid arguments or query
3  invalid or inaccessible scan root
4  output/report failure
5  internal failure
```

Filesystem warnings encountered *inside* an otherwise valid scan do not change a successful exit code.

A downstream consumer closing stdout early is also normal success. For example, if a pager, `head`, or an agent stops reading after enough results, DirRake treats the resulting broken pipe as exit code `0`. Other terminal/JSON/JSONL write failures and Markdown report failures remain exit code `4`.

## Build and verify

Optimized build:

```text
cargo build --release --locked
```

Run the complete test suite:

```text
cargo test --all-features --locked
```

Output:

- Windows: `target\release\dirrake.exe`
- Linux/macOS: `target/release/dirrake`
