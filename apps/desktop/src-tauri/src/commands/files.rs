//! A project's file library. The webview reads the files the user picks or
//! drops and sends their text, or a PDF's bytes; no command here takes a
//! disk path.

use tauri::State;
use tauri::http::HeaderMap;
use tauri::ipc::{InvokeBody, Request};
use uuid::Uuid;

use crate::error::AppError;
use crate::pdf::{PDF_TIMEOUT, PdfLimits, PdfWorker, extract_text};
use crate::redacted::Redacted;
use crate::storage::StorageState;
use crate::storage::files::{FilePage, ProjectFile};

/// Stores one file, replacing a file of the same name in the project.
#[tauri::command]
pub async fn project_file_add(
    storage: State<'_, StorageState>,
    project_id: Uuid,
    name: Redacted,
    content: Redacted,
) -> Result<ProjectFile, AppError> {
    storage.get()?.add_file(project_id, &name, &content)
}

/// `%XX`-escaped `text` as UTF-8, as `encodeURIComponent` wrote it.
fn percent_decode(text: &str) -> Result<String, AppError> {
    let invalid = || AppError::InvalidInput("a file name is not validly encoded".into());
    let mut bytes = Vec::with_capacity(text.len());
    let mut rest = text.as_bytes();
    while let Some((&byte, tail)) = rest.split_first() {
        if byte == b'%' {
            let hex = tail.get(..2).ok_or_else(invalid)?;
            let hex = std::str::from_utf8(hex).map_err(|_| invalid())?;
            bytes.push(u8::from_str_radix(hex, 16).map_err(|_| invalid())?);
            rest = &tail[2..];
        } else {
            bytes.push(byte);
            rest = tail;
        }
    }
    String::from_utf8(bytes).map_err(|_| invalid())
}

fn header<'a>(headers: &'a HeaderMap, name: &str) -> Result<&'a str, AppError> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| AppError::InvalidInput(format!("the {name} header is missing")))
}

/// The project, name and bytes of a PDF upload: the raw body is the PDF,
/// `x-project-id` names the project and `x-file-name` the file,
/// percent-encoded since header values are not Unicode.
fn pdf_upload(
    body: &InvokeBody,
    headers: &HeaderMap,
) -> Result<(Uuid, Redacted, Vec<u8>), AppError> {
    let InvokeBody::Raw(bytes) = body else {
        return Err(AppError::InvalidInput("a PDF is sent as raw bytes".into()));
    };
    let project_id = Uuid::parse_str(header(headers, "x-project-id")?)
        .map_err(|_| AppError::InvalidInput("the project id is not a UUID".into()))?;
    let name = percent_decode(header(headers, "x-file-name")?)?;
    Ok((project_id, Redacted::new(name), bytes.clone()))
}

/// Extracts a PDF's text and stores it like a text file, under the PDF's
/// name. The PDF itself is not kept.
#[tauri::command]
pub async fn project_file_add_pdf(
    storage: State<'_, StorageState>,
    pdf: State<'_, PdfWorker>,
    request: Request<'_>,
) -> Result<ProjectFile, AppError> {
    let (project_id, name, bytes) = pdf_upload(request.body(), request.headers())?;
    let size = bytes.len();
    let extracted = pdf
        .run(bytes, PdfLimits::default(), PDF_TIMEOUT, extract_text)
        .await
        .inspect_err(|error| {
            tracing::info!(bytes = size, kind = error.kind(), "PDF skipped");
        })?;
    tracing::info!(
        bytes = size,
        pages = extracted.pages,
        text_bytes = extracted.text.len(),
        truncated = extracted.truncated,
        "PDF text extracted"
    );
    storage
        .get()?
        .add_file(project_id, &name, &Redacted::new(extracted.text))
}

/// One page of the project's files, by name.
#[tauri::command]
pub async fn project_file_list(
    storage: State<'_, StorageState>,
    project_id: Uuid,
    offset: u32,
    limit: u32,
) -> Result<FilePage, AppError> {
    storage.get()?.list_files(project_id, offset, limit)
}

#[tauri::command]
pub async fn project_file_delete(
    storage: State<'_, StorageState>,
    project_id: Uuid,
    id: Uuid,
) -> Result<(), AppError> {
    storage.get()?.delete_file(project_id, id)
}

#[cfg(test)]
mod tests {
    use tauri::http::HeaderValue;

    use super::*;

    fn headers(pairs: &[(&'static str, &str)]) -> HeaderMap {
        let mut headers = HeaderMap::new();
        for (name, value) in pairs {
            headers.insert(*name, HeaderValue::from_str(value).unwrap());
        }
        headers
    }

    #[test]
    fn percent_decode_reads_encode_uri_component() {
        // encodeURIComponent("raportit/Q3 tulos ä.pdf")
        assert_eq!(
            percent_decode("raportit%2FQ3%20tulos%20%C3%A4.pdf").unwrap(),
            "raportit/Q3 tulos ä.pdf"
        );
        assert_eq!(percent_decode("plain.pdf").unwrap(), "plain.pdf");
    }

    #[test]
    fn percent_decode_rejects_bad_escapes_and_bad_utf8() {
        for bad in ["%", "%4", "%zz.pdf", "%C3.pdf", "%FF"] {
            assert!(
                matches!(percent_decode(bad), Err(AppError::InvalidInput(_))),
                "{bad}"
            );
        }
    }

    #[test]
    fn pdf_upload_reads_body_and_headers() {
        let project = Uuid::new_v4();
        let (id, name, bytes) = pdf_upload(
            &InvokeBody::Raw(b"%PDF-1.5".to_vec()),
            &headers(&[
                ("x-project-id", &project.to_string()),
                ("x-file-name", "a%20b.pdf"),
            ]),
        )
        .unwrap();
        assert_eq!(id, project);
        assert_eq!(name.expose(), "a b.pdf");
        assert_eq!(bytes, b"%PDF-1.5");
    }

    #[test]
    fn pdf_upload_rejects_json_missing_headers_and_bad_ids() {
        let project = Uuid::new_v4().to_string();
        let good = headers(&[("x-project-id", &project), ("x-file-name", "a.pdf")]);
        let raw = InvokeBody::Raw(Vec::new());
        for (body, headers) in [
            (InvokeBody::Json(serde_json::json!([1, 2])), good.clone()),
            (raw.clone(), headers(&[("x-file-name", "a.pdf")])),
            (raw.clone(), headers(&[("x-project-id", &project)])),
            (
                raw.clone(),
                headers(&[("x-project-id", "nope"), ("x-file-name", "a.pdf")]),
            ),
        ] {
            assert!(matches!(
                pdf_upload(&body, &headers),
                Err(AppError::InvalidInput(_))
            ));
        }
    }
}
