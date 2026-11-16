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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dry_runner_records_commands() {
        let mut runner = DryRunner::default();
        runner.run("echo hello").unwrap();
        runner.run("echo world").unwrap();
        let executed = runner.executed.borrow();
        assert_eq!(executed.len(), 2);
        assert_eq!(executed[0], "echo hello");
        assert_eq!(executed[1], "echo world");
    }

    #[test]
    fn test_shell_runner_success() {
        let mut runner = ShellRunner;
        let result = runner.run("true");
        assert!(result.is_ok());
    }

    #[test]
    fn test_shell_runner_failure() {
        let mut runner = ShellRunner;
        let result = runner.run("false");
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(err.to_string().contains("exit code 1"));
    }

    #[test]
    fn test_run_shell_with_success() {
        let result = run_shell_with("/bin/sh", "exit 0");
        assert!(result.is_ok());
    }

    #[test]
    fn test_run_shell_with_failure_code() {
        let result = run_shell_with("/bin/sh", "exit 42");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("exit code 42"));
    }

    #[test]
    fn test_default_commands_defined() {
        assert!(DEFAULT_LOCK_CMD.contains("lock"));
        assert!(DEFAULT_UNLOCK_CMD.contains("deactivate") || DEFAULT_UNLOCK_CMD.contains("unlock"));
    }
}
