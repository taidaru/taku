# cmd — команды

Выполняет команду **сейчас**, внутри `function(ctx)`-шага. Про команды как
шаги таска — голые строки, `argv`, `pipe` — см. [Шаги](../guide/steps.md).

```lua
function(ctx)
    cmd.run({ "cargo", "build" })                       -- ошибка при ненулевом коде
    local code = cmd.try({ "git", "diff", "--quiet" })  -- возвращает код завершения
    local r = cmd.capture({ "git", "rev-parse", "HEAD" })
    print(r.code, r.stdout, r.stderr)
end
```

| Функция | Поведение |
|---|---|
| `cmd.run(argv [, opts])` | стримит stdio; ненулевой код — ошибка |
| `cmd.try(argv [, opts])` | стримит stdio; возвращает код завершения |
| `cmd.capture(argv [, opts])` | возвращает `{ code, stdout, stderr }` |

- Команда — argv-таблица: `{ "prog", "arg", ... }`. Голая строка отклоняется —
  здесь нет шелла, чтобы её разобрать. Для возможностей шелла запустите его
  явно: `{ "sh", "-c", "..." }`.
- `opts`: `cwd`, `env = {...}`, `stdin`, `timeout` (секунды; по истечении
  процесс убивается и поднимается ошибка).
- `stdout`/`stderr` из `capture` — байтовые строки.
