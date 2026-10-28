//! The text of PDFs added to a project library, extracted locally by the
//! pure-Rust `pdf-extract`. Only the text is stored, never the PDF.
//!
//! A hostile PDF can make the parser panic or work for a long time, and a
//! running thread cannot be killed, so the bound comes from limits: bytes,
//! pages and text are capped, one dedicated thread extracts at a time, and
//! after a timeout new PDFs are refused until that thread really ends. At
//! most one runaway extraction can ever exist. Memory and stack exhaustion
//! (unbounded inflate, cyclic page parents or forms) cannot be caught in
//! process; `docs/security.md` lists that as a known limit.

use std::fmt;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::{Arc, Once};
use std::time::Duration;

use pdf_extract::{ConvertToFmt, Document, PlainTextOutput, output_doc_page};
use tokio::sync::{Semaphore, oneshot};

use crate::error::AppError;
use crate::storage::files::MAX_FILE_BYTES;
use crate::storage::memory::truncate_at;

/// The largest PDF read, in bytes.
pub const MAX_PDF_BYTES: usize = 32 * 1024 * 1024;
/// A PDF with more pages is skipped.
pub const MAX_PDF_PAGES: usize = 2000;
/// How long one extraction may take before the command gives up on it.
pub const PDF_TIMEOUT: Duration = Duration::from_secs(30);

/// The caps on one extraction; tests shrink them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PdfLimits {
    pub max_bytes: usize,
    pub max_pages: usize,
    /// Extraction stops once the text reaches this many bytes.
    pub max_text_bytes: usize,
}

impl Default for PdfLimits {
    fn default() -> Self {
        Self {
            max_bytes: MAX_PDF_BYTES,
            max_pages: MAX_PDF_PAGES,
            max_text_bytes: MAX_FILE_BYTES,
        }
    }
}

/// A PDF's text and what logs may say about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Extracted {
    pub text: String,
    pub pages: usize,
    /// The text reached [`PdfLimits::max_text_bytes`] and was cut there.
    pub truncated: bool,
}

/// Collects text up to `max` bytes. The write that would pass it is cut at
/// a char boundary and fails, which stops the extraction.
struct Bounded {
    text: String,
    max: usize,
    full: bool,
}

impl fmt::Write for Bounded {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        let room = self.max - self.text.len();
        if s.len() > room {
            self.text.push_str(truncate_at(s, room));
            self.full = true;
            return Err(fmt::Error);
        }
        self.text.push_str(s);
        Ok(())
    }
}

impl<'a> ConvertToFmt for &'a mut Bounded {
    type Writer = &'a mut Bounded;

    fn convert(self) -> Self::Writer {
        self
    }
}

fn skipped(reason: &str) -> AppError {
    AppError::InvalidInput(reason.into())
}

/// The text of the PDF in `bytes`, or why it is skipped: too large, not a
/// PDF, encrypted, too many pages, or no text (scanned or image-only).
pub fn extract_text(bytes: &[u8], limits: &PdfLimits) -> Result<Extracted, AppError> {
    if bytes.len() > limits.max_bytes {
        return Err(AppError::TooLarge(format!(
            "a PDF is at most {} MiB",
            limits.max_bytes / (1024 * 1024)
        )));
    }
    let doc = Document::load_mem(bytes).map_err(|_| skipped("not a readable PDF"))?;
    // lopdf tries the empty password while loading, which opens PDFs that
    // only restrict editing; any other password leaves nothing to read.
    if doc.trailer.get(b"Encrypt").is_ok() && !doc.was_encrypted() {
        return Err(skipped("encrypted"));
    }
    let numbers: Vec<u32> = doc.get_pages().into_keys().collect();
    let pages = numbers.len();
    if pages > limits.max_pages {
        return Err(skipped(&format!("more than {} pages", limits.max_pages)));
    }
    let mut text = Bounded {
        text: String::new(),
        max: limits.max_text_bytes,
        full: false,
    };
    for (index, number) in numbers.into_iter().enumerate() {
        // Pages are apart, so the last word of one and the first of the
        // next stay two words for search.
        if index > 0 && fmt::Write::write_str(&mut text, "\n\n").is_err() {
            break;
        }
        // A failed write is the cap, not an error.
        if output_doc_page(&doc, &mut PlainTextOutput::new(&mut text), number).is_err() {
            if text.full {
                break;
            }
            return Err(skipped("not a readable PDF"));
        }
    }
    if text.text.trim().is_empty() {
        return Err(skipped("no text to extract (scanned or image-only)"));
    }
    Ok(Extracted {
        text: text.text,
        pages,
        truncated: text.full,
    })
}

/// The extraction thread's name, which the panic hook keys on.
const THREAD_NAME: &str = "pdf-extract";

/// Whether a panic on the thread named `thread` stays silent: the parser's
/// panic messages can quote document text.
fn is_extraction_thread(thread: Option<&str>) -> bool {
    thread == Some(THREAD_NAME)
}

/// Wraps the panic hook, once per process, so a panic on the extraction
/// thread prints a fixed line instead of its message; `catch_unwind` then
/// reports the PDF as skipped. Other threads keep the previous hook.
fn quiet_extraction_panics() {
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            if is_extraction_thread(std::thread::current().name()) {
                eprintln!("PDF extraction panicked; the PDF is skipped");
            } else {
                previous(info);
            }
        }));
    });
}

/// Runs extractions one at a time, each on its own thread.
pub struct PdfWorker {
    permit: Arc<Semaphore>,
}

impl Default for PdfWorker {
    fn default() -> Self {
        quiet_extraction_panics();
        Self {
            permit: Arc::new(Semaphore::new(1)),
        }
    }
}

impl PdfWorker {
    /// Runs `extract` on `bytes` on a new thread and waits at most
    /// `timeout`. The thread holds the one permit until it ends, so while a
    /// timed-out extraction still runs every new PDF is refused. A panic in
    /// `extract` is caught and reported as an unreadable PDF.
    pub async fn run<F>(
        &self,
        bytes: Vec<u8>,
        limits: PdfLimits,
        timeout: Duration,
        extract: F,
    ) -> Result<Extracted, AppError>
    where
        F: FnOnce(&[u8], &PdfLimits) -> Result<Extracted, AppError> + Send + 'static,
    {
        let Ok(permit) = Arc::clone(&self.permit).try_acquire_owned() else {
            return Err(AppError::Busy("another PDF is still being read".into()));
        };
        let (sender, receiver) = oneshot::channel();
        std::thread::Builder::new()
            .name(THREAD_NAME.into())
            .spawn(move || {
                let result = catch_unwind(AssertUnwindSafe(|| extract(&bytes, &limits)))
                    .unwrap_or_else(|_| Err(skipped("could not be read")));
                // The permit is released only now, when the work has really
                // ended, and before the result is sent, so a caller that
                // got it may start the next PDF at once. The command may
                // have stopped waiting; then the send goes nowhere.
                drop(permit);
                drop(sender.send(result));
            })
            .map_err(|err| AppError::Internal(format!("PDF thread: {err}")))?;
        match tokio::time::timeout(timeout, receiver).await {
            Ok(Ok(result)) => result,
            Ok(Err(_)) => Err(AppError::Internal(
                "the PDF thread ended without a result".into(),
            )),
            Err(_) => Err(AppError::Timeout(format!(
                "reading the PDF took longer than {} s",
                timeout.as_secs()
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;

    use pdf_extract::{Object, Stream, dictionary};

    use super::*;
    use crate::redacted::Redacted;
    use crate::storage::testing::temp_storage;

    /// A PDF with one page per entry of `pages`, each showing its text in
    /// Helvetica.
    fn text_pdf(pages: &[&str]) -> Vec<u8> {
        let mut doc = Document::with_version("1.5");
        let pages_id = doc.new_object_id();
        let font = doc.add_object(dictionary! {
            "Type" => "Font",
            "Subtype" => "Type1",
            "BaseFont" => "Helvetica",
        });
        let resources = doc.add_object(dictionary! {
            "Font" => dictionary! { "F1" => font },
        });
        let kids: Vec<Object> = pages
            .iter()
            .map(|text| {
                let content = format!(
                    "BT /F1 12 Tf 72 720 Td ({}) Tj ET",
                    text.replace('\\', "\\\\")
                        .replace('(', "\\(")
                        .replace(')', "\\)")
                );
                let stream = doc.add_object(Stream::new(dictionary! {}, content.into_bytes()));
                doc.add_object(dictionary! {
                    "Type" => "Page",
                    "Parent" => pages_id,
                    "Contents" => stream,
                    "Resources" => resources,
                    "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
                })
                .into()
            })
            .collect();
        let count = kids.len() as i64;
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => kids,
                "Count" => count,
            }),
        );
        let catalog = doc.add_object(dictionary! {
            "Type" => "Catalog",
            "Pages" => pages_id,
        });
        doc.trailer.set("Root", catalog);
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).unwrap();
        bytes
    }

    /// `text_pdf(&["secret"])` with a standard security handler whose
    /// password is not empty, so nothing can be read.
    fn encrypted_pdf() -> Vec<u8> {
        let mut doc = Document::load_mem(&text_pdf(&["secret"])).unwrap();
        let encrypt = doc.add_object(dictionary! {
            "Filter" => "Standard",
            "V" => 1,
            "R" => 2,
            "Length" => 40,
            "P" => -4,
            "O" => Object::string_literal(vec![0x11; 32]),
            "U" => Object::string_literal(vec![0x22; 32]),
        });
        doc.trailer.set("Encrypt", encrypt);
        doc.trailer.set(
            "ID",
            vec![
                Object::string_literal(vec![0x33; 16]),
                Object::string_literal(vec![0x33; 16]),
            ],
        );
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes).unwrap();
        bytes
    }

    fn reason(result: Result<Extracted, AppError>) -> String {
        match result {
            Err(AppError::InvalidInput(reason)) => reason,
            other => panic!("expected a skip, got {other:?}"),
        }
    }

    fn extracted(text: &str) -> Extracted {
        Extracted {
            text: text.into(),
            pages: 1,
            truncated: false,
        }
    }

    #[test]
    fn extracts_text_from_a_text_pdf() {
        let pdf = extract_text(
            &text_pdf(&["Quarterly revenue grew"]),
            &PdfLimits::default(),
        )
        .unwrap();
        assert_eq!(pdf.text.trim(), "Quarterly revenue grew");
        assert_eq!(pdf.pages, 1);
        assert!(!pdf.truncated);
    }

    #[test]
    fn extracts_every_page_in_order() {
        let pdf = extract_text(
            &text_pdf(&["alpha", "beta", "gamma"]),
            &PdfLimits::default(),
        )
        .unwrap();
        let words: Vec<&str> = pdf.text.split_whitespace().collect();
        assert_eq!(words, ["alpha", "beta", "gamma"]);
        assert_eq!(pdf.pages, 3);
    }

    #[test]
    fn rejects_bytes_that_are_not_a_pdf() {
        let limits = PdfLimits::default();
        assert_eq!(
            reason(extract_text(b"hello", &limits)),
            "not a readable PDF"
        );
        let mut cut = text_pdf(&["alpha"]);
        cut.truncate(cut.len() / 3);
        assert_eq!(reason(extract_text(&cut, &limits)), "not a readable PDF");
    }

    #[test]
    fn rejects_an_encrypted_pdf() {
        assert_eq!(
            reason(extract_text(&encrypted_pdf(), &PdfLimits::default())),
            "encrypted"
        );
    }

    #[test]
    fn rejects_a_pdf_without_text() {
        assert_eq!(
            reason(extract_text(&text_pdf(&[""]), &PdfLimits::default())),
            "no text to extract (scanned or image-only)"
        );
    }

    #[test]
    fn rejects_an_oversize_pdf_before_parsing() {
        let limits = PdfLimits {
            max_bytes: 1024 * 1024,
            ..PdfLimits::default()
        };
        // Not a PDF at all: the size check comes first.
        let result = extract_text(&vec![b'x'; limits.max_bytes + 1], &limits);
        assert_eq!(
            result,
            Err(AppError::TooLarge("a PDF is at most 1 MiB".into()))
        );
    }

    #[test]
    fn rejects_too_many_pages() {
        let limits = PdfLimits {
            max_pages: 2,
            ..PdfLimits::default()
        };
        assert_eq!(
            reason(extract_text(&text_pdf(&["a", "b", "c"]), &limits)),
            "more than 2 pages"
        );
        assert!(extract_text(&text_pdf(&["a", "b"]), &limits).is_ok());
    }

    #[test]
    fn stops_at_the_output_cap() {
        let limits = PdfLimits {
            max_text_bytes: 12,
            ..PdfLimits::default()
        };
        let pdf = extract_text(&text_pdf(&["first page", "second page"]), &limits).unwrap();
        assert!(pdf.truncated);
        assert!(pdf.text.len() <= 12);
        assert!(pdf.text.contains("first"));
        assert!(!pdf.text.contains("second"));
    }

    #[test]
    fn stops_at_the_output_cap_inside_one_page() {
        let limits = PdfLimits {
            max_text_bytes: 20,
            ..PdfLimits::default()
        };
        let long = "word ".repeat(200);
        let pdf = extract_text(&text_pdf(&[&long]), &limits).unwrap();
        assert!(pdf.truncated);
        assert!(pdf.text.len() <= 20);
    }

    #[test]
    fn bounded_cuts_at_a_char_boundary() {
        use std::fmt::Write;
        let mut text = Bounded {
            text: String::new(),
            max: 4,
            full: false,
        };
        assert!(text.write_str("ab").is_ok());
        assert!(text.write_str("äö").is_err());
        assert_eq!(text.text, "abä");
        assert!(text.full);
    }

    #[test]
    fn extracted_text_is_found_by_search() {
        let (storage, _dir) = temp_storage();
        let project = storage.create_project("p").unwrap().id;
        let pdf = extract_text(
            &text_pdf(&["The heron nests by the lake"]),
            &PdfLimits::default(),
        )
        .unwrap();
        storage
            .add_file(
                project,
                &Redacted::new("birds.pdf"),
                &Redacted::new(pdf.text),
            )
            .unwrap();
        let hits = storage.search(project, "heron", 5).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].name.expose(), "birds.pdf");
        assert!(storage.search(project, "unrelated", 5).unwrap().is_empty());
    }

    #[tokio::test]
    async fn run_returns_the_extraction() {
        let worker = PdfWorker::default();
        let result = worker
            .run(Vec::new(), PdfLimits::default(), PDF_TIMEOUT, |_, _| {
                Ok(extracted("text"))
            })
            .await;
        assert_eq!(result, Ok(extracted("text")));
    }

    #[tokio::test]
    async fn run_times_out() {
        let worker = PdfWorker::default();
        let (release, wait) = mpsc::channel::<()>();
        let result = worker
            .run(
                Vec::new(),
                PdfLimits::default(),
                Duration::from_millis(50),
                move |_, _| {
                    wait.recv().ok();
                    Ok(extracted("late"))
                },
            )
            .await;
        assert!(matches!(result, Err(AppError::Timeout(_))), "{result:?}");
        release.send(()).unwrap();
    }

    #[tokio::test]
    async fn run_refuses_a_second_pdf_while_a_timed_out_extraction_runs() {
        let worker = PdfWorker::default();
        let (release, wait) = mpsc::channel::<()>();
        let (finished, ended) = mpsc::channel::<()>();
        let first = worker
            .run(
                Vec::new(),
                PdfLimits::default(),
                Duration::from_millis(50),
                move |_, _| {
                    wait.recv().ok();
                    // Sent before the closure returns: the permit is
                    // released right after, so the test polls below.
                    finished.send(()).ok();
                    Ok(extracted("late"))
                },
            )
            .await;
        assert!(matches!(first, Err(AppError::Timeout(_))));

        let second = worker
            .run(Vec::new(), PdfLimits::default(), PDF_TIMEOUT, |_, _| {
                Ok(extracted("second"))
            })
            .await;
        assert_eq!(
            second,
            Err(AppError::Busy("another PDF is still being read".into()))
        );

        release.send(()).unwrap();
        ended.recv().unwrap();
        // The thread drops the permit just after `finished`.
        for _ in 0..200 {
            if worker.permit.available_permits() == 1 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        let third = worker
            .run(Vec::new(), PdfLimits::default(), PDF_TIMEOUT, |_, _| {
                Ok(extracted("third"))
            })
            .await;
        assert_eq!(third, Ok(extracted("third")));
    }

    #[test]
    fn only_the_extraction_thread_panics_quietly() {
        assert!(is_extraction_thread(Some("pdf-extract")));
        assert!(!is_extraction_thread(Some("main")));
        assert!(!is_extraction_thread(None));
    }

    #[tokio::test]
    async fn run_catches_a_panic() {
        let worker = PdfWorker::default();
        let result = worker
            .run(Vec::new(), PdfLimits::default(), PDF_TIMEOUT, |_, _| {
                panic!("malformed font")
            })
            .await;
        assert_eq!(
            result,
            Err(AppError::InvalidInput("could not be read".into()))
        );
        // The permit came back with the thread.
        let next = worker
            .run(Vec::new(), PdfLimits::default(), PDF_TIMEOUT, |_, _| {
                Ok(extracted("next"))
            })
            .await;
        assert_eq!(next, Ok(extracted("next")));
    }
}
