# Terminal Interface Polish Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Improve Crosspack's rich interactive terminal output while preserving plain output contracts.

**Architecture:** Keep command behavior and plain formatters stable. Add reusable rich-only rendering helpers in `crates/crosspack-cli/src/render.rs`, then route selected command formatters through explicit `OutputStyle`-aware functions. Plain output must continue to use the existing field-oriented strings.

**Tech Stack:** Rust 2021, existing `anstyle` coloring, existing `indicatif` progress support, current `crosspack-cli` include-file module layout.

---

## File Structure

- Modify `crates/crosspack-cli/src/render.rs`: add reusable rich formatting primitives and tests for deterministic output helpers.
- Modify `crates/crosspack-cli/src/metadata.rs`: keep `format_search_results` as the plain contract and add `format_search_results_for_style` for rich search output.
- Modify `crates/crosspack-cli/src/core_flows.rs`: keep `format_info_lines` as the plain contract and add `format_info_lines_for_style` for rich info output.
- Modify `crates/crosspack-cli/src/command_flows.rs`: add styled formatters for installed package list and related read-only output where plain output can stay unchanged.
- Modify `crates/crosspack-cli/src/dispatch.rs`: pass `current_output_style()` into the new style-aware formatters.
- Modify `crates/crosspack-cli/src/tests.rs`: add unit tests for rich helpers and regression tests for plain output paths touched by this work.

Do not create new crates or add runtime dependencies in this pass.

---

### Task 1: Add Rich Renderer Primitives

**Files:**
- Modify: `crates/crosspack-cli/src/render.rs`
- Test: `crates/crosspack-cli/src/tests.rs`

- [ ] **Step 1: Add failing tests for renderer helper output**

Add tests near the existing render tests around `render_status_line_*` in `crates/crosspack-cli/src/tests.rs`:

```rust
#[test]
fn render_compact_table_plain_uses_tabs() {
    let rows = vec![
        vec!["name".to_string(), "version".to_string()],
        vec!["ripgrep".to_string(), "14.1.0".to_string()],
    ];

    assert_eq!(
        render_compact_table(OutputStyle::Plain, &rows),
        vec!["name\tversion", "ripgrep\t14.1.0"]
    );
}

#[test]
fn render_compact_table_rich_aligns_columns() {
    let rows = vec![
        vec!["name".to_string(), "version".to_string()],
        vec!["ripgrep".to_string(), "14.1.0".to_string()],
    ];

    assert_eq!(
        render_compact_table(OutputStyle::Rich, &rows),
        vec!["name     version", "ripgrep  14.1.0"]
    );
}

#[test]
fn render_key_value_detail_rich_aligns_key() {
    assert_eq!(
        render_key_value_detail(OutputStyle::Rich, "snapshot", "abc123"),
        "     snapshot  abc123"
    );
}

#[test]
fn render_empty_state_plain_returns_message_only() {
    assert_eq!(
        render_empty_state(
            OutputStyle::Plain,
            "No installed packages",
            Some("Run `crosspack install <name>` to add one."),
        ),
        vec!["No installed packages"]
    );
}

#[test]
fn render_empty_state_rich_includes_hint() {
    assert_eq!(
        render_empty_state(
            OutputStyle::Rich,
            "No installed packages",
            Some("Run `crosspack install <name>` to add one."),
        ),
        vec![
            "[WARN] No installed packages".to_string(),
            "[..] Run `crosspack install <name>` to add one.".to_string(),
        ]
    );
}
```

- [ ] **Step 2: Run focused tests and verify they fail**

Run:

```bash
cargo test -p crosspack-cli render_compact_table -- --test-threads=1
```

Expected: compile failure because `render_compact_table`, `render_key_value_detail`, and `render_empty_state` do not exist yet.

- [ ] **Step 3: Implement renderer helpers**

Add these helpers to `crates/crosspack-cli/src/render.rs` after `render_section_header`:

```rust
fn render_compact_table(style: OutputStyle, rows: &[Vec<String>]) -> Vec<String> {
    if rows.is_empty() {
        return Vec::new();
    }

    if style == OutputStyle::Plain {
        return rows.iter().map(|row| row.join("\t")).collect();
    }

    let column_count = rows.iter().map(Vec::len).max().unwrap_or(0);
    let mut widths = vec![0_usize; column_count];
    for row in rows {
        for (index, cell) in row.iter().enumerate() {
            widths[index] = widths[index].max(cell.len());
        }
    }

    rows.iter()
        .map(|row| {
            let mut line = String::new();
            for index in 0..column_count {
                if index > 0 {
                    line.push_str("  ");
                }
                let cell = row.get(index).map(String::as_str).unwrap_or("");
                if index + 1 == column_count {
                    line.push_str(cell);
                } else {
                    line.push_str(&format!("{cell:<width$}", width = widths[index]));
                }
            }
            line
        })
        .collect()
}

fn render_key_value_detail(style: OutputStyle, key: &str, value: &str) -> String {
    match style {
        OutputStyle::Plain => format!("{key}: {value}"),
        OutputStyle::Rich => format!("     {key:<9} {value}"),
    }
}

fn render_empty_state(style: OutputStyle, message: &str, hint: Option<&str>) -> Vec<String> {
    match style {
        OutputStyle::Plain => vec![message.to_string()],
        OutputStyle::Rich => {
            let mut lines = vec![render_status_line(style, "warn", message)];
            if let Some(hint) = hint {
                lines.push(render_status_line(style, "step", hint));
            }
            lines
        }
    }
}
```

- [ ] **Step 4: Run focused tests and verify they pass**

Run:

```bash
cargo test -p crosspack-cli render_ -- --test-threads=1
```

Expected: all new helper tests pass.

- [ ] **Step 5: Commit renderer primitives**

```bash
git add crates/crosspack-cli/src/render.rs crates/crosspack-cli/src/tests.rs
git commit -m "feat(cli): add rich terminal renderer primitives"
```

---

### Task 2: Polish Search Output in Rich Mode

**Files:**
- Modify: `crates/crosspack-cli/src/metadata.rs`
- Modify: `crates/crosspack-cli/src/dispatch.rs`
- Test: `crates/crosspack-cli/src/tests.rs`

- [ ] **Step 1: Add failing tests for style-aware search formatting**

Add tests near existing `format_search_results_*` tests:

```rust
#[test]
fn format_search_results_for_style_plain_preserves_contract() {
    let results = vec![SearchResult {
        name: "ripgrep".to_string(),
        description: Some("line search".to_string()),
        latest_version: "14.1.0".to_string(),
        source: "core".to_string(),
        match_kind: SearchMatchKind::Exact,
    }];

    assert_eq!(
        format_search_results_for_style(OutputStyle::Plain, &results, "rip"),
        vec![
            "name\tdescription\tlatest\tsource".to_string(),
            "ripgrep\tline search\t14.1.0\tcore".to_string(),
        ]
    );
}

#[test]
fn format_search_results_for_style_rich_adds_summary_and_aligned_rows() {
    let results = vec![SearchResult {
        name: "ripgrep".to_string(),
        description: Some("line search".to_string()),
        latest_version: "14.1.0".to_string(),
        source: "core".to_string(),
        match_kind: SearchMatchKind::Exact,
    }];

    assert_eq!(
        format_search_results_for_style(OutputStyle::Rich, &results, "rip"),
        vec![
            "[OK] 1 package matched 'rip'".to_string(),
            "name     description  latest  source".to_string(),
            "ripgrep  line search  14.1.0  core".to_string(),
        ]
    );
}
```

- [ ] **Step 2: Run focused search tests and verify they fail**

Run:

```bash
cargo test -p crosspack-cli format_search_results_for_style -- --test-threads=1
```

Expected: compile failure because `format_search_results_for_style` does not exist yet.

- [ ] **Step 3: Implement style-aware search formatter**

Add below `format_search_results` in `crates/crosspack-cli/src/metadata.rs`:

```rust
fn format_search_results_for_style(
    style: OutputStyle,
    results: &[SearchResult],
    query: &str,
) -> Vec<String> {
    if style == OutputStyle::Plain {
        return format_search_results(results, query);
    }

    if results.is_empty() {
        return render_empty_state(
            style,
            &format!("No packages found matching '{query}'."),
            Some("Try a broader keyword or run `crosspack update` to refresh local snapshots."),
        );
    }

    let mut rows = vec![vec![
        "name".to_string(),
        "description".to_string(),
        "latest".to_string(),
        "source".to_string(),
    ]];
    for result in results {
        rows.push(vec![
            result.name.clone(),
            result.description.clone().unwrap_or_else(|| "-".to_string()),
            result.latest_version.clone(),
            result.source.clone(),
        ]);
    }

    let mut lines = vec![render_status_line(
        style,
        "ok",
        &format!("{} package{} matched '{query}'", results.len(), if results.len() == 1 { "" } else { "s" }),
    )];
    lines.extend(render_compact_table(style, &rows));
    lines
}
```

- [ ] **Step 4: Use the new formatter in dispatch**

In `crates/crosspack-cli/src/dispatch.rs`, replace the `Commands::Search` output block with:

```rust
let output_style = current_output_style();
let lines = format_search_results_for_style(output_style, &results, &query);
for line in lines {
    println!("{line}");
}
```

This intentionally removes the old special-case warning wrapper because `format_search_results_for_style(OutputStyle::Plain, ...)` preserves the old plain message and rich mode now owns the empty-state decoration.

- [ ] **Step 5: Run search tests**

Run:

```bash
cargo test -p crosspack-cli format_search_results -- --test-threads=1
```

Expected: existing plain search tests and new rich search tests pass.

- [ ] **Step 6: Commit search polish**

```bash
git add crates/crosspack-cli/src/metadata.rs crates/crosspack-cli/src/dispatch.rs crates/crosspack-cli/src/tests.rs
git commit -m "feat(cli): polish rich search output"
```

---

### Task 3: Polish Info Output in Rich Mode

**Files:**
- Modify: `crates/crosspack-cli/src/core_flows.rs`
- Modify: `crates/crosspack-cli/src/dispatch.rs`
- Test: `crates/crosspack-cli/src/tests.rs`

- [ ] **Step 1: Add failing tests for style-aware info formatting**

Add tests near `format_info_lines_*` tests:

```rust
#[test]
fn format_info_lines_for_style_plain_preserves_contract() {
    let manifest = PackageManifest {
        name: "ripgrep".to_string(),
        version: Version::parse("14.1.0").unwrap(),
        description: Some("line search".to_string()),
        license: Some("MIT".to_string()),
        homepage: Some("https://github.com/BurntSushi/ripgrep".to_string()),
        provides: Vec::new(),
        conflicts: BTreeMap::new(),
        replaces: BTreeMap::new(),
        dependencies: BTreeMap::new(),
        artifacts: Vec::new(),
        source_build: None,
        services: Vec::new(),
    };

    assert_eq!(
        format_info_lines_for_style(OutputStyle::Plain, "ripgrep", &[manifest.clone()]),
        format_info_lines("ripgrep", &[manifest])
    );
}

#[test]
fn format_info_lines_for_style_rich_adds_sectioned_details() {
    let manifest = PackageManifest {
        name: "ripgrep".to_string(),
        version: Version::parse("14.1.0").unwrap(),
        description: Some("line search".to_string()),
        license: Some("MIT".to_string()),
        homepage: Some("https://github.com/BurntSushi/ripgrep".to_string()),
        provides: Vec::new(),
        conflicts: BTreeMap::new(),
        replaces: BTreeMap::new(),
        dependencies: BTreeMap::new(),
        artifacts: Vec::new(),
        source_build: None,
        services: Vec::new(),
    };

    let lines = format_info_lines_for_style(OutputStyle::Rich, "ripgrep", &[manifest]);

    assert!(lines.contains(&"[OK] ripgrep".to_string()));
    assert!(lines.contains(&"     version   14.1.0".to_string()));
    assert!(lines.contains(&"     summary   line search".to_string()));
    assert!(lines.contains(&"     license   MIT".to_string()));
}
```

- [ ] **Step 2: Run focused info tests and verify they fail**

Run:

```bash
cargo test -p crosspack-cli format_info_lines_for_style -- --test-threads=1
```

Expected: compile failure because `format_info_lines_for_style` does not exist yet.

- [ ] **Step 3: Implement style-aware info formatter**

Add below `format_info_lines` in `crates/crosspack-cli/src/core_flows.rs`:

```rust
fn format_info_lines_for_style(
    style: OutputStyle,
    name: &str,
    versions: &[PackageManifest],
) -> Vec<String> {
    if style == OutputStyle::Plain {
        return format_info_lines(name, versions);
    }

    let Some(latest) = versions.first() else {
        return render_empty_state(style, &format!("No package found: {name}"), None);
    };

    let mut lines = vec![render_status_line(style, "ok", name)];
    lines.push(render_key_value_detail(style, "version", &latest.version.to_string()));
    if let Some(description) = best_available_short_description(latest) {
        lines.push(render_key_value_detail(style, "summary", &description));
    }
    if let Some(homepage) = &latest.homepage {
        lines.push(render_key_value_detail(style, "homepage", homepage));
    }
    if let Some(license) = &latest.license {
        lines.push(render_key_value_detail(style, "license", license));
    }
    if versions.len() > 1 {
        let available = versions
            .iter()
            .map(|manifest| manifest.version.to_string())
            .collect::<Vec<_>>()
            .join(", ");
        lines.push(render_key_value_detail(style, "versions", &available));
    }
    lines
}
```

- [ ] **Step 4: Use the new formatter in dispatch**

In `Commands::Info` in `crates/crosspack-cli/src/dispatch.rs`, replace the non-empty branch:

```rust
for line in format_info_lines(&name, &versions) {
    println!("{line}");
}
```

with:

```rust
for line in format_info_lines_for_style(current_output_style(), &name, &versions) {
    println!("{line}");
}
```

Keep the existing empty package warning branch unchanged.

- [ ] **Step 5: Run info tests**

Run:

```bash
cargo test -p crosspack-cli format_info_lines -- --test-threads=1
```

Expected: existing plain info tests and new rich info tests pass.

- [ ] **Step 6: Commit info polish**

```bash
git add crates/crosspack-cli/src/core_flows.rs crates/crosspack-cli/src/dispatch.rs crates/crosspack-cli/src/tests.rs
git commit -m "feat(cli): polish rich info output"
```

---

### Task 4: Polish Installed List, Outdated, Registry, and Doctor Rich Output

**Files:**
- Modify: `crates/crosspack-cli/src/command_flows.rs`
- Modify: `crates/crosspack-cli/src/dispatch.rs`
- Test: `crates/crosspack-cli/src/tests.rs`

- [ ] **Step 1: Add tests for rich registry list formatting**

Near existing `format_registry_list_lines` tests, add:

```rust
#[test]
fn format_registry_list_status_lines_rich_adds_badges_without_changing_plain_lines() {
    let sources = vec![RegistrySourceWithSnapshotState {
        source: RegistrySourceRecord {
            name: "core".to_string(),
            kind: RegistrySourceKind::Git,
            location: "https://github.com/spiritledsoftware/crosspack-registry.git".to_string(),
            fingerprint_sha256: "abc123".to_string(),
            enabled: true,
            priority: 100,
            community: None,
        },
        snapshot: RegistrySourceSnapshotState::Ready {
            snapshot_id: "snap-1".to_string(),
        },
    }];

    let plain = format_registry_list_status_lines(OutputStyle::Plain, sources.clone());
    assert_eq!(plain, format_registry_list_lines(sources.clone()));

    let rich = format_registry_list_status_lines(OutputStyle::Rich, sources);
    assert!(rich.iter().any(|line| line.starts_with("[OK]")));
    assert!(rich.iter().any(|line| line.contains("snapshot")));
}
```

- [ ] **Step 2: Add tests for rich list empty state**

Add a small formatter test after list-related tests if no formatter exists yet:

```rust
#[test]
fn format_installed_list_lines_for_style_rich_empty_includes_hint() {
    assert_eq!(
        format_installed_list_lines_for_style(OutputStyle::Rich, &[]),
        vec![
            "[WARN] No installed packages".to_string(),
            "[..] Run `crosspack install <name>` to install a package.".to_string(),
        ]
    );
}
```

- [ ] **Step 3: Run focused tests and verify they fail**

Run:

```bash
cargo test -p crosspack-cli format_registry_list_status_lines -- --test-threads=1
```

Then run:

```bash
cargo test -p crosspack-cli format_installed_list_lines_for_style -- --test-threads=1
```

Expected: at least the installed-list formatter test fails because `format_installed_list_lines_for_style` does not exist yet. Registry rich assertions may also fail depending on current badge output.

- [ ] **Step 4: Add installed list formatter**

Add to `crates/crosspack-cli/src/command_flows.rs` near other read-only command helpers:

```rust
fn format_installed_list_lines_for_style(
    style: OutputStyle,
    receipts: &[InstallReceipt],
) -> Vec<String> {
    if receipts.is_empty() {
        return render_empty_state(
            style,
            "No installed packages",
            Some("Run `crosspack install <name>` to install a package."),
        );
    }

    if style == OutputStyle::Plain {
        return receipts
            .iter()
            .map(|receipt| format!("{} {}", receipt.name, receipt.version))
            .collect();
    }

    let mut rows = vec![vec!["name".to_string(), "version".to_string()]];
    for receipt in receipts {
        rows.push(vec![receipt.name.clone(), receipt.version.clone()]);
    }
    render_compact_table(style, &rows)
}
```

- [ ] **Step 5: Use installed list formatter in dispatch**

In `Commands::List` in `crates/crosspack-cli/src/dispatch.rs`, replace the current `if receipts.is_empty()` block with:

```rust
let output_style = current_output_style();
for line in format_installed_list_lines_for_style(output_style, &receipts) {
    println!("{line}");
}
```

- [ ] **Step 6: Upgrade registry rich list formatting while preserving plain**

Replace `format_registry_list_status_lines` in `crates/crosspack-cli/src/dispatch.rs` with:

```rust
fn format_registry_list_status_lines(
    style: OutputStyle,
    sources: Vec<RegistrySourceWithSnapshotState>,
) -> Vec<String> {
    if style == OutputStyle::Plain {
        return format_registry_list_lines(sources);
    }

    let mut lines = Vec::new();
    for line in format_registry_list_lines(sources) {
        let status = if line.contains("snapshot=ready:") {
            "ok"
        } else if line.contains("snapshot=none") || line.contains("snapshot=error:") {
            "warn"
        } else {
            "step"
        };
        lines.push(render_status_line(style, status, &line));
    }
    lines
}
```

Do not change `format_registry_list_lines`; it is the plain formatter and currently emits `snapshot=ready:<id>`, `snapshot=none`, or `snapshot=error:<reason>`.

- [ ] **Step 7: Run focused read-output tests**

Run:

```bash
cargo test -p crosspack-cli format_installed_list_lines_for_style -- --test-threads=1
```

Then run:

```bash
cargo test -p crosspack-cli format_registry_list -- --test-threads=1
```

Expected: rich tests pass and existing plain registry tests pass.

- [ ] **Step 8: Commit read-command polish**

```bash
git add crates/crosspack-cli/src/command_flows.rs crates/crosspack-cli/src/dispatch.rs crates/crosspack-cli/src/tests.rs
git commit -m "feat(cli): polish rich read command output"
```

---

### Task 5: Full Verification and Contract Review

**Files:**
- Review: `crates/crosspack-cli/src/render.rs`
- Review: `crates/crosspack-cli/src/metadata.rs`
- Review: `crates/crosspack-cli/src/core_flows.rs`
- Review: `crates/crosspack-cli/src/command_flows.rs`
- Review: `crates/crosspack-cli/src/dispatch.rs`
- Review: `crates/crosspack-cli/src/tests.rs`

- [ ] **Step 1: Run formatting check**

```bash
cargo fmt --all --check
```

Expected: exits 0.

- [ ] **Step 2: Run clippy**

```bash
cargo clippy --workspace --all-targets --all-features -- -D warnings
```

Expected: exits 0.

- [ ] **Step 3: Run workspace tests**

```bash
cargo test --workspace
```

Expected: exits 0.

- [ ] **Step 4: Manually inspect protected strings**

Run:

```bash
git diff main...HEAD -- crates/crosspack-cli/src | grep -E 'transaction_preview|transaction_summary|risk_flags|change_add|change_remove|change_replace|change_transition|update summary:'
```

Expected: no changes to protected string literals or their plain formatting logic unless the diff only adds tests asserting they stay unchanged.

- [ ] **Step 5: Commit verification fixes if needed**

If formatting or lint commands changed files, commit those fixes:

```bash
git add crates/crosspack-cli/src
git commit -m "test(cli): verify terminal output contracts"
```

If no files changed, do not create an empty commit.

---

## Self-Review Notes

- Spec coverage: renderer helpers, rich output for search/info/list/registry, and plain-contract protection are covered by Tasks 1-5.
- Scope control: this plan does not add prompts, a dashboard, new dependencies, or resolver/install behavior changes.
- Plain contract protection: each command formatter keeps an explicit `OutputStyle::Plain` path that delegates to the existing plain formatter or reproduces the existing line shape.
- Testing: every implementation task starts with a failing test and ends with focused verification, followed by full workspace verification.
