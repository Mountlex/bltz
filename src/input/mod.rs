mod handler;
mod keybindings;

pub use handler::{InputResult, handle_input};
pub use keybindings::{Action, KeyBindings, KeybindingEntry};
