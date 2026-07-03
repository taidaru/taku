use std::io::{self, Read, Write};
use std::sync::Arc;

use taku_shell::{Capture, Opts};

use crate::host::Host;
use crate::util::{ext, shq};

fn remote_command(argv: &[String], opts: &Opts) -> String {
    let quoted = argv.iter().map(|a| shq(a)).collect::<Vec<_>>().join(" ");
    let body = if opts.env.is_empty() {
        quoted
    } else {
        // Quote the whole `NAME=VALUE` word, not just the value: the remote `env`
        // utility splits on the first `=`, so a name with shell metacharacters
        // must not be able to break out of the argument.
        let assignments = opts
            .env
            .iter()
            .map(|(k, v)| shq(&format!("{k}={v}")))
            .collect::<Vec<_>>()
            .join(" ");
        format!("env {assignments} -- {quoted}")
    };
    match &opts.cwd {
        Some(cwd) => format!("cd {} && {body}", shq(cwd)),
        None => body,
    }
}

impl taku_shell::Shell for Host {
    fn run(&self, argv: &[String], opts: &Opts) -> mlua::Result<i32> {
        self.run_streaming(&remote_command(argv, opts), opts.stdin.as_deref())
    }

    fn capture(&self, argv: &[String], opts: &Opts) -> mlua::Result<Capture> {
        let remote = remote_command(argv, opts);
        let out = self
            .exec(&remote, opts.stdin.as_deref())
            .map_err(|e| self.spawn_error(&format!("ssh.capture({})", argv.join(" ")), &e))?;
        Ok(Capture {
            code: out.status.code().unwrap_or(-1),
            stdout: out.stdout,
            stderr: out.stderr,
        })
    }
}

impl taku_fs::FileSystem for Host {
    fn read(&self, path: &str) -> mlua::Result<Vec<u8>> {
        self.checked(
            &format!("fs.read({path})"),
            &format!("cat -- {}", shq(path)),
            None,
        )
    }
    fn write(&self, path: &str, contents: &[u8]) -> mlua::Result<()> {
        self.checked(
            &format!("fs.write({path})"),
            &format!("cat > {}", shq(path)),
            Some(contents),
        )?;
        Ok(())
    }
    fn append(&self, path: &str, contents: &[u8]) -> mlua::Result<()> {
        self.checked(
            &format!("fs.append({path})"),
            &format!("cat >> {}", shq(path)),
            Some(contents),
        )?;
        Ok(())
    }
    fn exists(&self, path: &str) -> mlua::Result<bool> {
        self.test(
            &format!("fs.exists({path})"),
            &format!("test -e {}", shq(path)),
        )
    }
    fn is_file(&self, path: &str) -> mlua::Result<bool> {
        self.test(
            &format!("fs.is_file({path})"),
            &format!("test -f {}", shq(path)),
        )
    }
    fn is_dir(&self, path: &str) -> mlua::Result<bool> {
        self.test(
            &format!("fs.is_dir({path})"),
            &format!("test -d {}", shq(path)),
        )
    }
    fn mkdir(&self, path: &str) -> mlua::Result<()> {
        self.checked(
            &format!("fs.mkdir({path})"),
            &format!("mkdir -p -- {}", shq(path)),
            None,
        )?;
        Ok(())
    }
    fn remove(&self, path: &str) -> mlua::Result<()> {
        self.checked(
            &format!("fs.remove({path})"),
            &format!("rm -rf -- {}", shq(path)),
            None,
        )?;
        Ok(())
    }
    fn copy(&self, src: &str, dst: &str) -> mlua::Result<()> {
        self.checked(
            &format!("fs.copy({src} -> {dst})"),
            &format!("cp -- {} {}", shq(src), shq(dst)),
            None,
        )?;
        Ok(())
    }
    fn rename(&self, src: &str, dst: &str) -> mlua::Result<()> {
        self.checked(
            &format!("fs.rename({src} -> {dst})"),
            &format!("mv -- {} {}", shq(src), shq(dst)),
            None,
        )?;
        Ok(())
    }
    fn read_dir(&self, path: &str) -> mlua::Result<Vec<String>> {
        let out = self.checked(
            &format!("fs.read_dir({path})"),
            &format!("ls -1A -- {}", shq(path)),
            None,
        )?;
        Ok(String::from_utf8_lossy(&out)
            .lines()
            .filter(|l| !l.is_empty())
            .map(str::to_string)
            .collect())
    }
}

impl taku_env::Env for Host {
    fn get(&self, name: &str) -> mlua::Result<Option<String>> {
        let (set, stdout) = self.try_output(
            &format!("env.get({name})"),
            &format!("printenv -- {}", shq(name)),
        )?;
        if !set {
            return Ok(self.dotenv().get(name).cloned());
        }
        let mut value = String::from_utf8_lossy(&stdout).into_owned();
        if value.ends_with('\n') {
            value.pop();
        }
        Ok(Some(value))
    }
}

impl taku_net::http::Dialer for Host {
    fn dial(&self, host: &str, port: u16) -> io::Result<Box<dyn taku_net::http::Stream>> {
        let tunnel = self
            .open_tunnel(host, port)
            .map_err(|e| io::Error::other(e.to_string()))?;
        Ok(Box::new(tunnel))
    }
}

impl taku_net::Net for Host {
    fn tcp_request(&self, host: &str, port: u16, data: &[u8]) -> mlua::Result<Vec<u8>> {
        let ctx = format!("net.tcp_request({host}:{port})");
        let mut tunnel = self.open_tunnel(host, port)?;
        tunnel
            .write_all(data)
            .and_then(|_| tunnel.flush())
            .map_err(|e| ext(&ctx, e))?;
        let mut buf = Vec::new();
        tunnel.read_to_end(&mut buf).map_err(|e| ext(&ctx, e))?;
        Ok(buf)
    }

    fn http_get(&self, url: &str) -> mlua::Result<Vec<u8>> {
        let agent = taku_net::http::dialer_agent(Arc::new(self.clone()));
        taku_net::http::get(&agent, url).map_err(|e| ext(&format!("net.http_get({url})"), e))
    }

    fn download(&self, url: &str, path: &str) -> mlua::Result<()> {
        let agent = taku_net::http::dialer_agent(Arc::new(self.clone()));
        let mut body = taku_net::http::get_reader(&agent, url)
            .map_err(|e| ext(&format!("net.download({url})"), e))?;
        self.checked_stream(
            &format!("net.download({url} -> {path})"),
            &format!("cat > {}", shq(path)),
            &mut body,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(args: &[&str]) -> Vec<String> {
        args.iter().map(|s| s.to_string()).collect()
    }

    fn opts(cwd: Option<&str>, env: &[(&str, &str)]) -> Opts {
        Opts {
            cwd: cwd.map(str::to_owned),
            env: env
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
            ..Opts::default()
        }
    }

    #[test]
    fn argv_is_quoted_element_by_element() {
        let cmd = argv(&["git", "commit", "-m", "a b; rm"]);
        assert_eq!(
            remote_command(&cmd, &Opts::default()),
            "'git' 'commit' '-m' 'a b; rm'"
        );
    }

    #[test]
    fn cwd_and_env_wrap_the_command() {
        let cmd = argv(&["make"]);
        assert_eq!(
            remote_command(&cmd, &opts(Some("/srv app"), &[("A", "x y")])),
            "cd '/srv app' && env 'A=x y' -- 'make'"
        );
    }

    #[test]
    fn env_names_with_metacharacters_cannot_inject() {
        let cmd = argv(&["make"]);
        let line = remote_command(&cmd, &opts(None, &[("A;touch /tmp/x", "v")]));
        assert_eq!(line, "env 'A;touch /tmp/x=v' -- 'make'");
        assert!(
            !line.contains("; "),
            "metacharacter escaped quoting: {line}"
        );
    }
}
