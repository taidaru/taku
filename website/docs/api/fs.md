# fs — filesystem

```lua
fs.write("out.txt", "hello")
local text = fs.read("out.txt")
for _, name in ipairs(fs.read_dir(".")) do print(name) end
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
| `fs.remove(path)` | remove a file, or a directory recursively |
| `fs.copy(src, dst)` | copy a file |
| `fs.rename(src, dst)` | move / rename |
| `fs.read_dir(path)` | list of entry names, sorted |

Contents are byte strings, so binary files round-trip unchanged.
