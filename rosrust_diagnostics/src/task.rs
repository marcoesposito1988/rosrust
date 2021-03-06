use crate::Status;

/// Trait that describes any diagnostic task.
pub trait Task {
    /// Name of the diagnostic task.
    ///
    /// This name will appear in the diagnostic status generated by the `Updater`.
    fn name(&self) -> &str {
        ""
    }

    /// Runs this diagnostic task, and outputs the result into the provided status.
    fn run(&self, status: &mut Status);
}
