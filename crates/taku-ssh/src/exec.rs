use std::io::{self, Read};
use std::process::{Output, Stdio};

use taku_shell::{wait_status_with_input, wait_with_input};

use crate::host::Host;
use crate::tunnel::Tunnel;
use crate::util::remote_failure;

impl Host {
    pub(crate) fn exec(&self, remote: &str, stdin: Option<&[u8]>) -> io::Result<Output> {
        let mut cmd = self.ssh_command(remote);
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
        cmd.stdin(if stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        });
        let child = cmd.spawn()?;
        wait_with_input(child, stdin)
    }

    pub(crate) fn run_streaming(&self, remote: &str, stdin: Option<&[u8]>) -> mlua::Result<i32> {
        let ctx = || format!("ssh.run({}, {remote})", self.destination());
        let mut cmd = self.ssh_command(remote);
        if stdin.is_some() {
            cmd.stdin(Stdio::piped());
        }
        let child = cmd.spawn().map_err(|e| self.spawn_error(&ctx(), &e))?;
        let status =
            wait_status_with_input(child, stdin).map_err(|e| self.spawn_error(&ctx(), &e))?;
        Ok(status.code().unwrap_or(-1))
    }

    pub(crate) fn checked(
        &self,
        ctx: &str,
        remote: &str,
        stdin: Option<&[u8]>,
    ) -> mlua::Result<Vec<u8>> {
        let out = self
            .exec(remote, stdin)
            .map_err(|e| self.spawn_error(ctx, &e))?;
        if !out.status.success() {
            return Err(remote_failure(ctx, &out));
        }
        Ok(out.stdout)
    }

    /// Streams `input` into the remote command's stdin (for bodies too large
    /// to buffer). The command is expected to produce no stdout of its own
    /// (`cat > file`), so a sequential copy cannot deadlock on a full pipe.
    pub(crate) fn checked_stream(
        &self,
        ctx: &str,
        remote: &str,
        input: &mut dyn Read,
    ) -> mlua::Result<()> {
        let mut cmd = self.ssh_command(remote);
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = cmd.spawn().map_err(|e| self.spawn_error(ctx, &e))?;
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| mlua::Error::external(format!("{ctx}: stdin was not piped")))?;
        let copied = io::copy(input, &mut stdin);
        drop(stdin); // EOF for the remote command
        let out = child
            .wait_with_output()
            .map_err(|e| self.spawn_error(ctx, &e))?;
        if !out.status.success() {
            return Err(remote_failure(ctx, &out));
        }
        copied.map_err(|e| crate::util::ext(ctx, e))?;
        Ok(())
    }

    /// Runs a remote `test`-style command and maps its exit code: 0 → true,
    /// 1 → false. Anything else (ssh itself exits 255 when the connection or
    /// auth fails) is an error, so a network failure can't read as "false".
    pub(crate) fn test(&self, ctx: &str, remote: &str) -> mlua::Result<bool> {
        let out = self
            .exec(remote, None)
            .map_err(|e| self.spawn_error(ctx, &e))?;
        match out.status.code() {
            Some(0) => Ok(true),
            Some(1) => Ok(false),
            _ => Err(remote_failure(ctx, &out)),
        }
    }

    pub(crate) fn try_output(&self, ctx: &str, remote: &str) -> mlua::Result<(bool, Vec<u8>)> {
        let out = self
            .exec(remote, None)
            .map_err(|e| self.spawn_error(ctx, &e))?;
        Ok((out.status.success(), out.stdout))
    }

    pub(crate) fn open_tunnel(&self, host: &str, port: u16) -> mlua::Result<Tunnel> {
        let ctx = format!("ssh -W {host}:{port}");
        let mut cmd = self.ssh_base();
        cmd.arg("-W").arg(format!("{host}:{port}"));
        cmd.arg("--");
        cmd.arg(self.destination());
        cmd.stdin(Stdio::piped()).stdout(Stdio::piped());
        let mut child = cmd.spawn().map_err(|e| self.spawn_error(&ctx, &e))?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| mlua::Error::external(format!("{ctx}: stdin was not piped")))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| mlua::Error::external(format!("{ctx}: stdout was not piped")))?;
        Ok(Tunnel::new(child, stdin, stdout))
    }
}
