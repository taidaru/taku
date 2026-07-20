# fs — файловая система

Доступ к файловой системе для `function(ctx)`-шагов. Про файловые операции как
шаги таска (`rm`, `mkdir`, `cp`, `mv`, `write`, `append`) см.
[Шаги](../guide/steps.md).

```lua
function(ctx)
    local text = fs.read("Cargo.toml")
    fs.write("out.txt", text)
    for _, path in ipairs(fs.glob("src/**/*.rs")) do print(path) end
end
```

| Функция | Результат |
|---|---|
| `fs.read(path)` | содержимое файла (байты) |
| `fs.write(path, data)` | записать `data`, создавая/усекая |
| `fs.append(path, data)` | дописать `data`, создав при необходимости |
| `fs.exists(path)` | `true` / `false` |
| `fs.is_file(path)` | `true` / `false` |
| `fs.is_dir(path)` | `true` / `false` |
| `fs.mkdir(path)` | создать каталог и родителей |
| `fs.rm(path)` | удалить файл или каталог рекурсивно |
| `fs.cp(src, dst)` | скопировать файл |
| `fs.mv(src, dst)` | переместить / переименовать |
| `fs.ls(path)` | имена записей, отсортированы |
| `fs.glob(pattern)` | подходящие пути, отсортированы; `**` рекурсивен |

Содержимое — байтовые строки, бинарные файлы проходят без искажений. Чтения
работают и при загрузке; записи требуют исполняющегося таска.
