//! Mapping from parsed CLI arguments to an initial process title.
//!
//! This logic depends on the clap `Args`/`Command` types defined in `cli`, so
//! it lives in the CLI layer. The low-level title-setting primitives it uses
//! (`compact_process_title`, `session_name`, `set_title`) live in the
//! `process_title` core module.

use crate::cli::args::Args;
use crate::process_title::set_title;

pub(crate) fn initial_title(_args: &Args) -> String {
    "anastasia".to_string()
}

pub(crate) fn set_initial_title(args: &Args) {
    set_title(initial_title(args));
}
