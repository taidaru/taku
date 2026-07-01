use std::collections::HashMap;
use std::io;
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;

use mlua::{Table, Value};

use crate::util::{ASKPASS_PASSWORD_ENV, ext};

#[derive(Clone)]
pub(crate) struct Host {
    host: String,
    user: Option<String>,
    port: Option<u16>,
    key: Option<String>,
    password: Option<String>,
    askpass: Option<PathBuf>,
    options: Vec<String>,
    dotenv: Arc<HashMap<String, String>>,
}

impl Host {
    pub(crate) fn from_value(value: Value) -> mlua::Result<Host> {
        match value {
            Value::String(s) => Ok(Host::from_target(&s.to_str()?)),
            Value::Table(t) => Host::from_table(&t),
            other => Err(mlua::Error::external(format!(
                "ssh: expected a \"user@host\" string or an options table, got {}",
                other.type_name()
            ))),
        }
    }

    fn from_target(target: &str) -> Host {
        let (user, rest) = match target.split_once('@') {
            Some((u, r)) => (Some(u.to_string()), r),
            None => (None, target),
        };
        let (host, port) = match rest.rsplit_once(':') {
            Some((h, p)) if !h.is_empty() => match p.parse::<u16>() {
                Ok(port) => (h.to_string(), Some(port)),
                Err(_) => (rest.to_string(), None),
            },
            _ => (rest.to_string(), None),
        };
        Host {
            host,
            user,
            port,
            key: None,
            password: None,
            askpass: None,
            options: Vec::new(),
            dotenv: Arc::new(HashMap::new()),
        }
    }

    pub(crate) fn with_dotenv(mut self, dotenv: Arc<HashMap<String, String>>) -> Host {
        self.dotenv = dotenv;
        self
    }

    pub(crate) fn dotenv(&self) -> &HashMap<String, String> {
        &self.dotenv
    }

    fn from_table(t: &Table) -> mlua::Result<Host> {
        let host: String = t.get("host").map_err(|_| {
            mlua::Error::external("ssh: options table is missing the required `host` field")
        })?;
        let mut h = Host::from_target(&host);
        if let Some(user) = t.get::<Option<String>>("user")? {
            h.user = Some(user);
        }
        if let Some(port) = t.get::<Option<u16>>("port")? {
            h.port = Some(port);
        }
        h.key = t.get::<Option<String>>("key")?;
        h.password = t.get::<Option<String>>("password")?;
        if h.password.is_some() {
            h.askpass = Some(std::env::current_exe().map_err(|e| {
                ext(
                    "ssh: cannot locate the taku executable for password auth",
                    e,
                )
            })?);
        }
        if let Some(options) = t.get::<Option<Table>>("options")? {
            h.options = options
                .sequence_values::<String>()
                .collect::<mlua::Result<_>>()?;
        }
        Ok(h)
    }

    pub(crate) fn destination(&self) -> String {
        match &self.user {
            Some(user) => format!("{user}@{}", self.host),
            None => self.host.clone(),
        }
    }

    pub(crate) fn ssh_base(&self) -> Command {
        let mut cmd = Command::new("ssh");
        if let Some(port) = self.port {
            cmd.arg("-p").arg(port.to_string());
        }
        if let Some(key) = &self.key {
            cmd.arg("-i").arg(key);
        }
        for opt in &self.options {
            cmd.arg("-o").arg(opt);
        }
        if let Some(password) = &self.password {
            cmd.env(ASKPASS_PASSWORD_ENV, password);
            if let Some(helper) = &self.askpass {
                cmd.env("SSH_ASKPASS", helper);
            }
        }
        cmd
    }

    pub(crate) fn ssh_command(&self, remote: &str) -> Command {
        let mut cmd = self.ssh_base();
        cmd.arg(self.destination());
        cmd.arg(remote);
        cmd
    }

    pub(crate) fn spawn_error(&self, ctx: &str, e: &io::Error) -> mlua::Error {
        if e.kind() == io::ErrorKind::NotFound {
            return mlua::Error::external(format!(
                "{ctx}: `ssh` not found — the OpenSSH client must be installed"
            ));
        }
        ext(ctx, e)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;

    #[test]
    fn parses_user_host_port() {
        let h = Host::from_target("deploy@host:2222");
        assert_eq!(h.user.as_deref(), Some("deploy"));
        assert_eq!(h.host, "host");
        assert_eq!(h.port, Some(2222));
    }

    #[test]
    fn parses_bare_host() {
        let h = Host::from_target("host");
        assert!(h.user.is_none());
        assert_eq!(h.host, "host");
        assert!(h.port.is_none());
    }

    #[test]
    fn non_numeric_port_is_kept_in_host() {
        let h = Host::from_target("user@example.com:notaport");
        assert_eq!(h.user.as_deref(), Some("user"));
        assert_eq!(h.host, "example.com:notaport");
        assert!(h.port.is_none());
    }

    #[test]
    fn destination_round_trips() {
        assert_eq!(Host::from_target("a@b").destination(), "a@b");
        assert_eq!(Host::from_target("b").destination(), "b");
    }

    #[test]
    fn without_password_sets_no_askpass_env() {
        let cmd = Host::from_target("user@host").ssh_command("uptime");
        assert_eq!(cmd.get_program(), "ssh");
        let envs: Vec<_> = cmd.get_envs().collect();
        assert!(!envs.iter().any(
            |(k, _)| *k == OsStr::new("SSH_ASKPASS") || *k == OsStr::new(ASKPASS_PASSWORD_ENV)
        ));
    }

    #[test]
    fn password_rides_in_askpass_env_not_argv() {
        let mut h = Host::from_target("user@host");
        h.password = Some("secret".into());
        h.askpass = Some(PathBuf::from("/path/to/taku"));
        let cmd = h.ssh_command("uptime");

        assert_eq!(cmd.get_program(), "ssh");
        let args: Vec<String> = cmd
            .get_args()
            .map(|a| a.to_string_lossy().into_owned())
            .collect();
        assert!(!args.iter().any(|a| a.contains("secret")));
        let has_env = |k: &str| cmd.get_envs().any(|(name, _)| name == OsStr::new(k));
        assert!(has_env(ASKPASS_PASSWORD_ENV));
        assert!(has_env("SSH_ASKPASS"));
        assert!(!has_env("SSH_ASKPASS_REQUIRE"));
    }
}
