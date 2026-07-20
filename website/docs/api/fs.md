# fs — filesystem

Filesystem access for `function(ctx)` steps. For file operations as task steps
(`rm`, `mkdir`, `cp`, `mv`, `write`, `append`) see
[Steps](../guide/steps.md).

```lua
function(ctx)
    local text = fs.read("Cargo.toml")
    fs.write("out.txt", text)
    for _, path in ipairs(fs.glob("src/**/*.rs")) do print(path) end
end
```

| Function | Result |
|---|---|
| `fs.read(path)` | file contents (bytes) |
| `fs.write(path, data)` | write `data`, creating/truncating |
| `fs.append(path, data)` | append `data`, creating if needed |
| `fs.exists(path)` | `true` / `false` |
| `fs.is_file(path)` | `true` / `false` |
| `fs.is_dir(path)` | `true` / `false` |
| `fs.mkdir(path)` | create the directory and any parents |
| `fs.rm(path)` | remove a file, or a directory recursively |
| `fs.cp(src, dst)` | copy a file |
| `fs.mv(src, dst)` | move / rename |
| `fs.ls(path)` | entry names, sorted |
| `fs.glob(pattern)` | matching paths, sorted; `**` recurses |

Contents are byte strings — binary files round-trip unchanged. Reads work at
load time too; writes require a running task.
