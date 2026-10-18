use std::path::Path;

use serde::Serialize;

use super::{Document, toon};

/// report: <path>, help: check the file before uploading it.
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
        ],
    }))
}
