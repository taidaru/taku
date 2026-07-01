use std::io::{self, Read, Write};
use std::process::{Child, ChildStdin, ChildStdout};

pub(crate) struct Tunnel {
    child: Child,
    stdin: ChildStdin,
    stdout: ChildStdout,
}

impl Tunnel {
    pub(crate) fn new(child: Child, stdin: ChildStdin, stdout: ChildStdout) -> Tunnel {
        Tunnel {
            child,
            stdin,
            stdout,
        }
    }
}

impl Read for Tunnel {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        self.stdout.read(buf)
    }
}

impl Write for Tunnel {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.stdin.write(buf)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.stdin.flush()
    }
}

impl Drop for Tunnel {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
