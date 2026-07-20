# Taku

A task runner powered by **Rust** and scripted in **Lua**.

Automate builds, tests, deployments, linting, and other repetitive development
tasks in a `Takufile.lua`.

Taku embeds Lua in a sandboxed runtime and exposes a controlled, cross-platform
API for interacting with the operating system.

## Why Taku?

* **Real programming language** — write tasks in Lua instead of shell or YAML.
* **Cross-platform** — the same Takufile works on Linux, macOS, and Windows.
* **Sandboxed runtime** — standard Lua `io` and `os` libraries are unavailable;
  tasks interact with the system only through Taku's APIs.
* **Plan, not script** — a task is a list of steps the runner executes; preview
  it with `--dry-run`, skip unchanged work with the `unchanged` guard.
* **Built-in steps & APIs** — commands, filesystem, networking, environment
  variables, confirmations, and long-lived dev services (`serve`).
* **Parallel execution** — independent tasks run concurrently.

## Documentation

Full docs, in English & Russian: <https://taidaru.github.io/taku/> (sources in [website/](website/), built with Docusaurus).

## Install

**Linux & macOS** — the installer downloads the latest release for your platform:

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/taidaru/taku/releases/latest/download/taku-installer.sh | sh
```

**Homebrew** — from the [taidaru/homebrew-taku](https://github.com/taidaru/homebrew-taku) tap:

```sh
brew install taidaru/taku/taku
```

**Windows** — via the installer:

```powershell
powershell -ExecutionPolicy Bypass -c "irm https://github.com/taidaru/taku/releases/latest/download/taku-installer.ps1 | iex"
```

or [Scoop](https://scoop.sh), from the [taidaru/scoop-taku](https://github.com/taidaru/scoop-taku) bucket:

```powershell
scoop bucket add taku https://github.com/taidaru/scoop-taku
scoop install taku
```

**From source** (needs a Rust toolchain and a C compiler — Lua is built from source):

```sh
cargo install --path crates/taku
```

Prebuilt archives and `.msi` installers are also on the [GitHub releases](https://github.com/taidaru/taku/releases) page. Full details: [Installation docs](https://taidaru.github.io/taku/installation).

## Example

```lua
--- generate sources
task "gen" {
    write { "1.0.0", to = "version.txt" },
}

--- build the project (skips itself when the inputs did not change)
task "build: gen" {
    unchanged { "src/**/*.rs", outputs = "target" },
    "cargo build",
}

--- run tests
task "test: build" {
    "cargo test",
}

--- run clippy
task "lint: build" {
    "cargo clippy",
}

--- run all checks
task "check: test lint" {}
```

Running

```sh
taku run check
```

first runs **gen** and **build**, then **test** and **lint** in parallel.
A task header is `"name <param=default>: deps"`; `${param}` placeholders and
`$ENV_VAR` references are resolved by the runner, and a bare string step is a
command (tokenized, no shell involved).

## Usage

```sh
taku init                     # create a starter Takufile.lua
taku list                     # list tasks with their docs (alias: ls)
taku run <task>               # run a task and its dependencies (alias: r)
taku run <task> --dry-run     # print the plan without executing anything
taku run <task> --vars k=v    # set a task parameter
taku run <task> --force       # ignore `unchanged` state and rebuild
taku run <task> --explain     # say why a guard skipped or rebuilt
taku run <task> --yes         # auto-answer `confirm` steps
taku run <task> -j N          # limit parallelism (default: CPU count)
```

Taku looks for `Takufile.lua` in the current directory.

## Available APIs

Bare verbs in a task body build **steps** (executed by the runner): commands,
`argv`/`pipe`, `rm`/`mkdir`/`cp`/`mv`/`write`/`append`, `download`, `echo`,
`confirm`, `invoke`, `unchanged`, `serve`. Inside `function(ctx)` steps the
module APIs perform effects directly:

* `cmd` — run processes (`run`, `try`, `capture`)
* `fs` — filesystem operations
* `net` — networking
* `env` — environment variables (with `.env` autoloading)

Standard Lua `io` and `os` libraries are disabled.

## Development

```sh
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets
```

The workspace consists of:

* `taku` — CLI executable
* `taku-runtime` — Lua runtime and task scheduler
* `taku-api` — shared registration plumbing for the API crates
* `taku-fs`
* `taku-cmd`
* `taku-net`
* `taku-env`
* `taku-ops`

## FAQ

### Why Lua?

Lua is small, fast, easy to embed, and provides an expressive scripting language
without requiring a full programming runtime.

### Is running a Takufile safe?

Only to the same extent as running a Makefile or a shell script.

The Lua runtime is sandboxed: standard libraries such as `io` and `os` are
disabled, and tasks can only access the operating system through Taku's APIs.
However, those APIs are intentionally capable of executing commands, modifying
files, and accessing the network.

Always review an untrusted `Takufile.lua` before running it.

## License

MIT
