use assert_cmd::Command;

fn assert_no_terminal_control_sequences(output_name: &str, output: &str) {
    assert!(
        !output.contains('\r'),
        "{output_name} contained carriage return: {output:?}"
    );
    assert!(
        !output.contains('\x1b'),
        "{output_name} contained ANSI escape/control sequence: {output:?}"
    );
    assert!(
        !output.contains("\x1b[2K"),
        "{output_name} contained clear-line escape: {output:?}"
    );
}

#[test]
fn doctor_stdout_has_no_terminal_control_sequences_when_captured() {
    let output = Command::cargo_bin("crosspack")
        .expect("crosspack binary should build")
        .arg("doctor")
        .output()
        .expect("doctor should run");

    assert!(output.status.success(), "doctor should succeed");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert_no_terminal_control_sequences("stdout", &stdout);
}

#[cfg(unix)]
#[test]
fn doctor_pty_output_has_no_clear_line_or_legacy_progress_tokens() {
    use rexpect::process::WaitStatus;
    use std::env;
    use std::path::PathBuf;
    use std::process::Command as StdCommand;

    let binary = env::var_os("CARGO_BIN_EXE_crosspack")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("target/debug/crosspack"));
    let mut command = StdCommand::new(binary);
    command.arg("doctor");
    let mut process = rexpect::session::spawn_command(command, Some(5_000))
        .expect("doctor should spawn in a PTY");

    let output = process
        .exp_eof()
        .expect("doctor should finish without hanging");
    match process.process().wait() {
        Ok(WaitStatus::Exited(_, 0)) => {}
        status => panic!("doctor should exit successfully, got {status:?}; output: {output:?}"),
    }
    assert!(
        !output.contains("\x1b[2K"),
        "PTY output contained clear-line escape: {output:?}"
    );
    assert!(
        !output.contains("[progress]")
            && !output.contains("progress:")
            && !output.contains("::progress"),
        "PTY output contained legacy progress token: {output:?}"
    );
}
