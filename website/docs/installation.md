# Installation

Pick whichever fits your platform. Prebuilt binaries and installers are
published on every [GitHub release](https://github.com/taidaru/taku/releases);
building from source needs a Rust toolchain and a C compiler (Lua is compiled
from source via `mlua`).

## Linux & macOS (install script)

The quickest way — downloads the latest release for your OS/architecture and
installs the `taku` binary:

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/taidaru/taku/releases/latest/download/taku-installer.sh | sh
```

It installs to `$CARGO_HOME/bin` (`~/.cargo/bin` by default) and updates your
shell profile so the binary is on your `PATH`. To install to a custom
directory instead:

```sh
TAKU_INSTALL_DIR=~/bin curl --proto '=https' --tlsv1.2 -LsSf https://github.com/taidaru/taku/releases/latest/download/taku-installer.sh | sh
```

To install a specific version, take the installer from that release instead of
`latest`:

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/taidaru/taku/releases/download/v0.1.4/taku-installer.sh | sh
```

## Homebrew

The Taku repository doubles as a Homebrew tap:

```sh
brew tap taidaru/taku https://github.com/taidaru/taku
brew install taku
```

## Windows (install script)

Downloads the latest release and installs the `taku` binary:

```powershell
powershell -ExecutionPolicy Bypass -c "irm https://github.com/taidaru/taku/releases/latest/download/taku-installer.ps1 | iex"
```

An `.msi` installer for each release is also available on the
[releases page](https://github.com/taidaru/taku/releases).

## Windows (Scoop)

The Taku repository is itself a [Scoop](https://scoop.sh) bucket:

```powershell
scoop bucket add taku https://github.com/taidaru/taku
scoop install taku
```

Update later with `scoop update taku`. To pin a specific version:

```powershell
scoop install taku@0.1.2-alpha
```

## From a release archive

Download the archive for your platform from the
[releases page](https://github.com/taidaru/taku/releases), extract the `taku`
binary, and put it on your `PATH`:

- Linux: `taku-x86_64-unknown-linux-gnu.tar.xz` (also `musl` and `aarch64` variants)
- macOS: `taku-aarch64-apple-darwin.tar.xz` (Apple silicon) or `taku-x86_64-apple-darwin.tar.xz`
- Windows: `taku-x86_64-pc-windows-msvc.zip`

Each archive has a matching `.sha256` checksum file. The tarballs unpack into
a `taku-<target>/` directory; the zips unpack flat.

## Updating

To update an install done with the install scripts, re-run the same install
command — it fetches and installs the latest release over the existing one.
Homebrew and Scoop installs update through the package manager as usual.

## From source

With a Rust toolchain and a C compiler installed:

```sh
cargo install --path crates/taku   # installs the `taku` binary
# or
cargo build --release              # -> target/release/taku
```
