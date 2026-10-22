//! A saved conversation as a Markdown document, for Copy as Markdown.

use uuid::Uuid;

use super::Storage;
use super::conversations::ConversationWithMessages;
use crate::error::AppError;
use crate::ollama::types::ChatRole;
use crate::redacted::Redacted;

/// An open code fence: its character and length, and the line that opened it.
struct Fence<'a> {
    marker: char,
    len: usize,
    opening: &'a str,
}

/// The fence `line` opens or closes, as CommonMark reads it: indented at
/// most three spaces, then three or more of one of `` ` `` or `~`.
fn fence_run(line: &str) -> Option<(char, usize, &str)> {
    let trimmed = line.trim_start_matches(' ');
    if line.len() - trimmed.len() > 3 {
        return None;
    }
    let marker = trimmed.chars().next().filter(|c| matches!(c, '`' | '~'))?;
    let len = trimmed.chars().take_while(|&c| c == marker).count();
    (len >= 3).then(|| (marker, len, &trimmed[len..]))
}

/// The fence still open at the end of `text`, if any.
fn open_fence(text: &str) -> Option<Fence<'_>> {
    let mut open: Option<Fence<'_>> = None;
    for line in text.lines() {
        let Some((marker, len, rest)) = fence_run(line) else {
            continue;
        };
        match &open {
            None => {
                // A backtick fence's info string cannot contain a backtick.
                if marker == '`' && rest.contains('`') {
                    continue;
                }
                let indent = line.len() - line.trim_start_matches(' ').len();
                open = Some(Fence {
                    marker,
                    len,
                    opening: &line[indent..indent + len],
                });
            }
            Some(fence) => {
                if marker == fence.marker && len >= fence.len && rest.trim().is_empty() {
                    open = None;
                }
            }
        }
    }
    open
}

/// Title, model and date, then a `## You` or `## Assistant` section per
/// message with text. Thinking is never included. A fence a stopped reply
/// left open is closed, so it cannot swallow the next section.
pub fn to_markdown(chat: &ConversationWithMessages) -> Redacted {
    let conversation = &chat.conversation;
    let date = conversation.created_at.get(..10).unwrap_or_default();
    let mut doc = format!("# {}\n\n", conversation.title.expose());
    if conversation.model.is_empty() {
        doc.push_str(date);
    } else {
        doc.push_str(&format!("Model: {} · {date}", conversation.model));
    }
    doc.push('\n');
    for message in &chat.messages {
        let heading = match message.role {
            ChatRole::User => "You",
            ChatRole::Assistant => "Assistant",
            ChatRole::System => continue,
        };
        let text = message.content.expose().trim_end();
        if text.is_empty() {
            continue;
        }
        doc.push_str(&format!("\n## {heading}\n\n{text}\n"));
        if let Some(fence) = open_fence(text) {
            doc.push_str(fence.opening);
            doc.push('\n');
        }
    }
    Redacted::new(doc)
}

impl Storage {
    /// The conversation as Markdown; nothing is written anywhere.
    pub fn conversation_markdown(&self, id: Uuid) -> Result<Redacted, AppError> {
        Ok(to_markdown(&self.get_conversation(id)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::conversations::{Conversation, ConversationSettings};
    use crate::storage::messages::{MessageStatus, StoredMessage};
    use crate::storage::testing::temp_storage;

    fn message(role: ChatRole, content: &str) -> StoredMessage {
        StoredMessage {
            id: Uuid::new_v4(),
            role,
            content: Redacted::new(content),
            status: MessageStatus::Complete,
            created_at: "x".into(),
            prompt_tokens: None,
            completion_tokens: None,
            duration_ms: None,
            thinking: None,
            thinking_ms: None,
            eval_ms: None,
        }
    }

    fn chat(messages: Vec<StoredMessage>) -> ConversationWithMessages {
        ConversationWithMessages {
            conversation: Conversation {
                id: Uuid::nil(),
                title: Redacted::new("Trip plan"),
                model: "qwen3:8b".into(),
                created_at: "2026-10-22T08:15:00.000Z".into(),
                updated_at: "2026-10-22T08:15:00.000Z".into(),
                settings: ConversationSettings::default(),
            },
            messages,
        }
    }

    fn markdown(messages: Vec<StoredMessage>) -> String {
        to_markdown(&chat(messages)).expose().to_owned()
    }

    #[test]
    fn markdown_has_title_and_role_sections() {
        let doc = markdown(vec![
            message(ChatRole::User, "Where to?"),
            message(ChatRole::Assistant, "Lapland.\n"),
        ]);
        assert_eq!(
            doc,
            "# Trip plan\n\nModel: qwen3:8b · 2026-10-22\n\n## You\n\nWhere to?\n\n\
             ## Assistant\n\nLapland.\n"
        );
    }

    #[test]
    fn markdown_keeps_code_fences() {
        let code = "Run:\n\n```bash\necho \"## You\"\n```\n\nDone.";
        let doc = markdown(vec![message(ChatRole::Assistant, code)]);
        assert!(doc.ends_with(&format!("## Assistant\n\n{code}\n")), "{doc}");
    }

    #[test]
    fn markdown_excludes_thinking() {
        let mut reply = message(ChatRole::Assistant, "Answer");
        reply.thinking = Some(Redacted::new("private reasoning"));
        let doc = markdown(vec![reply]);
        assert!(!doc.contains("reasoning"));
        assert!(doc.contains("Answer"));
    }

    #[test]
    fn markdown_closes_unbalanced_fence() {
        for (text, close) in [("```rust\nfn main() {", "```"), ("  ~~~~\npartial", "~~~~")] {
            let doc = markdown(vec![
                message(ChatRole::Assistant, text),
                message(ChatRole::User, "Next"),
            ]);
            assert!(
                doc.contains(&format!("{text}\n{close}\n\n## You\n\nNext")),
                "{doc}"
            );
        }
    }

    #[test]
    fn markdown_longer_fence_needs_longer_close() {
        // The inner three-backtick lines are content of the four-backtick
        // fence, which the reply never closed.
        let text = "````md\n```js\nx\n```";
        let doc = markdown(vec![message(ChatRole::Assistant, text)]);
        assert!(doc.ends_with(&format!("{text}\n````\n")), "{doc}");

        let closed = "````md\n```js\nx\n```\n````";
        let doc = markdown(vec![message(ChatRole::Assistant, closed)]);
        assert!(doc.ends_with(&format!("{closed}\n")), "{doc}");
        // A tilde line never closes a backtick fence.
        assert!(open_fence("```\n~~~").is_some());
        // Inline backticks are not a fence.
        assert!(open_fence("``` a ` b").is_none());
    }

    #[test]
    fn markdown_skips_empty_messages() {
        let doc = markdown(vec![
            message(ChatRole::User, "Hi"),
            message(ChatRole::Assistant, "  \n"),
            message(ChatRole::System, "hidden"),
        ]);
        assert_eq!(doc.matches("## ").count(), 1);
        assert!(!doc.contains("hidden"));
    }

    #[test]
    fn conversation_markdown_reads_saved_chat() {
        let (storage, _dir) = temp_storage();
        let id = storage.create_conversation(Some("Saved"), "").unwrap().id;
        let doc = storage.conversation_markdown(id).unwrap();
        assert!(doc.expose().starts_with("# Saved\n\n20"));
        assert!(matches!(
            storage.conversation_markdown(Uuid::new_v4()),
            Err(AppError::NotFound(_))
        ));
    }
}
