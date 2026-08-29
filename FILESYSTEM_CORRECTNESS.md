# DirRake Filesystem Correctness

DirRake treats filesystem behavior as a compatibility contract. The dedicated torture suite is `tests/filesystem_torture.rs` and runs as part of the normal `cargo test` matrix on both Ubuntu and Windows.

## Cross-platform invariants

DirRake must:

- remain read-only with respect to scanned entries;
- recurse through ordinary directories but never follow symbolic links, junctions, or other link/reparse targets;
- continue after an inaccessible entry and report a warning when the filesystem exposes the failure;
- keep result ordering deterministic even when discovery is parallel;
- preserve ordinary Unicode filenames exactly in structured output;
- handle paths beyond the legacy Windows 260-character boundary when the operating system supports the standard Rust filesystem APIs;
- use full-width byte accounting for large/sparse files;
- apply age boundaries consistently: `older N` is strictly before the cutoff, while `newer N` includes the cutoff;
- render terminal/Markdown control characters visibly rather than allowing a filename to inject new lines or terminal control sequences.

## Unicode and OS-native names

Valid Unicode names are preserved normally. The suite includes accented characters, emoji, and non-Latin text.

Unix filesystems can also contain names that are not valid UTF-8. DirRake does not silently replace those bytes with the Unicode replacement character.

For an invalid Unix path, human/structured path text uses the explicit prefix:

```text
<non-utf8>:
```

Invalid bytes are represented as uppercase byte escapes such as:

```text
\xFF
```

Example:

```text
<non-utf8>:CAMERA-\xFF.RS
```

ASCII `word`/`ext` queries operate against the underlying Unix filename bytes, so an invalid byte elsewhere in the filename does not prevent `camera` or `rs` from matching. Non-ASCII case-insensitive filename matching requires a valid Unicode filename; DirRake does not invent Unicode semantics for an invalid byte sequence.

Windows filenames are Unicode in normal operation. If an OS string contains an unpaired UTF-16 unit, DirRake uses an explicit `<non-unicode>:` escaped representation rather than silently substituting a replacement character.

## Control characters

JSON remains valid JSON and preserves valid filename text according to JSON escaping rules.

Terminal and Markdown output are human-facing surfaces. Control characters are rendered visibly, for example a newline inside a Unix filename is shown as:

```text
camera-line\nbreak.txt
```

rather than creating a second terminal/report line. This also prevents embedded escape characters from being interpreted as terminal control sequences.

## Links and reparse points

The walker is configured with link following disabled. An explicitly supplied symbolic-link/reparse-point scan root is rejected with exit code `3`; otherwise the walker could dereference that root before recursive link policy takes effect. A process working directory that the operating system has already entered through a junction/symlink remains valid (for example for `capabilities` or an omitted scan path); this is process path resolution rather than following an entry discovered during traversal.

The platform suite verifies:

- Unix symbolic link to an external directory is not followed;
- Unix self-referential symlink loop is not followed;
- Unix broken symlink does not destabilize the scan;
- Unix symbolic-link scan root is rejected;
- Windows NTFS directory junction to an external directory is not followed;
- Windows NTFS junction scan root is rejected.

The scan reports only ordinary files actually reached without traversing those links.

## Permission failures

On Unix, the suite creates an unreadable subdirectory (`000` mode) next to a readable matching file. The command must still succeed, return the readable match, and expose at least one filesystem warning.

An inaccessible, non-directory, or link/reparse **scan root** remains different: root validation fails the command with exit code `3`.

## Long paths

The cross-platform suite creates a nested relative path longer than 260 characters and verifies that DirRake can find the file and return its complete relative path. This guards against accidental reintroduction of legacy Windows `MAX_PATH` assumptions in DirRake itself.

## Large sparse files

The suite creates a logically 5 GiB sparse file and verifies:

- `size 4096` matches it;
- `size_bytes` is exact;
- matched-byte totals remain exact.

The test writes no 5 GiB payload; it changes logical file length only.

## Age boundary

For a one-day cutoff:

```text
modified < cutoff   => older 1
modified >= cutoff  => newer 1
```

The exact boundary belongs to `newer`, making the two predicates complementary at the cutoff.

## Files disappearing during a scan

Filesystem mutation is inherently racy. If an entry disappears after discovery but before metadata is read, the existing metadata-error path records the failure as a warning and continues.

The suite deliberately does not use timing-dependent delete races because such tests can pass without exercising the race or fail nondeterministically under CI load. Deterministic permission/link failures cover the same continue-on-entry-error contract without adding flaky gates.

## Running the suite

All platforms:

```text
cargo test --test filesystem_torture --locked
```

Full verification:

```text
cargo test --all-features --locked
```

Unix-specific cases compile and execute only on Unix runners; the Windows junction case compiles and executes only on Windows.
