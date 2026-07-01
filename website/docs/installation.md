# Installation

Pick whichever fits your platform. Prebuilt binaries are published on every
[GitHub release](https://github.com/taidaru/taku/releases); building from source
needs a Rust toolchain and a C compiler (Lua is compiled from source via `mlua`).

## Linux & macOS (install script)

The quickest way — downloads the latest release for your OS/architecture and
installs the `taku` binary:

```sh
curl -fsSL https://raw.githubusercontent.com/taidaru/taku/main/install.sh | sh
```

It installs to `~/.local/bin` (or `/usr/local/bin` when that's writable and on
your `PATH`). Override with environment variables:

```sh
# install a specific version
TAKU_VERSION=v0.1.0-alpha.1 curl -fsSL https://raw.githubusercontent.com/taidaru/taku/main/install.sh | sh

# install to a custom directory
TAKU_INSTALL_DIR=~/bin curl -fsSL https://raw.githubusercontent.com/taidaru/taku/main/install.sh | sh
```

If the install directory isn't on your `PATH`, the script tells you what to add.

## Windows (Scoop)

The Taku repository is itself a [Scoop](https://scoop.sh) bucket:

```powershell
scoop bucket add taku https://github.com/taidaru/taku
scoop install taku
```

Update later with `scoop update taku`. To pin a specific version:

```powershell
scoop install taku@0.1.0-alpha.1
```

## From a release archive

Download the archive for your platform from the
[releases page](https://github.com/taidaru/taku/releases), extract the `taku`
binary, and put it on your `PATH`:

- Linux: `taku-x86_64-unknown-linux-gnu.tar.gz`
- macOS (Apple silicon): `taku-aarch64-apple-darwin.tar.gz`
- Windows: `taku-x86_64-pc-windows-msvc.zip`

## From source

With a Rust toolchain and a C compiler installed:

```sh
cargo install --path crates/taku   # installs the `taku` binary
# or
cargo build --release              # -> target/release/taku
```
