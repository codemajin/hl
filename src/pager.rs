use std::{
    env,
    io::{self, Write},
    process::{Child, ChildStdin, Command, Stdio},
};

/// A running pager process whose stdin receives our output.
///
/// Dropping it closes the stdin and waits for the pager, so the shell prompt
/// never comes back while the pager still owns the terminal.
pub struct Pager {
    child: Child,
    stdin: Option<ChildStdin>,
}

impl Pager {
    /// Starts `$PAGER`, falling back to `less -RX`. With `quit_if_one_screen`
    /// the fallback is `less -RFX`, which exits at once when the output fits
    /// on one screen; `$PAGER` is run as given either way.
    pub fn spawn(quit_if_one_screen: bool) -> io::Result<Self> {
        let (program, args) = match env::var("PAGER") {
            Ok(pager) if !pager.trim().is_empty() => {
                let mut parts = pager.split_whitespace().map(String::from);
                let program = parts.next().unwrap_or_default();
                (program, parts.collect())
            }
            _ => {
                let flags = if quit_if_one_screen { "-RFX" } else { "-RX" };
                ("less".to_string(), vec![flags.to_string()])
            }
        };
        let mut child = Command::new(program)
            .args(args)
            .stdin(Stdio::piped())
            .spawn()?;
        let stdin = child.stdin.take();
        Ok(Self { child, stdin })
    }

    pub fn stdin(&mut self) -> &mut ChildStdin {
        self.stdin.as_mut().expect("pager stdin is piped")
    }

    /// Closes the pager's stdin and waits for the user to quit it, reporting
    /// a failure to wait that `Drop` would have to ignore.
    pub fn finish(mut self) -> io::Result<()> {
        self.close_stdin();
        self.child.wait()?;
        Ok(())
    }

    fn close_stdin(&mut self) {
        if let Some(mut stdin) = self.stdin.take() {
            let _ = stdin.flush();
        }
    }
}

impl Drop for Pager {
    fn drop(&mut self) {
        self.close_stdin();
        let _ = self.child.wait();
    }
}
