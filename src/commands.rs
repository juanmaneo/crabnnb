use std::cell::RefCell;
use std::io;
use std::process::Command;

pub const DEFAULT_LOCK_CMD: &str = "(xscreensaver-command --time 2>&1|grep -q \"no screensaver is running\" && xscreensaver &); xscreensaver-command -lock";

pub const DEFAULT_UNLOCK_CMD: &str = "(xscreensaver-command -time 2>&1|grep -q \"screen non-blanked since\" && xscreensaver-command -deactivate || killall xscreensaver); (xscreensaver-command --time 2>&1|grep -q \"no screensaver is running\" && xscreensaver &)";

pub trait CmdRunner {
    fn run(&mut self, cmd: &str) -> io::Result<()>;
}

pub struct ShellRunner;

impl CmdRunner for ShellRunner {
    fn run(&mut self, cmd: &str) -> io::Result<()> {
        run_shell(cmd)
    }
}

#[derive(Default)]
pub struct DryRunner {
    pub executed: RefCell<Vec<String>>,
}

impl CmdRunner for DryRunner {
    fn run(&mut self, cmd: &str) -> io::Result<()> {
        self.executed.borrow_mut().push(cmd.to_string());
        Ok(())
    }
}

pub fn run_shell(cmd: &str) -> io::Result<()> {
    run_shell_with("/bin/sh", cmd)
}

pub fn run_shell_with(shell: &str, cmd: &str) -> io::Result<()> {
    let status = Command::new(shell).arg("-c").arg(cmd).status()?;
    if status.success() {
        Ok(())
    } else {
        let detail = match status.code() {
            Some(code) => format!("exit code {code}"),
            None => "terminated by signal".to_string(),
        };
        Err(io::Error::other(format!("command failed: {detail}")))
    }
}
