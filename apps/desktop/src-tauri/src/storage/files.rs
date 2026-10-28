//! A project's file library: copies of text files the user added, split
//! into chunks for full-text search. The webview reads the files; nothing
//! here reads a disk path. Names and content are the user's text, so both
//! are redacted in logs.

use rusqlite::{Connection, OptionalExtension, Row, params};
use serde::Serialize;
use uuid::Uuid;

use super::Storage;
use super::conversations::{NOW, uuid_column};
use super::memory::truncate_at;
use super::projects::check_exists;
use crate::error::AppError;
use crate::redacted::Redacted;

/// The largest file, in bytes of UTF-8.
pub const MAX_FILE_BYTES: usize = 4 * 1024 * 1024;
/// A file name is a relative path inside a picked folder, so it may be long.
pub const MAX_FILE_NAME_CHARS: usize = 1024;
/// A chunk closes at this many lines,
const CHUNK_LINES: u32 = 40;
/// or before it would grow past this many bytes.
const CHUNK_BYTES: usize = 2048;
/// Search uses at most this many terms of a query.
const MAX_SEARCH_TERMS: usize = 16;
const MAX_SEARCH_HITS: u32 = 50;
/// How many passages a chat's turn gets at most.
pub const TURN_PASSAGES: u32 = 6;
const MAX_PAGE: u32 = 500;
/// `read_lines` returns at most this many lines,
pub const MAX_READ_LINES: u32 = 200;
/// and this many bytes,
pub const MAX_READ_BYTES: usize = 16 * 1024;
/// with each line cut to this many bytes.
const MAX_READ_LINE_BYTES: usize = 2048;
const LINE_TRUNCATED: &str = "…[line truncated]";

/// One file of a project, without its content; mirrored by `ProjectFile` in
/// `src/lib/types.ts`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectFile {
    pub id: Uuid,
    pub name: Redacted,
    pub bytes: u32,
    pub line_count: u32,
    pub created_at: String,
}

/// One page of a project's files, by name; mirrored by `FilePage` in
/// `src/lib/types.ts`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FilePage {
    pub files: Vec<ProjectFile>,
    /// How many files the project has in all.
    pub total: u32,
}

/// A chunk that matched a search, best first.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchHit {
    pub name: Redacted,
    pub start_line: u32,
    pub end_line: u32,
    pub snippet: Redacted,
    /// The whole chunk.
    pub text: Redacted,
}

/// Numbered lines of a file, `"{n}\t{line}\n"` each. Empty when `start` is
/// past the end, with `last_line` then `first_line - 1`.
#[derive(Debug, Clone, PartialEq)]
pub struct LineRange {
    pub text: Redacted,
    pub first_line: u32,
    pub last_line: u32,
    pub total_lines: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Chunk {
    start_line: u32,
    end_line: u32,
    text: String,
}

const COLUMNS: &str = "id, name, bytes, line_count, created_at";

fn file_row(row: &Row<'_>) -> rusqlite::Result<ProjectFile> {
    Ok(ProjectFile {
        id: uuid_column(row, 0)?,
        name: Redacted::new(row.get::<_, String>(1)?),
        bytes: row.get(2)?,
        line_count: row.get(3)?,
        created_at: row.get(4)?,
    })
}

fn file_not_found() -> AppError {
    AppError::NotFound("no such file".into())
}

/// Kept exactly as given, so `a.txt` and ` a.txt` stay two files: not
/// blank, at most [`MAX_FILE_NAME_CHARS`], no control characters.
fn check_file_name(name: &str) -> Result<(), AppError> {
    if name.trim().is_empty()
        || name.chars().count() > MAX_FILE_NAME_CHARS
        || name.chars().any(char::is_control)
    {
        return Err(AppError::InvalidInput(format!(
            "a file name is 1 to {MAX_FILE_NAME_CHARS} characters on one line"
        )));
    }
    Ok(())
}

fn check_content(content: &str) -> Result<(), AppError> {
    if content.len() > MAX_FILE_BYTES {
        return Err(AppError::InvalidInput(format!(
            "a file is at most {MAX_FILE_BYTES} bytes"
        )));
    }
    if content.contains('\0') {
        return Err(AppError::InvalidInput("a file is text, without NUL".into()));
    }
    Ok(())
}

/// `text` in pieces of at most `max` bytes, cut at char boundaries.
fn pieces(text: &str, max: usize) -> impl Iterator<Item = &str> {
    let mut rest = text;
    std::iter::from_fn(move || {
        if rest.is_empty() {
            return None;
        }
        let mut cut = truncate_at(rest, max).len();
        // Cut after the last separator, so no word is split between two
        // chunks and lost to search; only a word longer than `max` is.
        if cut < rest.len()
            && let Some((at, separator)) = rest[..cut]
                .char_indices()
                .rev()
                .find(|(_, c)| !c.is_alphanumeric())
        {
            cut = at + separator.len_utf8();
        }
        let (piece, tail) = rest.split_at(cut);
        rest = tail;
        Some(piece)
    })
}

/// Chunks of about [`CHUNK_LINES`] lines and at most [`CHUNK_BYTES`],
/// split on line boundaries; lines are numbered from 1. A line longer than
/// [`CHUNK_BYTES`] becomes several chunks that all carry its number.
fn chunk_lines(content: &str) -> Vec<Chunk> {
    let mut chunks = Vec::new();
    let mut text = String::new();
    let mut start_line = 0;
    let mut lines = 0;
    let flush = |chunks: &mut Vec<Chunk>, text: &mut String, start_line, lines: &mut u32| {
        if *lines > 0 {
            chunks.push(Chunk {
                start_line,
                end_line: start_line + *lines - 1,
                text: std::mem::take(text),
            });
            *lines = 0;
        }
    };
    for (number, line) in (1..).zip(content.lines()) {
        if line.len() > CHUNK_BYTES {
            flush(&mut chunks, &mut text, start_line, &mut lines);
            chunks.extend(pieces(line, CHUNK_BYTES).map(|piece| Chunk {
                start_line: number,
                end_line: number,
                text: piece.to_owned(),
            }));
            continue;
        }
        if lines == CHUNK_LINES || (lines > 0 && text.len() + 1 + line.len() > CHUNK_BYTES) {
            flush(&mut chunks, &mut text, start_line, &mut lines);
        }
        if lines == 0 {
            start_line = number;
        } else {
            text.push('\n');
        }
        text.push_str(line);
        lines += 1;
    }
    flush(&mut chunks, &mut text, start_line, &mut lines);
    chunks
}

/// An FTS5 query matching any of the first [`MAX_SEARCH_TERMS`] terms, each
/// quoted so no FTS syntax gets through. Terms with nothing the tokenizer
/// indexes are dropped; `None` when no term is left.
fn match_expression(query: &str) -> Option<String> {
    // A NUL inside a quoted term still breaks FTS5's parser, so control
    // characters separate terms like whitespace.
    let terms: Vec<String> = query
        .split(|c: char| c.is_whitespace() || c.is_control())
        .filter(|term| term.chars().any(char::is_alphanumeric))
        .take(MAX_SEARCH_TERMS)
        .map(|term| format!("\"{}\"", term.replace('"', "\"\"")))
        .collect();
    (!terms.is_empty()).then(|| terms.join(" OR "))
}

/// The chunks of project `project` matching any term of `query`, best
/// first, at most `limit` (up to 50). Read on `conn` so a transaction sees
/// its own snapshot. A query with no searchable term finds nothing.
pub(super) fn search_chunks(
    conn: &Connection,
    project: &str,
    query: &str,
    limit: u32,
) -> rusqlite::Result<Vec<SearchHit>> {
    let Some(expression) = match_expression(query) else {
        return Ok(Vec::new());
    };
    conn.prepare(
        "SELECT f.name, c.start_line, c.end_line,
                snippet(file_chunks_fts, 0, '', '', '…', 32), c.text
         FROM file_chunks_fts
         JOIN file_chunks c ON c.id = file_chunks_fts.rowid
         JOIN project_files f ON f.id = c.file_id
         WHERE file_chunks_fts MATCH ?1 AND f.project_id = ?2
         ORDER BY bm25(file_chunks_fts), c.id
         LIMIT ?3",
    )?
    .query_map(
        params![expression, project, limit.clamp(1, MAX_SEARCH_HITS)],
        |row| {
            Ok(SearchHit {
                name: Redacted::new(row.get::<_, String>(0)?),
                start_line: row.get(1)?,
                end_line: row.get(2)?,
                snippet: Redacted::new(row.get::<_, String>(3)?),
                text: Redacted::new(row.get::<_, String>(4)?),
            })
        },
    )?
    .collect()
}

/// Whether project `project` has any file.
pub(super) fn has_files(conn: &Connection, project: &str) -> rusqlite::Result<bool> {
    conn.query_row(
        "SELECT EXISTS (SELECT 1 FROM project_files WHERE project_id = ?1)",
        [project],
        |row| row.get(0),
    )
}

/// `line` for `read_lines`: cut to [`MAX_READ_LINE_BYTES`] with a marker.
fn shown_line(line: &str) -> String {
    if line.len() > MAX_READ_LINE_BYTES {
        format!("{}{LINE_TRUNCATED}", truncate_at(line, MAX_READ_LINE_BYTES))
    } else {
        line.to_owned()
    }
}

impl Storage {
    /// Validates, stores and indexes one file in one transaction. A file of
    /// the same name in the project is replaced.
    pub fn add_file(
        &self,
        project_id: Uuid,
        name: &Redacted,
        content: &Redacted,
    ) -> Result<ProjectFile, AppError> {
        let name = name.expose();
        let content = content.expose();
        check_file_name(name)?;
        check_content(content)?;
        let chunks = chunk_lines(content);
        // Both fit: the content is at most 4 MiB.
        let line_count = content.lines().count() as u32;
        let project = project_id.to_string();

        let mut conn = self.conn();
        let tx = conn.transaction()?;
        check_exists(&tx, &project)?;
        tx.execute(
            "DELETE FROM project_files WHERE project_id = ?1 AND name = ?2",
            params![project, name],
        )?;
        let file = tx.query_row(
            &format!(
                "INSERT INTO project_files
                   (id, project_id, name, content, bytes, line_count, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, {NOW}) RETURNING {COLUMNS}"
            ),
            params![
                Uuid::new_v4().to_string(),
                project,
                name,
                content,
                content.len() as u32,
                line_count
            ],
            file_row,
        )?;
        {
            let mut insert = tx.prepare(
                "INSERT INTO file_chunks (file_id, start_line, end_line, text)
                 VALUES (?1, ?2, ?3, ?4)",
            )?;
            let file_id = file.id.to_string();
            for chunk in &chunks {
                insert.execute(params![
                    file_id,
                    chunk.start_line,
                    chunk.end_line,
                    chunk.text
                ])?;
            }
        }
        tx.commit()?;
        Ok(file)
    }

    /// Deleting one that is already gone is not an error.
    pub fn delete_file(&self, project_id: Uuid, id: Uuid) -> Result<(), AppError> {
        self.conn().execute(
            "DELETE FROM project_files WHERE id = ?1 AND project_id = ?2",
            params![id.to_string(), project_id.to_string()],
        )?;
        Ok(())
    }

    /// Files by name, `limit` (at most 500) from `offset`, and how many there
    /// are in all.
    pub fn list_files(
        &self,
        project_id: Uuid,
        offset: u32,
        limit: u32,
    ) -> Result<FilePage, AppError> {
        let project = project_id.to_string();
        let mut conn = self.conn();
        let tx = conn.transaction()?;
        check_exists(&tx, &project)?;
        let total = tx.query_row(
            "SELECT COUNT(*) FROM project_files WHERE project_id = ?1",
            [&project],
            |row| row.get(0),
        )?;
        let files = tx
            .prepare(&format!(
                "SELECT {COLUMNS} FROM project_files WHERE project_id = ?1
                 ORDER BY name LIMIT ?2 OFFSET ?3"
            ))?
            .query_map(params![project, limit.clamp(1, MAX_PAGE), offset], file_row)?
            .collect::<Result<_, _>>()?;
        Ok(FilePage { files, total })
    }

    /// Whether project `project_id` has any file.
    pub fn project_has_files(&self, project_id: Uuid) -> Result<bool, AppError> {
        Ok(has_files(&self.conn(), &project_id.to_string())?)
    }

    /// The project's chunks matching any term of `query`, best first, at
    /// most `limit` (up to 50). A query with no searchable term finds
    /// nothing.
    pub fn search(
        &self,
        project_id: Uuid,
        query: &str,
        limit: u32,
    ) -> Result<Vec<SearchHit>, AppError> {
        Ok(search_chunks(
            &self.conn(),
            &project_id.to_string(),
            query,
            limit,
        )?)
    }

    /// Up to `count` numbered lines of file `name` from line `start` (from
    /// 1), at most [`MAX_READ_LINES`] lines and [`MAX_READ_BYTES`].
    pub fn read_lines(
        &self,
        project_id: Uuid,
        name: &str,
        start: u32,
        count: u32,
    ) -> Result<LineRange, AppError> {
        let (content, total_lines): (String, u32) = self
            .conn()
            .query_row(
                "SELECT content, line_count FROM project_files
                 WHERE project_id = ?1 AND name = ?2",
                params![project_id.to_string(), name],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?
            .ok_or_else(file_not_found)?;
        let first_line = start.max(1);
        let mut last_line = first_line - 1;
        let mut text = String::new();
        let skip = usize::try_from(first_line - 1).unwrap_or(usize::MAX);
        let take = count.min(MAX_READ_LINES) as usize;
        for (number, line) in (first_line..).zip(content.lines().skip(skip).take(take)) {
            let entry = format!("{number}\t{}\n", shown_line(line));
            if text.len() + entry.len() > MAX_READ_BYTES {
                break;
            }
            text.push_str(&entry);
            last_line = number;
        }
        Ok(LineRange {
            text: Redacted::new(text),
            first_line,
            last_line,
            total_lines,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::testing::{count_rows, temp_storage};

    fn add(storage: &Storage, project: Uuid, name: &str, content: &str) -> ProjectFile {
        storage
            .add_file(project, &Redacted::new(name), &Redacted::new(content))
            .unwrap()
    }

    fn names(hits: &[SearchHit]) -> Vec<&str> {
        hits.iter().map(|hit| hit.name.expose()).collect()
    }

    fn numbered(lines: std::ops::RangeInclusive<usize>) -> String {
        lines
            .map(|n| format!("line {n}"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn chunks_split_on_lines_and_size() {
        let chunks = chunk_lines(&numbered(1..=100));
        let ranges: Vec<(u32, u32)> = chunks
            .iter()
            .map(|chunk| (chunk.start_line, chunk.end_line))
            .collect();
        assert_eq!(ranges, [(1, 40), (41, 80), (81, 100)]);
        assert_eq!(chunks[0].text, numbered(1..=40));

        // 30 lines of 100 bytes: a 2 KiB chunk holds 20 of them.
        let wide = vec!["x".repeat(100); 30].join("\n");
        let chunks = chunk_lines(&wide);
        assert_eq!(
            chunks
                .iter()
                .map(|chunk| (chunk.start_line, chunk.end_line))
                .collect::<Vec<_>>(),
            [(1, 20), (21, 30)]
        );
        assert!(chunks.iter().all(|chunk| chunk.text.len() <= CHUNK_BYTES));

        assert!(chunk_lines("").is_empty());
        assert_eq!(chunk_lines("a\r\nb\n").len(), 1);
        assert_eq!(chunk_lines("a\r\nb\n")[0].text, "a\nb");
    }

    #[test]
    fn long_single_line_splits_at_char_boundaries() {
        let (storage, _dir) = temp_storage();
        let project = storage.create_project("p").unwrap().id;
        // "ä" is two bytes, so 2048-byte pieces fall on char boundaries only
        // because the cut moves back; the odd prefix makes them all odd.
        let line = format!("x{}", "ä".repeat(MAX_FILE_BYTES / 2 - 1));
        assert!(line.len() <= MAX_FILE_BYTES);

        let chunks = chunk_lines(&format!("before\n{line}\nafter"));
        assert_eq!(chunks.first().unwrap().text, "before");
        assert_eq!(chunks.last().unwrap().text, "after");
        let middle = &chunks[1..chunks.len() - 1];
        assert!(middle.iter().all(|chunk| chunk.start_line == 2
            && chunk.end_line == 2
            && chunk.text.len() <= CHUNK_BYTES));
        let joined: String = middle.iter().map(|chunk| chunk.text.as_str()).collect();
        assert_eq!(joined, line);

        let file = add(&storage, project, "one-line.txt", &line);
        assert_eq!(file.line_count, 1);
        assert_eq!(file.bytes, line.len() as u32);
        let read = storage.read_lines(project, "one-line.txt", 1, 10).unwrap();
        assert_eq!(
            (read.first_line, read.last_line, read.total_lines),
            (1, 1, 1)
        );
        assert!(read.text.expose().starts_with("1\tx"));
        assert!(read.text.expose().ends_with(&format!("{LINE_TRUNCATED}\n")));
        assert!(read.text.expose().len() <= MAX_READ_BYTES);
    }

    #[test]
    fn long_line_splits_between_words() {
        let (storage, _dir) = temp_storage();
        let project = storage.create_project("p").unwrap().id;
        // A plain cut at 2048 bytes would leave "nee" and "dle".
        let line = format!("{}needle and more words", " ".repeat(2045));
        let chunks = chunk_lines(&line);
        assert_eq!(chunks.len(), 2);
        assert!(chunks[1].text.starts_with("needle"));
        let joined: String = chunks.iter().map(|chunk| chunk.text.as_str()).collect();
        assert_eq!(joined, line);

        add(&storage, project, "wide.txt", &line);
        assert_eq!(
            names(&storage.search(project, "needle", 10).unwrap()),
            ["wide.txt"]
        );
    }

    #[test]
    fn add_same_name_replaces() {
        let (storage, _dir) = temp_storage();
        let project = storage.create_project("p").unwrap().id;
        let first = add(&storage, project, "notes.md", "apples");
        let second = add(&storage, project, "notes.md", "bananas\ncherries");

        assert_ne!(first.id, second.id);
        assert_eq!(second.line_count, 2);
        let page = storage.list_files(project, 0, 100).unwrap();
        assert_eq!((page.total, page.files), (1, vec![second]));
        assert_eq!(count_rows(&storage, "file_chunks"), 1);
        assert!(storage.search(project, "apples", 10).unwrap().is_empty());
        assert_eq!(
            names(&storage.search(project, "cherries", 10).unwrap()),
            ["notes.md"]
        );
    }

    #[test]
    fn search_returns_whole_chunk() {
        let (storage, _dir) = temp_storage();
        let project = storage.create_project("p").unwrap().id;
        assert!(!has_files(&storage.conn(), &project.to_string()).unwrap());
        add(&storage, project, "a.md", &numbered(1..=50));

        let hits = storage.search(project, "line 7", 1).unwrap();
        assert_eq!(hits[0].text.expose(), numbered(1..=40));
        assert!(hits[0].snippet.len_bytes() < hits[0].text.len_bytes());
        assert!(has_files(&storage.conn(), &project.to_string()).unwrap());
    }

    #[test]
    fn add_validates_name_and_content() {
        let (storage, _dir) = temp_storage();
        let project = storage.create_project("p").unwrap().id;
        let long = "x".repeat(MAX_FILE_NAME_CHARS + 1);
        for name in ["", "  ", "two\nlines", "tab\tname", long.as_str()] {
            assert!(matches!(
                storage.add_file(project, &Redacted::new(name), &Redacted::new("x")),
                Err(AppError::InvalidInput(_))
            ));
        }
        for content in ["x".repeat(MAX_FILE_BYTES + 1), "a\0b".to_owned()] {
            assert!(matches!(
                storage.add_file(project, &Redacted::new("a.txt"), &Redacted::new(content)),
                Err(AppError::InvalidInput(_))
            ));
        }
        assert!(matches!(
            storage.add_file(Uuid::new_v4(), &Redacted::new("a.txt"), &Redacted::new("x")),
            Err(AppError::NotFound(_))
        ));
        assert_eq!(count_rows(&storage, "project_files"), 0);

        // Names are kept as given: these are two files.
        add(&storage, project, "a.txt", "x");
        add(&storage, project, " a.txt", "x");
        add(
            &storage,
            project,
            "dir/sub/b.txt",
            &"x".repeat(MAX_FILE_BYTES),
        );
        add(&storage, project, "empty.txt", "");
        let page = storage.list_files(project, 0, 100).unwrap();
        let listed: Vec<&str> = page.files.iter().map(|file| file.name.expose()).collect();
        assert_eq!(listed, [" a.txt", "a.txt", "dir/sub/b.txt", "empty.txt"]);
        assert_eq!(page.files[3].line_count, 0);
    }

    #[test]
    fn list_files_pages_by_name() {
        let (storage, _dir) = temp_storage();
        let project = storage.create_project("p").unwrap().id;
        for name in ["c", "a", "b", "d"] {
            add(&storage, project, name, "x");
        }
        let page = storage.list_files(project, 1, 2).unwrap();
        assert_eq!(page.total, 4);
        let listed: Vec<&str> = page.files.iter().map(|file| file.name.expose()).collect();
        assert_eq!(listed, ["b", "c"]);
        assert!(storage.list_files(project, 10, 2).unwrap().files.is_empty());
        assert_eq!(storage.list_files(project, 0, 0).unwrap().files.len(), 1);
        assert!(matches!(
            storage.list_files(Uuid::new_v4(), 0, 10),
            Err(AppError::NotFound(_))
        ));
    }

    #[test]
    fn delete_file_removes_chunks_from_search() {
        let (storage, _dir) = temp_storage();
        let project = storage.create_project("p").unwrap().id;
        let file = add(&storage, project, "a.txt", &numbered(1..=90));
        add(&storage, project, "b.txt", "line kept");
        assert_eq!(count_rows(&storage, "file_chunks"), 4);

        storage.delete_file(project, file.id).unwrap();
        storage.delete_file(project, file.id).unwrap();

        assert_eq!(count_rows(&storage, "file_chunks"), 1);
        assert_eq!(
            names(&storage.search(project, "line", 10).unwrap()),
            ["b.txt"]
        );
        assert_eq!(storage.list_files(project, 0, 10).unwrap().total, 1);
    }

    #[test]
    fn project_delete_cascades_files() {
        let (storage, _dir) = temp_storage();
        let project = storage.create_project("p").unwrap().id;
        let other = storage.create_project("q").unwrap().id;
        add(&storage, project, "a.txt", &numbered(1..=50));
        add(&storage, other, "b.txt", "line");

        storage.delete_project(project).unwrap();

        assert_eq!(count_rows(&storage, "project_files"), 1);
        assert_eq!(count_rows(&storage, "file_chunks"), 1);
        // The FTS index lost the deleted rows too, not only the content table.
        let indexed: i64 = storage
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM file_chunks_fts WHERE file_chunks_fts MATCH 'line'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(indexed, 1);
    }

    #[test]
    fn search_folds_finnish_diacritics() {
        let (storage, _dir) = temp_storage();
        let project = storage.create_project("p").unwrap().id;
        add(
            &storage,
            project,
            "päätös.md",
            "Hallituksen päätös tehtiin.\nMuuta.",
        );
        add(&storage, project, "other.md", "Nothing here.");

        for query in ["paatos", "PÄÄTÖS", "päätös hallituksen"] {
            let hits = storage.search(project, query, 10).unwrap();
            assert_eq!(names(&hits), ["päätös.md"], "{query}");
            assert_eq!((hits[0].start_line, hits[0].end_line), (1, 2));
            assert!(hits[0].snippet.expose().contains("päätös"));
        }
    }

    #[test]
    fn hostile_queries_do_not_error() {
        let (storage, _dir) = temp_storage();
        let project = storage.create_project("p").unwrap().id;
        add(
            &storage,
            project,
            "a.txt",
            "near the end, a star * and a - dash",
        );

        for query in [
            "\"",
            "*",
            "NEAR(",
            "-",
            "",
            "   ",
            "a\"b",
            "end*",
            "NEAR(near end)",
            "text:end",
            "AND",
            "OR OR",
            "^end",
            "(",
            ")",
            "{text}",
            "x\0y",
        ] {
            assert!(storage.search(project, query, 10).is_ok(), "{query:?}");
        }
        assert_eq!(
            names(&storage.search(project, "NEAR(", 10).unwrap()),
            ["a.txt"]
        );
        assert!(storage.search(project, "*", 10).unwrap().is_empty());
        assert!(storage.search(project, "", 10).unwrap().is_empty());
    }

    #[test]
    fn search_caps_terms_and_hits() {
        let (storage, _dir) = temp_storage();
        let project = storage.create_project("p").unwrap().id;
        add(&storage, project, "a.txt", "needle");
        let mut query = vec!["miss"; MAX_SEARCH_TERMS];
        query.push("needle");
        assert!(
            storage
                .search(project, &query.join(" "), 10)
                .unwrap()
                .is_empty()
        );

        for n in 0..60 {
            add(&storage, project, &format!("{n}.txt"), "common");
        }
        assert_eq!(storage.search(project, "common", 1000).unwrap().len(), 50);
        assert_eq!(storage.search(project, "common", 0).unwrap().len(), 1);
    }

    #[test]
    fn read_lines_caps() {
        let (storage, _dir) = temp_storage();
        let project = storage.create_project("p").unwrap().id;
        add(&storage, project, "many.txt", &numbered(1..=500));

        let read = storage.read_lines(project, "many.txt", 1, 1000).unwrap();
        assert_eq!(
            (read.first_line, read.last_line, read.total_lines),
            (1, 200, 500)
        );
        assert!(read.text.expose().starts_with("1\tline 1\n2\tline 2\n"));

        let read = storage.read_lines(project, "many.txt", 499, 10).unwrap();
        assert_eq!(read.text.expose(), "499\tline 499\n500\tline 500\n");
        let past = storage.read_lines(project, "many.txt", 501, 10).unwrap();
        assert_eq!(
            (past.text.expose(), past.first_line, past.last_line),
            ("", 501, 500)
        );
        let from_zero = storage.read_lines(project, "many.txt", 0, 1).unwrap();
        assert_eq!(from_zero.text.expose(), "1\tline 1\n");

        // 200 lines of 2 KiB pass the byte cap first.
        let wide = vec!["y".repeat(MAX_READ_LINE_BYTES); 200].join("\n");
        add(&storage, project, "wide.txt", &wide);
        let read = storage.read_lines(project, "wide.txt", 1, 200).unwrap();
        assert!(read.text.expose().len() <= MAX_READ_BYTES);
        assert_eq!(read.last_line, 7);
        assert!(!read.text.expose().contains(LINE_TRUNCATED));

        assert!(matches!(
            storage.read_lines(project, "missing.txt", 1, 1),
            Err(AppError::NotFound(_))
        ));
    }

    #[test]
    fn files_are_isolated_per_project() {
        let (storage, _dir) = temp_storage();
        let project = storage.create_project("p").unwrap().id;
        let other = storage.create_project("q").unwrap().id;
        let file = add(&storage, project, "secret.txt", "classified");
        add(&storage, other, "public.txt", "open");

        assert!(storage.search(other, "classified", 10).unwrap().is_empty());
        assert!(matches!(
            storage.read_lines(other, "secret.txt", 1, 1),
            Err(AppError::NotFound(_))
        ));
        assert_eq!(storage.list_files(other, 0, 10).unwrap().total, 1);
        // Deleting through the wrong project leaves the file.
        storage.delete_file(other, file.id).unwrap();
        assert_eq!(storage.list_files(project, 0, 10).unwrap().total, 1);
    }

    #[test]
    fn file_names_and_text_debug_redacted() {
        let (storage, _dir) = temp_storage();
        let project = storage.create_project("p").unwrap().id;
        let file = add(&storage, project, "Secret plans.txt", "Never reveal this");
        let hits = storage.search(project, "reveal", 1).unwrap();
        let read = storage
            .read_lines(project, "Secret plans.txt", 1, 1)
            .unwrap();

        let debug = format!("{file:?} {hits:?} {read:?}");
        assert!(!debug.contains("Secret"));
        assert!(!debug.contains("reveal"));
    }
}
