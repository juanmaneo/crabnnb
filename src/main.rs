use crabnnb::run;
use crabnnb::sys::LinuxSys;

fn main() {
    let sys = LinuxSys;
    let args: Vec<String> = std::env::args().collect();
    let code = run::run_cli(
        "crabnnb",
        &args[1..],
        &sys,
        &mut std::io::stdout(),
        &mut std::io::stderr(),
    );
    std::process::exit(code);
}
