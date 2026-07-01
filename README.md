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
* **Built-in APIs** — filesystem, processes, networking, SSH, and environment
  variables.
* **Parallel execution** — independent tasks run concurrently.

## Documentation

Full docs, in English & Russian: <https://taidaru.github.io/taku/> (sources in [website/](website/), built with Docusaurus).

## Install

**Linux & macOS** — download the latest release and install the binary:

```sh
curl -fsSL https://raw.githubusercontent.com/taidaru/taku/main/install.sh | sh
```

**Windows** — the repo is a [Scoop](https://scoop.sh) bucket:

```powershell
scoop bucket add taku https://github.com/taidaru/taku
scoop install taku
```

**From source** (needs a Rust toolchain and a C compiler — Lua is built from source):

```sh
cargo install --path crates/taku
```

You can also grab a prebuilt archive from the [GitHub releases](https://github.com/taidaru/taku/releases). Full details: [Installation docs](https://taidaru.github.io/taku/installation).

## Example

```lua
task("build", {
    desc = "compile the project",
    run = function()
        if sh.run({ "cargo", "build" }) ~= 0 then
            error("build failed")
        end
    end,
})

task("test", {
    desc = "run tests",
    deps = { "build" },
    run = function()
        sh.run({ "cargo", "test" })
    end,
})

task("lint", {
    desc = "run clippy",
    run = function()
        sh.run({ "cargo", "clippy" })
    end,
})

task("check", {
    desc = "run all checks",
    deps = { "test", "lint" },
    run = function() end, -- run is required
})
```

Running

```sh
taku run check
```

first builds the project, then runs **test** and **lint** in parallel.

## Usage

```sh
taku init              # create a starter Takufile.lua
taku list              # list available tasks (alias: ls)
taku run <task>        # run a task and its dependencies (alias: r)
taku run <task> -j N   # limit parallelism (default: CPU count)
```

Taku searches the current directory and its parents for `Takufile.lua`.

## Available APIs

The sandbox exposes only Taku's APIs:

* `fs` — filesystem operations
* `sh` — execute processes
* `net` — networking
* `ssh` — remote execution over SSH
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
* `taku-fs`
* `taku-shell`
* `taku-net`
* `taku-ssh`
* `taku-env`

## FAQ

### Why Lua?

Lua is small, fast, easy to embed, and provides an expressive scripting language
without requiring a full programming runtime.

### Is running a Takufile safe?

Only to the same extent as running a Makefile or a shell script.

The Lua runtime is sandboxed: standard libraries such as `io` and `os` are
disabled, and tasks can only access the operating system through Taku's APIs.
However, those APIs are intentionally capable of executing commands, modifying
files, accessing the network, or connecting over SSH.

Always review an untrusted `Takufile.lua` before running it.

## License

MIT
