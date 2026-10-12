use tokio::task::JoinHandle;

/// Aborts the tasks when dropped.
pub(super) struct Tasks(pub(super) Vec<JoinHandle<()>>);

impl Drop for Tasks {
    fn drop(&mut self) {
        for task in &self.0 {
            task.abort();
        }
    }
}
