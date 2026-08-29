# Installing DirRake

DirRake is a Rust command-line application that builds to a single `dirrake` executable (`dirrake.exe` on Windows).

## Requirements

Install a current stable Rust toolchain with Cargo from <https://rustup.rs/>.

Verify:

```text
rustc --version
cargo --version
```

DirRake uses the Rust 2024 edition. Use a current stable toolchain rather than an old system Rust package.

## Install from this source checkout

From the repository root:

```text
cargo install --path . --locked
```

`--locked` uses the committed `Cargo.lock`, matching the dependency graph verified by the project.

Cargo normally installs to:

- Windows: `%USERPROFILE%\.cargo\bin\dirrake.exe`
- Linux/macOS: `$HOME/.cargo/bin/dirrake`

`rustup` normally puts this directory on `PATH`. If `dirrake` is not found, add the appropriate Cargo `bin` directory and open a new terminal.

Verify the installed tool and its machine contract:

```text
dirrake --version
dirrake --help
dirrake capabilities json
```

## Build without installing

Optimized release build:

```text
cargo build --release --locked
```

Output:

- Windows: `target\release\dirrake.exe`
- Linux/macOS: `target/release/dirrake`

You can run that executable directly or copy it to a directory already on `PATH`.

## Update a Cargo installation

After updating the source checkout:

```text
cargo install --path . --locked --force
```

Then verify:

```text
dirrake --version
dirrake capabilities json
```

## Uninstall

Cargo installation:

```text
cargo uninstall dirrake
```

If you manually copied the executable, remove that copied executable instead.

## Windows

PowerShell:

```powershell
cargo build --release --locked
.\target\release\dirrake.exe --help
cargo install --path . --locked
where.exe dirrake
dirrake capabilities json
```

## Linux/macOS

```bash
cargo build --release --locked
./target/release/dirrake --help
cargo install --path . --locked
command -v dirrake
dirrake capabilities json
```

## Isolated install verification

Maintainers can test installation without replacing their normal Cargo-installed binary.

Windows:

```powershell
cargo install --path . --locked --root target\install-smoke --force
.\target\install-smoke\bin\dirrake.exe --version
.\target\install-smoke\bin\dirrake.exe capabilities json
```

Linux/macOS:

```bash
cargo install --path . --locked --root target/install-smoke --force
./target/install-smoke/bin/dirrake --version
./target/install-smoke/bin/dirrake capabilities json
```
