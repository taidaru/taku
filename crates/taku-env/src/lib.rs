use std::collections::HashMap;
use std::sync::Arc;

pub use dotenvy::Error as DotenvError;

pub trait Env: Send + Sync {
    fn get(&self, name: &str) -> mlua::Result<Option<String>>;
}

pub struct Local {
    dotenv: Arc<HashMap<String, String>>,
}

impl Local {
    pub fn new() -> Self {
        Local {
            dotenv: Arc::new(HashMap::new()),
        }
    }

    pub fn with_dotenv(dotenv: Arc<HashMap<String, String>>) -> Self {
        Local { dotenv }
    }
}

impl Default for Local {
    fn default() -> Self {
        Local::new()
    }
}

impl Env for Local {
    fn get(&self, name: &str) -> mlua::Result<Option<String>> {
        if let Ok(value) = std::env::var(name) {
            return Ok(Some(value));
        }
        Ok(self.dotenv.get(name).cloned())
    }
}

pub const API: taku_api::ApiEntry = taku_api::ApiEntry {
    global: "env",
    register: |lua, ctx| register(lua, Arc::new(Local::with_dotenv(ctx.dotenv.clone()))),
};

taku_api::lua_api! {
    pub fn register(global = "env", backend: Env as e) {
        get => |_, (name, default): (String, Option<String>)| Ok(e.get(&name)?.or(default)),
        require => |_, name: String| {
            e.get(&name)?.ok_or_else(|| {
                mlua::Error::external(format!("env.require('{name}'): variable is not set"))
            })
        },
    }
}

pub fn parse_dotenv(contents: &str) -> Result<HashMap<String, String>, dotenvy::Error> {
    dotenvy::from_read_iter(contents.as_bytes()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_pairs_comments_and_blanks() {
        let env = parse_dotenv("# a comment\n\nFOO=bar\nBAZ=qux\n").unwrap();
        assert_eq!(env.get("FOO"), Some(&"bar".to_string()));
        assert_eq!(env.get("BAZ"), Some(&"qux".to_string()));
        assert_eq!(env.len(), 2);
    }

    #[test]
    fn handles_export_prefix_and_quotes() {
        let env = parse_dotenv("export TOKEN=\"secret value\"\nLITERAL='no $expansion'\n").unwrap();
        assert_eq!(env.get("TOKEN"), Some(&"secret value".to_string()));
        assert_eq!(env.get("LITERAL"), Some(&"no $expansion".to_string()));
    }

    #[test]
    fn substitutes_from_earlier_file_var() {
        let env = parse_dotenv("HOST=example.com\nURL=${HOST}/api\n").unwrap();
        assert_eq!(env.get("URL"), Some(&"example.com/api".to_string()));
    }

    #[test]
    fn substitutes_from_process_env() {
        let path = std::env::var("PATH").unwrap();
        let env = parse_dotenv("REF=${PATH}\n").unwrap();
        assert_eq!(env.get("REF"), Some(&path));
    }

    #[test]
    fn malformed_line_is_an_error() {
        assert!(parse_dotenv("KEY=\"unterminated\n").is_err());
    }

    #[test]
    fn real_env_takes_precedence_over_dotenv() {
        let mut dotenv = HashMap::new();
        dotenv.insert("PATH".to_string(), "from-dotenv".to_string());
        dotenv.insert(
            "TAKU_ENV_TEST_ONLY_DOTENV".to_string(),
            "fallback".to_string(),
        );
        let local = Local::with_dotenv(Arc::new(dotenv));
        assert_eq!(
            local.get("PATH").unwrap().as_deref(),
            Some(std::env::var("PATH").unwrap().as_str())
        );
        assert_eq!(
            local.get("TAKU_ENV_TEST_ONLY_DOTENV").unwrap(),
            Some("fallback".to_string())
        );
    }
}
