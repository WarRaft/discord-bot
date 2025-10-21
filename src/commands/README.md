# Discord Commands

Модуль для Discord slash команд с поддержкой модульной архитектуры.

## Архитектура

- **`mod.rs`** - Главный модуль с трейтом `Command` и роутингом
- **`ahoy.rs`** - Пример команды (приветствие пирата)

## Добавление новой команды

1. Создайте новый файл, например `src/commands/ping.rs`:

```rust
use crate::commands::{Command, SlashCommand};
use crate::discord::api;
use crate::error::Result;
use crate::state;
use crate::types::discord::Interaction;

pub struct Ping;

impl Command for Ping {
    fn definition() -> SlashCommand {
        SlashCommand {
            name: "ping".to_string(),
            command_type: 1,
            description: "Check bot latency".to_string(),
        }
    }

    async fn handle(interaction: Interaction) -> Result<()> {
        let client = state::client().await;
        let token = state::token().await;

        api::respond_to_interaction(
            &client,
            &token,
            &interaction.id,
            &interaction.token,
            "Pong! 🏓".to_string(),
        )
        .await
    }
}
```

2. Добавьте модуль в `src/commands/mod.rs`:

```rust
mod ahoy;
mod ping;  // <-- добавить
```

3. Зарегистрируйте команду в функции `all_commands()`:

```rust
pub fn all_commands() -> Vec<SlashCommand> {
    vec![
        ahoy::Ahoy::definition(),
        ping::Ping::definition(),  // <-- добавить
    ]
}
```

4. Добавьте роутинг в функцию `handle_interaction()`:

```rust
match data.name.as_str() {
    "ahoy" => ahoy::Ahoy::handle(interaction).await,
    "ping" => ping::Ping::handle(interaction).await,  // <-- добавить
    _ => Ok(()),
}
```

5. Перерегистрируйте команды на сервере:

```bash
./reregister-commands.sh
```

## Трейт Command

Каждая команда должна реализовать трейт `Command`:

- **`definition()`** - Возвращает описание команды для регистрации в Discord API
- **`handle()`** - Асинхронный обработчик команды

## Автоматическая регистрация

Команды автоматически регистрируются при:
- Первом запуске бота
- Получении сигнала SIGUSR1 (`./reregister-commands.sh`)
