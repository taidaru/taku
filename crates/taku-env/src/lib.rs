use std::collections::HashMap;
use std::sync::Arc;

pub const API: taku_api::ApiEntry = taku_api::ApiEntry {
    globals: &["env"],
    register: |lua, ctx| register(lua, ctx.dotenv.clone()),
    steps: &[],
};

/// The real process env wins; the project `.env` only fills what's unset.
pub fn get(dotenv: &HashMap<String, String>, name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .or_else(|| dotenv.get(name).cloned())
}

pub fn register(lua: &mlua::Lua, dotenv: Arc<HashMap<String, String>>) -> mlua::Result<()> {
    let map = dotenv.clone();
    taku_api::lua_api!(lua, global = "env" {
        get => move |_, (name, default): (String, Option<String>)| {
            Ok(get(&map, &name).or(default))
        },
        require => move |_, name: String| {
            get(&dotenv, &name).ok_or_else(|| {
                taku_api::Diag::new(format!(
                    "environment variable '{name}' is required but not set"
                ))
                .help(format!("'export {name}=...' or 'echo \"{name}=...\" >> .env'"))
                .into_lua()
            })
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn real_env_takes_precedence_over_dotenv() {
        let mut dotenv = HashMap::new();
        dotenv.insert("PATH".to_string(), "from-dotenv".to_string());
        dotenv.insert(
            "TAKU_ENV_TEST_ONLY_DOTENV".to_string(),
            "fallback".to_string(),
        );
        assert_eq!(
            get(&dotenv, "PATH").as_deref(),
            Some(std::env::var("PATH").unwrap().as_str())
        );
        assert_eq!(
            get(&dotenv, "TAKU_ENV_TEST_ONLY_DOTENV"),
            Some("fallback".to_string())
        );
    }
}
