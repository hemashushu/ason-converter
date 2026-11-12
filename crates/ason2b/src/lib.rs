//! Command-line entry points for the ASON conversion utilities.

mod cli;

/// Runs a command-line converter and returns its process exit status.
pub use cli::{run_ason2b, run_ason2j};
