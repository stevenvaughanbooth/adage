//! Reading a secret from the person at the terminal, echo off, with no dependencies: the
//! controlling terminal (`/dev/tty`) is put into no-echo mode with `stty`, exactly as a shell
//! script would, and its previous mode is restored afterwards whatever happens.
//!
//! Only when stdin is a terminal: under a pipe, in CI, or inside an agent's tool call there is no
//! one to ask, and the caller falls back (for signing: ssh-agent). Windows: not supported yet.

use adage_sign::Zeroizing;

/// Ask for a line on the controlling terminal without echoing it. `None` if there is no terminal.
pub fn secret(prompt: &str) -> Option<Zeroizing<String>> {
    use std::io::IsTerminal;
    if !std::io::stdin().is_terminal() {
        return None;
    }
    imp::secret(prompt)
}

#[cfg(unix)]
mod imp {
    use super::Zeroizing;
    use std::fs::{File, OpenOptions};
    use std::io::{BufRead, BufReader, Write};
    use std::process::{Command, Stdio};

    fn stty() -> Option<&'static str> {
        ["/bin/stty", "/usr/bin/stty"].into_iter().find(|p| std::path::Path::new(p).exists())
    }

    fn run(tty: &File, args: &[&str]) -> Option<String> {
        let out = Command::new(stty()?).args(args).stdin(tty.try_clone().ok()?).stderr(Stdio::null()).output().ok()?;
        out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
    }

    /// Restores the terminal's saved mode when dropped.
    struct Restore<'a>(&'a File, String);

    impl Drop for Restore<'_> {
        fn drop(&mut self) {
            let _ = run(self.0, &[&self.1]);
        }
    }

    pub fn secret(prompt: &str) -> Option<Zeroizing<String>> {
        let tty = OpenOptions::new().read(true).write(true).open("/dev/tty").ok()?;
        let saved = run(&tty, &["-g"])?;
        let restore = Restore(&tty, saved);
        run(&tty, &["-echo"])?;
        let mut w = &tty;
        write!(w, "{prompt}").ok()?;
        w.flush().ok()?;
        let mut line = Zeroizing::new(String::new());
        let read = BufReader::new(&tty).read_line(&mut line);
        drop(restore);
        let _ = writeln!(w);
        read.ok()?;
        let trimmed = line.trim_end_matches(['\r', '\n']).len();
        line.truncate(trimmed);
        Some(line)
    }
}

#[cfg(not(unix))]
mod imp {
    use super::Zeroizing;

    pub fn secret(_prompt: &str) -> Option<Zeroizing<String>> {
        None
    }
}
