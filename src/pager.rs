use std::env;
use std::io::{self, Write};
use std::process::{Child, ChildStdin, Command, Stdio};

/// A running pager process whose stdin receives our output.
pub struct Pager {
    child: Child,
}

impl Pager {
    /// Starts `$PAGER`, falling back to `less -RFX`.
    pub fn spawn() -> io::Result<Self> {
        let (program, args) = match env::var("PAGER") {
            Ok(pager) if !pager.trim().is_empty() => {
                let mut parts = pager.split_whitespace().map(String::from);
                let program = parts.next().unwrap_or_default();
                (program, parts.collect())
            }
            _ => ("less".to_string(), vec!["-RFX".to_string()]),
        };
        let child = Command::new(program)
            .args(args)
            .stdin(Stdio::piped())
            .spawn()?;
        Ok(Self { child })
    }

    pub fn stdin(&mut self) -> &mut ChildStdin {
        self.child.stdin.as_mut().expect("pager stdin is piped")
    }

    /// Closes the pager's stdin and waits for the user to quit it.
    pub fn finish(mut self) -> io::Result<()> {
        if let Some(mut stdin) = self.child.stdin.take() {
            let _ = stdin.flush();
        }
        self.child.wait()?;
        Ok(())
    }
}
