fn render_lifecycle_outcome(outcome: LifecycleCommandOutcome) -> Vec<String> {
    match outcome {
        LifecycleCommandOutcome::Lines(lines) => lines,
    }
}
