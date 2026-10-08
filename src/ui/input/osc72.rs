//! kitty OSC 72 receiver (T-31).

/// A kitty drag-and-drop event (T-31). It has no variants until T-31 gives it
/// the protocol's moves, drops and errors, so [`super::Input::Dnd`] cannot be
/// built before then.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DndEvent {}
