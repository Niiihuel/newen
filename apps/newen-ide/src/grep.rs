//! Finding text in every file of the folder: quill-files' `grep`, behind the dialog's ids.

pub use newen_files::find::{grep, Hit, LIMIT};

pub const GREP_OPEN: &str = "grep:open";
pub const GREP_FIELD: &str = "grep:field";
pub const GREP_CLOSE: &str = "grep:close";
