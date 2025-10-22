// Library root
// -----------
// This crate exposes a small library surface for the CLI. The binary
// (`main.rs`) uses these modules to implement the interactive CLI.
//
// Module responsibilities:
// - `api`: Encapsulates HTTP interactions with the backend (register,
//   auth, upload) and token persistence helpers.
// - `ui`: Implements the terminal-based user interface flows and
//   delegates requests to `api`.
//
// Keeping this separation makes it easier to test the API logic or
// replace the UI in the future (for example, adding a TUI or GUI).
pub mod api;
pub mod ui;
