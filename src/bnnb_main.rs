fn main() {
    let args: Vec<String> = std::env::args().collect();
    let prog = args.first().cloned().unwrap_or("bnnb".to_string());
    let stdout = std::io::stdout();
    let stderr = std::io::stderr();
    let code = bnnb::run_cli(
        &prog,
        &args[1..],
        &bnnb::LinuxSys,
        &mut stdout.lock(),
        &mut stderr.lock(),
    );
    std::process::exit(code);
}
