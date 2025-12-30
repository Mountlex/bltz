use std::collections::HashMap;

use super::types::{EmailFlags, EmailHeader};

pub type ThreadId = String;

/// Email thread using indices into the emails array (avoids cloning).
/// Emails are sorted by date ascending within the thread.
#[derive(Debug, Clone)]
pub struct EmailThread {
    pub id: ThreadId,
    /// Indices into the AppState.emails array, sorted by date ascending
    pub email_indices: Vec<usize>,
    /// Pre-computed metadata (avoids repeated iteration)
    pub unread_count: usize,
    pub total_count: usize,
    pub latest_date: i64,
    pub has_attachments: bool,
    /// Index of the latest (most recent) email for quick access
    pub latest_idx: usize,
}

impl EmailThread {
    /// Check if thread has any unread emails
    pub fn has_unread(&self) -> bool {
        self.unread_count > 0
    }

    /// Get email at position within thread (requires emails slice)
    #[inline]
    pub fn email_at<'a>(&self, emails: &'a [EmailHeader], pos: usize) -> Option<&'a EmailHeader> {
        self.email_indices.get(pos).map(|&idx| &emails[idx])
    }

    /// Get the latest (most recent) email (requires emails slice)
    #[inline]
    pub fn latest<'a>(&self, emails: &'a [EmailHeader]) -> &'a EmailHeader {
        &emails[self.latest_idx]
    }

    /// Get the first email in the thread (requires emails slice)
    #[inline]
    #[allow(dead_code)]
    pub fn first<'a>(&self, emails: &'a [EmailHeader]) -> &'a EmailHeader {
        &emails[self.email_indices[0]]
    }

    /// Iterate over emails in this thread (requires emails slice)
    #[inline]
    pub fn emails<'a>(
        &self,
        all_emails: &'a [EmailHeader],
    ) -> impl Iterator<Item = &'a EmailHeader> + use<'a, '_> {
        self.email_indices.iter().map(move |&idx| &all_emails[idx])
    }

    /// Get number of emails in thread
    #[inline]
    pub fn len(&self) -> usize {
        self.email_indices.len()
    }
}

/// Group emails into threads using a hybrid algorithm:
/// 1. Message-ID based linking (in_reply_to/references)
/// 2. Subject-based fallback for emails without threading headers
///
/// Takes a slice reference to avoid cloning the entire vector at the call site.
pub fn group_into_threads(emails: &[EmailHeader]) -> Vec<EmailThread> {
    if emails.is_empty() {
        return Vec::new();
    }

    // Build message-id index
    let mut by_message_id: HashMap<String, usize> = HashMap::new();
    for (i, email) in emails.iter().enumerate() {
        if let Some(ref mid) = email.message_id {
            by_message_id.insert(mid.clone(), i);
        }
    }

    // Build parent links: find which email each email replies to
    let mut parent: Vec<Option<usize>> = vec![None; emails.len()];
    for (i, email) in emails.iter().enumerate() {
        // First try in_reply_to
        if let Some(ref reply_to) = email.in_reply_to
            && let Some(&parent_idx) = by_message_id.get(reply_to)
                && parent_idx != i {
                    parent[i] = Some(parent_idx);
                    continue;
                }
        // Fallback: try references (last one is most immediate parent)
        for ref_id in email.references.iter().rev() {
            if let Some(&parent_idx) = by_message_id.get(ref_id)
                && parent_idx != i {
                    parent[i] = Some(parent_idx);
                    break;
                }
        }
    }

    // Find roots using union-find with path compression
    let mut root: Vec<usize> = (0..emails.len()).collect();
    for (i, r) in root.iter_mut().enumerate() {
        *r = find_root_compressed(&mut parent, i);
    }

    // Group by root, with subject-based fallback for orphans
    let mut thread_groups: HashMap<ThreadId, Vec<usize>> = HashMap::new();
    let mut subject_groups: HashMap<String, Vec<usize>> = HashMap::new();

    for (i, email) in emails.iter().enumerate() {
        if root[i] == i && parent[i].is_none() {
            // This is a root with no parent - check if we can group by subject
            let normalized = normalize_subject(&email.subject);
            subject_groups.entry(normalized).or_default().push(i);
        } else {
            // Part of a message-id based thread
            let root_email = &emails[root[i]];
            let thread_id = root_email
                .message_id
                .clone()
                .unwrap_or_else(|| format!("uid:{}", root_email.uid));
            thread_groups.entry(thread_id).or_default().push(i);
        }
    }

    // Merge subject groups that have more than one email
    for (subject, indices) in subject_groups {
        if indices.len() > 1 {
            // Multiple emails with same subject - treat as thread
            let thread_id = format!("subj:{}", subject);
            thread_groups.entry(thread_id).or_default().extend(indices);
        } else {
            // Single email - keep as its own thread
            let i = indices[0];
            let email = &emails[i];
            let thread_id = email
                .message_id
                .clone()
                .unwrap_or_else(|| format!("uid:{}", email.uid));
            thread_groups.entry(thread_id).or_default().push(i);
        }
    }

    // Build EmailThread objects using indices (no cloning!)
    let mut threads: Vec<EmailThread> = thread_groups
        .into_iter()
        .map(|(id, mut indices)| {
            // Sort indices by date ascending within thread
            indices.sort_by_key(|&i| emails[i].date);

            // Compute metadata without cloning
            let unread_count = indices
                .iter()
                .filter(|&&i| !emails[i].flags.contains(EmailFlags::SEEN))
                .count();

            let has_attachments = indices.iter().any(|&i| emails[i].has_attachments);
            // Safety: indices is always non-empty because thread_groups only contains
            // entries that were populated via push() or extend() with non-empty data.
            // Using unwrap_or with first index as fallback for defensive programming.
            let latest_idx = indices.last().copied().unwrap_or(indices[0]);
            let latest_date = emails[latest_idx].date;
            let total_count = indices.len();

            EmailThread {
                id,
                email_indices: indices,
                unread_count,
                total_count,
                latest_date,
                has_attachments,
                latest_idx,
            }
        })
        .collect();

    // Sort threads by latest date descending
    threads.sort_by(|a, b| b.latest_date.cmp(&a.latest_date));

    threads
}

/// Find root with path compression for O(log n) amortized lookups
fn find_root_compressed(parent: &mut [Option<usize>], mut i: usize) -> usize {
    // First pass: find the root
    let mut current = i;
    while let Some(p) = parent[current] {
        current = p;
    }
    let root = current;

    // Second pass: path compression - point all non-root nodes directly to root
    while let Some(p) = parent[i] {
        parent[i] = Some(root);
        i = p;
    }

    root
}

/// Normalize subject for grouping: strip Re:/Fwd:/Fw: prefixes and lowercase
fn normalize_subject(subject: &str) -> String {
    let mut s = subject.trim();
    loop {
        let lower = s.to_lowercase();
        if lower.starts_with("re:") {
            s = s[3..].trim_start();
        } else if lower.starts_with("fwd:") {
            s = s[4..].trim_start();
        } else if lower.starts_with("fw:") {
            s = s[3..].trim_start();
        } else if lower.starts_with("aw:") {
            // German "Antwort"
            s = s[3..].trim_start();
        } else if lower.starts_with("sv:") {
            // Swedish "Svar"
            s = s[3..].trim_start();
        } else if lower.starts_with("re[") {
            // Handle Re[2]: style
            if let Some(end) = s.find("]:") {
                s = s[end + 2..].trim_start();
            } else {
                break;
            }
        } else {
            break;
        }
    }
    s.to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_email(
        uid: u32,
        subject: &str,
        message_id: Option<&str>,
        in_reply_to: Option<&str>,
        date: i64,
    ) -> EmailHeader {
        EmailHeader {
            uid,
            message_id: message_id.map(|s| s.to_string()),
            subject: subject.to_string(),
            from_addr: "test@example.com".to_string(),
            from_name: None,
            to_addr: None,
            date,
            flags: EmailFlags::empty(),
            has_attachments: false,
            preview: None,
            body_cached: false,
            in_reply_to: in_reply_to.map(|s| s.to_string()),
            references: Vec::new(),
        }
    }

    #[test]
    fn test_normalize_subject() {
        assert_eq!(normalize_subject("Hello"), "hello");
        assert_eq!(normalize_subject("Re: Hello"), "hello");
        assert_eq!(normalize_subject("RE: Hello"), "hello");
        assert_eq!(normalize_subject("Fwd: Hello"), "hello");
        assert_eq!(normalize_subject("Re: Re: Hello"), "hello");
        assert_eq!(normalize_subject("Re: Fwd: Hello"), "hello");
        assert_eq!(normalize_subject("  Re:  Hello  "), "hello");
    }

    #[test]
    fn test_group_by_message_id() {
        let emails = vec![
            make_email(1, "Hello", Some("msg1@test"), None, 1000),
            make_email(2, "Re: Hello", Some("msg2@test"), Some("msg1@test"), 2000),
            make_email(3, "Re: Hello", Some("msg3@test"), Some("msg2@test"), 3000),
        ];

        let threads = group_into_threads(&emails);
        assert_eq!(threads.len(), 1);
        assert_eq!(threads[0].total_count, 3);
        // Access via email_at helper
        assert_eq!(threads[0].email_at(&emails, 0).unwrap().uid, 1); // Oldest first
        assert_eq!(threads[0].email_at(&emails, 2).unwrap().uid, 3); // Newest last
    }

    #[test]
    fn test_group_by_subject() {
        let emails = vec![
            make_email(1, "Project Update", None, None, 1000),
            make_email(2, "Re: Project Update", None, None, 2000),
        ];

        let threads = group_into_threads(&emails);
        assert_eq!(threads.len(), 1);
        assert_eq!(threads[0].total_count, 2);
    }

    #[test]
    fn test_separate_threads() {
        let emails = vec![
            make_email(1, "Topic A", Some("a1@test"), None, 1000),
            make_email(2, "Topic B", Some("b1@test"), None, 2000),
        ];

        let threads = group_into_threads(&emails);
        assert_eq!(threads.len(), 2);
    }
}
