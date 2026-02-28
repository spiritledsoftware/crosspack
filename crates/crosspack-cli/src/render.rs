use anstyle::{AnsiColor, Effects, Style};
use indicatif::{HumanCount, ProgressBar, ProgressStyle};

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
enum UiMode {
    Plain,
    Interactive,
}

#[derive(Copy, Clone, Debug)]
struct TerminalRenderer {
    style: OutputStyle,
    mode: UiMode,
}

struct TerminalProgress {
    style: OutputStyle,
    label: String,
    total: u64,
    current: u64,
    progress_bar: Option<ProgressBar>,
    started_at: Instant,
}

impl TerminalRenderer {
    fn from_style(style: OutputStyle) -> Self {
        Self {
            style,
            mode: ui_mode_from_style(style),
        }
    }

    fn current() -> Self {
        Self::from_style(current_output_style())
    }

    fn style(self) -> OutputStyle {
        self.style
    }

    fn print_status(self, status: &str, message: &str) {
        println!("{}", render_status_line(self.style, status, message));
    }

    fn print_section(self, title: &str) {
        if let Some(line) = render_section_header(self.mode, title) {
            println!();
            let rendered = match self.style {
                OutputStyle::Plain => line,
                OutputStyle::Rich => colorize(section_style(), &line),
            };
            println!("{rendered}");
        }
    }

    fn start_progress(self, label: &str, total: u64) -> TerminalProgress {
        let progress_bar = if self.style == OutputStyle::Rich {
            let progress_bar = ProgressBar::new(total.max(1));
            if let Ok(style) = ProgressStyle::with_template(
                "{spinner:.cyan.bold} {msg:<12} [{bar:20.cyan/blue}] {pos:>3}/{len:3} {elapsed_precise}",
            ) {
                progress_bar.set_style(
                    style
                        .tick_chars(progress_tick_chars(label))
                        .progress_chars("=>-"),
                );
            }
            progress_bar.set_message(label.to_string());
            progress_bar.enable_steady_tick(Duration::from_millis(80));
            Some(progress_bar)
        } else {
            None
        };

        TerminalProgress {
            style: self.style,
            label: label.to_string(),
            total,
            current: 0,
            progress_bar,
            started_at: Instant::now(),
        }
    }

    fn print_lines(self, lines: &[String]) {
        for line in lines {
            println!("{line}");
        }
    }
}

impl TerminalProgress {
    fn set(&mut self, current: u64) {
        self.current = current.min(self.total);

        let Some(progress_bar) = &self.progress_bar else {
            return;
        };

        let safe_total = self.total.max(1);
        progress_bar.set_length(safe_total);
        progress_bar.set_position(self.current.min(safe_total));
    }

    fn finish_success(mut self) {
        let Some(progress_bar) = self.progress_bar.take() else {
            return;
        };

        progress_bar.finish_and_clear();
        if let Some(line) = render_progress_line(
            self.style,
            &self.label,
            self.current,
            self.total,
            Some(self.started_at.elapsed()),
        ) {
            println!("{line}");
        }
    }

    fn finish_abandon(mut self) {
        if let Some(progress_bar) = self.progress_bar.take() {
            progress_bar.finish_and_clear();
        }
    }
}

fn format_elapsed(elapsed: Duration) -> String {
    let secs = elapsed.as_secs();
    let millis = elapsed.subsec_millis();
    format!("{secs}.{millis:03}s")
}

fn progress_tick_chars(label: &str) -> &'static str {
    match label {
        "install" => ".oO@* ",
        "upgrade" => "-=~* ",
        "update" => "<^>v ",
        "uninstall" => "\\|/- ",
        "self-update" => ".:;* ",
        _ => "|/-\\ ",
    }
}

fn section_style() -> Style {
    Style::new()
        .fg_color(Some(AnsiColor::BrightBlue.into()))
        .effects(Effects::BOLD)
}

fn progress_label_style() -> Style {
    Style::new()
        .fg_color(Some(AnsiColor::BrightCyan.into()))
        .effects(Effects::BOLD)
}

fn progress_bar_style() -> Style {
    Style::new().fg_color(Some(AnsiColor::BrightBlue.into()))
}

fn colorize(style: Style, text: &str) -> String {
    format!("{}{}{}", style.render(), text, style.render_reset())
}

fn ui_mode_from_style(style: OutputStyle) -> UiMode {
    match style {
        OutputStyle::Plain => UiMode::Plain,
        OutputStyle::Rich => UiMode::Interactive,
    }
}

fn render_section_header(mode: UiMode, title: &str) -> Option<String> {
    match mode {
        UiMode::Plain => None,
        UiMode::Interactive => Some(format!("== {title} ==")),
    }
}

fn render_progress_line(
    style: OutputStyle,
    label: &str,
    current: u64,
    total: u64,
    elapsed: Option<Duration>,
) -> Option<String> {
    if style == OutputStyle::Plain {
        return None;
    }

    let width = 18_usize;
    let safe_total = total.max(1);
    let bounded_current = current.min(safe_total);
    let filled = ((bounded_current as usize) * width) / (safe_total as usize);
    let bar = format!(
        "{}{}",
        "=".repeat(filled),
        "-".repeat(width.saturating_sub(filled))
    );
    let percent = (bounded_current * 100) / safe_total;
    let counts = format!("{}/{}", HumanCount(current), HumanCount(total));
    let suffix = elapsed
        .map(|value| format!(" complete in {}", format_elapsed(value)))
        .unwrap_or_default();

    Some(format!(
        "{} [{}] {:>3}% {}{}",
        colorize(progress_label_style(), label),
        colorize(progress_bar_style(), &bar),
        percent,
        counts,
        suffix
    ))
}
