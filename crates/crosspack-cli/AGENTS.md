# CROSSPACK-CLI KNOWLEDGE BASE

## OVERVIEW
`crosspack-cli` is the single command router that binds Clap surfaces to installer, registry, resolver, and security flows while preserving stable user-facing output contracts.

## ENTRYPOINTS
- `src/main.rs`: defines `Cli`, `Commands`, `RegistryCommands`, and all dispatch logic in `main()`.
- `main()` match arms are integration boundaries; keep orchestration here and domain behavior in library crates.
- Shared output helpers (`resolve_output_style`, `render_status_line`, `format_*`) are contract code, not cosmetic glue.
- Renderer boundaries (`TerminalRenderer`, section/status/progress helpers, `format_*`) are contract code, not cosmetic glue.
- Transaction orchestration entrypoints (`execute_with_transaction`, `ensure_no_active_transaction_for`, rollback/repair paths) must remain centralized for consistent preflight and recovery behavior.
- Completion entrypoints (`Completions`, `InitShell`, completion sync helpers) are part of runtime UX and post-install maintenance.

## COMMAND SURFACES
| command area | location | invariants |
|---|---|---|
| metadata read commands (`search`, `info`) | `src/main.rs` (`Commands::Search`, `Commands::Info`) | Use selected metadata backend; preserve no-result text semantics; never bypass backend selection guards. |
| install/upgrade planning + apply | `src/main.rs` (`Commands::Install`, `Commands::Upgrade`, `run_upgrade_command`) | Enforce active-transaction preflight; dry-run prints deterministic transaction preview lines; apply path journals ordered steps. |
| rollback/repair transaction recovery | `src/main.rs` (`Commands::Rollback`, `Commands::Repair`, rollback helpers) | Fail closed on unreadable/missing metadata; status transitions stay explicit (`rolling_back`, `failed`, `rolled_back`). |
| uninstall/list/pin lifecycle | `src/main.rs` (`Commands::Uninstall`, `Commands::List`, `Commands::Pin`) | Receipt and pin writes remain source of truth; list output stays simple machine-friendly `name version`. |
| registry source management | `src/main.rs` (`Commands::Registry`, `format_registry_*`) | Add/list/remove output lines stay stable; fingerprint/priority semantics are required, not optional hints. |
| snapshot update + self-update | `src/main.rs` (`Commands::Update`, `Commands::SelfUpdate`) | Update summary formatting remains deterministic; self-update follows transaction safety and completion sync best effort. |
| diagnostics + shell integration | `src/main.rs` (`Commands::Doctor`, `Commands::Completions`, `Commands::InitShell`) | Doctor always reports prefix/bin/cache/transaction health; generated completions include package loader snippet behavior per shell. |

## OUTPUT CONTRACTS
- Output style is terminal-sensitive: both stdout+stderr TTY => rich badges; otherwise plain deterministic lines.
- Rich output is additive decoration only; plain text semantics must remain unchanged for automation.
- Dry-run transaction preview lines are contract-critical keys: `transaction_preview`, `transaction_summary`, `risk_flags`, `change_add`, `change_remove`, `change_replace`, `change_transition`.
- Keep ordering stable in machine-oriented output (summary before change lines; deterministic sort where already applied).
- Keep update and registry formatters centralized; do not inline ad hoc `println!` variants that drift wording or field order.
- Keep interactive enhancements additive (section headers/progress framing); plain mode remains the compatibility surface.
- Error/guidance messages tied to trust/snapshot flows should remain explicit and actionable; do not replace with vague failures.

## ANTI-PATTERNS (CLI)
- Moving command-specific orchestration into scattered modules that bypasses `main()` dispatch readability.
- Changing deterministic output tokens/line shapes without coordinated contract and test updates.
- Introducing direct filesystem mutations in command arms when equivalent installer/registry APIs already exist.
- Skipping `ensure_no_active_transaction_for` or transaction metadata writes in mutating flows.
- Mixing rich badge markers into plain-mode code paths.
- Duplicating backend/source selection logic instead of using existing helper paths.

## QUICK CHECKS
```bash
rustup run stable cargo fmt --all --check
rustup run stable cargo test -p crosspack-cli
rustup run stable cargo clippy -p crosspack-cli --all-targets -- -D warnings
cargo run -p crosspack-cli -- install ripgrep --dry-run
cargo run -p crosspack-cli -- upgrade --dry-run
cargo run -p crosspack-cli -- registry list
cargo run -p crosspack-cli -- doctor
```
