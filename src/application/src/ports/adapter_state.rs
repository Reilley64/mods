use std::any::Any;

/// Adapter-owned state behind an opaque port handle.
///
/// Handles are `Send` but not `Sync`. A handle moves to one step at a time, so
/// adapter state behind it is never used concurrently.
pub struct AdapterState(Box<dyn Any + Send>);
impl AdapterState {
	pub fn new(state: impl Any + Send) -> Self {
		Self(Box::new(state))
	}

	/// Returns `None` when the state was created by a different adapter.
	pub fn downcast<T: Any>(self) -> Option<T> {
		self.0.downcast().ok().map(|state| *state)
	}

	/// Returns `None` when the state was created by a different adapter.
	pub fn downcast_ref<T: Any>(&self) -> Option<&T> {
		self.0.downcast_ref()
	}
}
