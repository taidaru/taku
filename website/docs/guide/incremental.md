# Incremental tasks

`unchanged` is a guard step: when nothing relevant changed since the last
successful run, the steps after it are skipped.

```lua
--- compile protobuf definitions
task "gen" {
    unchanged { "proto/**/*.proto", outputs = "src/generated" },
    "protoc --rust_out src/generated proto/*.proto",
}
```

First run: the guard records a fingerprint, the steps run. Next runs: if the
fingerprint still matches **and** the `outputs` exist, the rest of the task is
skipped. Steps placed *before* the guard always run.

## What the fingerprint covers

| Part | Changes when |
|---|---|
| input files | a file matched by the globs is added, removed, or modified (size or mtime — contents are not read, so checks stay fast on large trees) |
| step plan | the task's steps themselves change |
| vars | a parameter or `--vars` override changes |
| environment | any environment variable changes (a tool you invoke may read any of them) |

Notes:

- Positional entries are globs, relative to the Takufile directory; `**`
  recurses.
- `outputs` is a path or a list of paths. Missing outputs force a rebuild even
  with a matching fingerprint — deleting build artifacts "just works".
- Because files are tracked by size + mtime, a `touch` causes one extra
  rebuild. That errs on the safe side.
- Variables set by a function step *below* the guard are not part of the key.
- State lives in `.taku/state/<task>.bin`, written only after the whole task
  succeeds. A failed run never records state.

## Flags

```sh
taku run gen --explain   # taku: gen: skip (unchanged)
                         # taku: gen: rebuild (input files changed)
taku run gen --force     # ignore the stored state, run everything
```

`--explain` names the part that changed; `--force` rebuilds and records a
fresh fingerprint.
