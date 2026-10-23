//! What `saaio bug-report` prints: the report's path and the check before uploading it

use std::path::Path;

use serde::Serialize;

use super::{Document, toon};

/// report: <path>, help: check the file before uploading it, attach it from where it is.
pub(crate) fn written(path: &Path) -> Document {
    #[derive(Serialize)]
    struct Written {
        report: String,
        help: Vec<String>,
    }
    Document::Toon(toon(&Written {
        report: path.display().to_string(),
        help: vec![
            "Check the file before uploading it: it can hold flow names, requests and paths"
                .to_string(),
            "Attach it from there; it is never written into a repository".to_string(),
        ],
    }))
}
