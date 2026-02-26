fn write_completions_script<W: Write>(
    shell: CliCompletionShell,
    layout: &PrefixLayout,
    writer: &mut W,
) -> Result<()> {
    let mut command = Cli::command();
    let generator: Shell = shell.into();
    let mut generated = Vec::new();
    clap_complete::generate(generator, &mut command, "crosspack", &mut generated);

    if shell == CliCompletionShell::Zsh {
        writer
            .write_all(package_completion_loader_snippet(layout, shell).as_bytes())
            .with_context(|| "failed writing package completion loader block")?;
        writer
            .write_all(b"\n")
            .with_context(|| "failed writing completion script delimiter")?;
    }

    writer
        .write_all(&generated)
        .with_context(|| "failed writing generated completion script")?;

    if shell != CliCompletionShell::Zsh {
        writer
            .write_all(b"\n")
            .with_context(|| "failed writing completion script delimiter")?;
        writer
            .write_all(package_completion_loader_snippet(layout, shell).as_bytes())
            .with_context(|| "failed writing package completion loader block")?;
    }

    Ok(())
}

fn crosspack_completion_script_path(layout: &PrefixLayout, shell: CliCompletionShell) -> PathBuf {
    layout.completions_dir().join(shell.completion_filename())
}

fn package_completion_loader_snippet(layout: &PrefixLayout, shell: CliCompletionShell) -> String {
    let package_completion_dir =
        layout.package_completions_shell_dir(shell.package_completion_shell());
    match shell {
        CliCompletionShell::Bash => {
            let escaped_dir =
                escape_single_quote_shell(&package_completion_dir.display().to_string());
            format!(
                "# crosspack package completions\nif [ -d '{escaped_dir}' ]; then\n  find '{escaped_dir}' -mindepth 1 -maxdepth 1 -type f -print 2>/dev/null \\\n    | LC_ALL=C sort \\\n    | while IFS= read -r _crosspack_pkg_completion_path; do\n        . \"${{_crosspack_pkg_completion_path}}\"\n      done\nfi\n"
            )
        }
        CliCompletionShell::Zsh => {
            let escaped_dir =
                escape_single_quote_shell(&package_completion_dir.display().to_string());
            format!(
                "# crosspack package completions\nif [ -d '{escaped_dir}' ]; then\n  if (( ${{fpath[(Ie)'{escaped_dir}']}} == 0 )); then\n    fpath=('{escaped_dir}' $fpath)\n  fi\n  autoload -Uz compinit\n  compinit -i >/dev/null 2>&1 || true\nfi\n"
            )
        }
        CliCompletionShell::Fish => {
            let escaped_dir =
                escape_single_quote_shell(&package_completion_dir.display().to_string());
            format!(
                "# crosspack package completions\nif test -d '{escaped_dir}'\n    for _crosspack_pkg_completion_path in (find '{escaped_dir}' -mindepth 1 -maxdepth 1 -type f -print 2>/dev/null | sort)\n        source \"$_crosspack_pkg_completion_path\"\n    end\nend\nset -e _crosspack_pkg_completion_path\n"
            )
        }
        CliCompletionShell::Powershell => {
            let escaped_dir = escape_ps_single_quote(&package_completion_dir.display().to_string());
            format!(
                "# crosspack package completions\n$crosspackPackageCompletionDir = '{escaped_dir}'\nif (Test-Path $crosspackPackageCompletionDir) {{\n  Get-ChildItem -Path $crosspackPackageCompletionDir -File | Sort-Object Name | ForEach-Object {{\n    . $_.FullName\n  }}\n}}\nRemove-Variable crosspackPackageCompletionDir -ErrorAction SilentlyContinue\n"
            )
        }
    }
}

fn refresh_crosspack_completion_assets(layout: &PrefixLayout) -> Result<()> {
    fs::create_dir_all(layout.completions_dir()).with_context(|| {
        format!(
            "failed to create completion directory: {}",
            layout.completions_dir().display()
        )
    })?;

    for shell in [
        CliCompletionShell::Bash,
        CliCompletionShell::Zsh,
        CliCompletionShell::Fish,
        CliCompletionShell::Powershell,
    ] {
        let mut output = Vec::new();
        write_completions_script(shell, layout, &mut output)?;
        let path = crosspack_completion_script_path(layout, shell);
        fs::write(&path, &output)
            .with_context(|| format!("failed writing completion asset: {}", path.display()))?;
    }

    Ok(())
}

fn sync_completion_assets_best_effort(layout: &PrefixLayout, operation: &str) -> Result<()> {
    if let Err(err) = refresh_crosspack_completion_assets(layout) {
        return Err(anyhow!(
            "warning: completion sync skipped (operation={operation} reason={})",
            err
        ));
    }
    Ok(())
}

fn escape_single_quote_shell(value: &str) -> String {
    value.replace('\'', "'\"'\"'")
}

fn detect_shell_from_env(shell_env: Option<&str>) -> Option<CliCompletionShell> {
    let shell_value = shell_env?;
    let shell_token = Path::new(shell_value)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or(shell_value)
        .to_ascii_lowercase();
    match shell_token.as_str() {
        "bash" => Some(CliCompletionShell::Bash),
        "zsh" => Some(CliCompletionShell::Zsh),
        "fish" => Some(CliCompletionShell::Fish),
        "powershell" | "pwsh" => Some(CliCompletionShell::Powershell),
        _ => None,
    }
}

fn resolve_init_shell(
    requested_shell: Option<CliCompletionShell>,
    shell_env: Option<&str>,
    is_windows: bool,
) -> CliCompletionShell {
    if let Some(shell) = requested_shell {
        return shell;
    }
    if let Some(shell) = detect_shell_from_env(shell_env) {
        return shell;
    }
    if is_windows {
        CliCompletionShell::Powershell
    } else {
        CliCompletionShell::Bash
    }
}

fn print_init_shell_snippet(layout: &PrefixLayout, shell: CliCompletionShell) {
    let bin = layout.bin_dir();
    let completion_path = crosspack_completion_script_path(layout, shell);
    match shell {
        CliCompletionShell::Bash | CliCompletionShell::Zsh => {
            let escaped_completion =
                escape_single_quote_shell(&completion_path.display().to_string());
            println!("export PATH=\"{}:$PATH\"", bin.display());
            println!("if [ -f '{escaped_completion}' ]; then");
            println!("  . '{escaped_completion}'");
            println!("fi");
        }
        CliCompletionShell::Fish => {
            let escaped_bin = escape_single_quote_shell(&bin.display().to_string());
            let escaped_completion =
                escape_single_quote_shell(&completion_path.display().to_string());
            println!("if test -d '{escaped_bin}'");
            println!("    if not contains -- '{escaped_bin}' $PATH");
            println!("        set -gx PATH '{escaped_bin}' $PATH");
            println!("    end");
            println!("end");
            println!("if test -f '{escaped_completion}'");
            println!("    source '{escaped_completion}'");
            println!("end");
        }
        CliCompletionShell::Powershell => {
            let escaped_bin = escape_ps_single_quote(&bin.display().to_string());
            let escaped_completion = escape_ps_single_quote(&completion_path.display().to_string());
            println!("if (Test-Path '{escaped_bin}') {{");
            println!(
                "  if (-not ($env:PATH -split ';' | Where-Object {{ $_ -eq '{escaped_bin}' }})) {{"
            );
            println!("    $env:PATH = '{escaped_bin};' + $env:PATH");
            println!("  }}");
            println!("}}");
            println!("if (Test-Path '{escaped_completion}') {{");
            println!("  . '{escaped_completion}'");
            println!("}}");
        }
    }
}

fn registry_state_root(layout: &PrefixLayout) -> PathBuf {
    layout.state_dir().join("registries")
}
