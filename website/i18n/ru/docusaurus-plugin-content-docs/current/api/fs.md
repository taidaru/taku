# fs — файловая система

```lua
fs.write("out.txt", "привет")
local text = fs.read("out.txt")
for _, name in ipairs(fs.read_dir(".")) do print(name) end
```

| Функция | Результат |
|---|---|
| `fs.read(path)` | содержимое файла (байты) |
| `fs.write(path, data)` | записать `data`, создав/обрезав файл |
| `fs.append(path, data)` | дописать `data`, создав файл при необходимости |
| `fs.exists(path)` | `true` / `false` |
| `fs.is_file(path)` | `true` / `false` |
| `fs.is_dir(path)` | `true` / `false` |
| `fs.mkdir(path)` | создать каталог и родительские |
| `fs.remove(path)` | удалить файл или каталог рекурсивно |
| `fs.copy(src, dst)` | скопировать файл |
| `fs.rename(src, dst)` | переместить / переименовать |
| `fs.read_dir(path)` | список имён в каталоге, отсортирован |

Содержимое — байтовые строки, поэтому бинарные файлы проходят без искажений.
