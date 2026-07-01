# ssh — удалённый хост

Удобная обёртка над клиентом OpenSSH `ssh`, поэтому ключи, ssh-agent,
`~/.ssh/config` и `known_hosts` работают так же, как в командной строке.

## Разовые команды

```lua
ssh.run("deploy@host", { "uptime" })             -- код выхода
local r = ssh.capture("deploy@host", { "hostname" })
```

## Целый блок на удалённой машине

Внутри `ssh.on` обычные глобалы `sh`/`fs`/`net`/`env` на время блока действуют на
**удалённом** хосте, а затем восстанавливаются:

```lua
ssh.on({ host = "host", user = "deploy" }, function()
    fs.write("/srv/app/config.toml", body)               -- файл на удалёнке
    sh.run({ "systemctl", "--user", "restart", "app" })  -- команда на удалёнке
    local v = net.http_get("https://internal/health")    -- запрос ИЗ хоста
end)
```

## Цель (target)

Строка `"[user@]host[:port]"` или таблица:

```lua
{ host = "h", user = "u", port = 22, key = "~/.ssh/id_ed25519",
  password = "...", options = { "StrictHostKeyChecking=yes" } }
```

По умолчанию аутентификация идёт через ключи/agent. С `password` пароль
передаётся `ssh` через `SSH_ASKPASS` (никогда не в командной строке); запрос
подтверждения `known_hosts` по-прежнему идёт в ваш терминал.

:::note
Удалённые `fs`/`env` работают через coreutils (`cat`, `test`, `printenv`, ...);
удалённый `net` туннелируется через `ssh -W`. На удалёнке не нужны `curl`/`nc`.
:::

:::note
Локальный [`.env`](env#автозагрузка-env) действует и внутри `ssh.on`: если
переменная не задана в окружении удалённого хоста, `env.get` берёт значение из
`.env` проекта. Реальное окружение удалёнки имеет приоритет, а фолбэк резолвится
локально — ничего из `.env` на хост не отправляется.
:::
