fn render_lifecycle_outcome(outcome: LifecycleCommandOutcome) -> Vec<String> {
    match outcome {
        LifecycleCommandOutcome::Lines(lines) => lines,
    }
}

fn render_list_command_outcome(outcome: ListCommandOutcome) -> Vec<String> {
    format_installed_list_lines_for_style(current_output_style(), &outcome.receipts)
}
