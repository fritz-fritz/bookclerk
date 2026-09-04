//! Official Cloudflare D1 REST export/import JSON protocol (no HTTP).
//!
//! Envelope shape follows
//! <https://developers.cloudflare.com/api/resources/d1/subresources/database/methods/export/>:
//! the signed download URL is nested at `/result/result/signed_url`. Poll
//! bodies resend `/result/at_bookmark` as `current_bookmark`.

use serde_json::{json, Value as JsonValue};

/// Outcome of one D1 REST export poll response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum D1ExportPoll {
    /// Job still running; send `current_bookmark` on the next POST.
    InProgress {
        /// Time-travel bookmark from `/result/at_bookmark`.
        bookmark: Option<String>,
    },
    /// SQL dump is ready to download.
    Complete {
        /// HTTPS (or test-loopback HTTP) signed URL.
        signed_url: String,
        /// Optional generated filename.
        filename: Option<String>,
        /// Bookmark from the completed poll.
        bookmark: Option<String>,
    },
    /// Cloudflare reported `status=error` or `success=false`.
    Failed {
        /// Human-readable error body.
        message: String,
    },
}

/// JSON body for an export start or poll POST.
#[must_use]
pub fn d1_export_poll_body(current_bookmark: Option<&str>) -> JsonValue {
    let mut body = json!({ "output_format": "polling" });
    if let Some(bookmark) = current_bookmark {
        body["current_bookmark"] = JsonValue::String(bookmark.to_string());
    }
    body
}

/// Parses one official D1 export JSON envelope.
///
/// # Errors
///
/// Returns when the body is not an object or is missing required fields for
/// a completed export.
pub fn parse_d1_export_envelope(body: &JsonValue) -> Result<D1ExportPoll, String> {
    if !body.is_object() {
        return Err("d1 export response is not a JSON object".into());
    }
    if body.get("success").and_then(JsonValue::as_bool) == Some(false) {
        return Ok(D1ExportPoll::Failed {
            message: format!(
                "d1 export rejected: {}",
                body.get("errors").cloned().unwrap_or(JsonValue::Null)
            ),
        });
    }
    let bookmark = pointer_string(body, "/result/at_bookmark");
    let status = pointer_string(body, "/result/status").unwrap_or_default();
    if status.eq_ignore_ascii_case("error") || status.eq_ignore_ascii_case("failed") {
        let detail = pointer_string(body, "/result/error").unwrap_or_else(|| body.to_string());
        return Ok(D1ExportPoll::Failed {
            message: format!("d1 export failed: {detail}"),
        });
    }
    let signed = d1_export_signed_url(body);
    if let Some(signed_url) = signed {
        return Ok(D1ExportPoll::Complete {
            signed_url,
            filename: pointer_string(body, "/result/result/filename")
                .or_else(|| pointer_string(body, "/result/filename")),
            bookmark,
        });
    }
    if status.eq_ignore_ascii_case("complete") {
        return Err("d1 export status is complete but signed_url is missing".into());
    }
    Ok(D1ExportPoll::InProgress { bookmark })
}

/// Official nested path first (`/result/result/signed_url`), then flattened.
#[must_use]
pub fn d1_export_signed_url(body: &JsonValue) -> Option<String> {
    pointer_string(body, "/result/result/signed_url")
        .or_else(|| pointer_string(body, "/result/signed_url"))
}

/// Import `init` upload URL (`/result/upload_url`).
#[must_use]
pub fn d1_import_upload_url(body: &JsonValue) -> Option<String> {
    pointer_string(body, "/result/upload_url")
}

/// Reads a JSON pointer as a string.
fn pointer_string(body: &JsonValue, pointer: &str) -> Option<String> {
    body.pointer(pointer)
        .and_then(JsonValue::as_str)
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn initial_poll() -> JsonValue {
        json!({
            "success": true,
            "result": {
                "at_bookmark": "bm-1",
                "status": "running",
                "type": "export",
                "messages": ["starting"]
            }
        })
    }

    fn completed_nested() -> JsonValue {
        json!({
            "success": true,
            "errors": [],
            "messages": [],
            "result": {
                "at_bookmark": "bm-1",
                "error": null,
                "messages": ["done"],
                "result": {
                    "filename": "db.sql",
                    "signed_url": "https://export.example/dump.sql"
                },
                "status": "complete",
                "success": true,
                "type": "export"
            }
        })
    }

    #[test]
    fn poll_body_starts_without_bookmark_then_resends() {
        let start = d1_export_poll_body(None);
        assert_eq!(start["output_format"], "polling");
        assert!(start.get("current_bookmark").is_none());
        let poll = d1_export_poll_body(Some("bm-1"));
        assert_eq!(poll["current_bookmark"], "bm-1");
    }

    #[test]
    fn initial_poll_propagates_bookmark() {
        match parse_d1_export_envelope(&initial_poll()).unwrap() {
            D1ExportPoll::InProgress { bookmark } => {
                assert_eq!(bookmark.as_deref(), Some("bm-1"));
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn completed_nested_result_contains_signed_url() {
        match parse_d1_export_envelope(&completed_nested()).unwrap() {
            D1ExportPoll::Complete {
                signed_url,
                filename,
                bookmark,
            } => {
                assert_eq!(signed_url, "https://export.example/dump.sql");
                assert_eq!(filename.as_deref(), Some("db.sql"));
                assert_eq!(bookmark.as_deref(), Some("bm-1"));
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(
            d1_export_signed_url(&completed_nested()).as_deref(),
            Some("https://export.example/dump.sql")
        );
    }

    #[test]
    fn error_result_is_failed() {
        let body = json!({
            "success": true,
            "result": {
                "at_bookmark": "bm-1",
                "status": "error",
                "error": "export exploded"
            }
        });
        match parse_d1_export_envelope(&body).unwrap() {
            D1ExportPoll::Failed { message } => {
                assert!(message.contains("export exploded"), "{message}");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn top_level_success_false_is_failed() {
        let body = json!({
            "success": false,
            "errors": [{ "code": 1000, "message": "nope" }]
        });
        match parse_d1_export_envelope(&body).unwrap() {
            D1ExportPoll::Failed { message } => assert!(message.contains("nope"), "{message}"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn malformed_complete_without_signed_url_errors() {
        let body = json!({
            "success": true,
            "result": { "status": "complete", "at_bookmark": "bm" }
        });
        let err = parse_d1_export_envelope(&body).unwrap_err();
        assert!(err.contains("signed_url is missing"), "{err}");
    }

    #[test]
    fn malformed_non_object_errors() {
        let err = parse_d1_export_envelope(&json!([])).unwrap_err();
        assert!(err.contains("not a JSON object"), "{err}");
    }

    #[test]
    fn import_upload_url_is_nested_under_result() {
        let init = json!({
            "success": true,
            "result": {
                "upload_url": "https://upload.example/put",
                "filename": "dump.sql"
            }
        });
        assert_eq!(
            d1_import_upload_url(&init).as_deref(),
            Some("https://upload.example/put")
        );
    }
}
