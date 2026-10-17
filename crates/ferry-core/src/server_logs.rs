//! The server's own log, kept in memory for Server → Server logs
//! (`GET /api/v1/server/logs`, DESIGN.md §22).
//!
//! ferryd's `tracing` subscriber writes each line it logs to its output (the
//! terminal, `ferryd.log` or the service manager) and into a [`ServerLogs`]:
//! the last [`SERVER_LOG_LINES`] lines, in memory only, so the log can be read
//! the same way without a log file, and nothing more is written to disk.
//!
//! Logging never waits for a reader: each follower has a queue of
//! [`FOLLOWER_QUEUE`] lines, a line that does not fit in it is skipped for
//! that follower, and the follower is told how many it missed
//! ([`ServerLogEvent::Skipped`]) before the next line it gets.

use std::collections::{BTreeMap, VecDeque};
use std::pin::Pin;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use chrono::{DateTime, Utc};
use futures::{Stream, StreamExt};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tokio::sync::mpsc::error::TrySendError;

/// Lines the server keeps.
pub const SERVER_LOG_LINES: usize = 2000;

/// A line longer than this, its message and fields together, is cut.
pub const MAX_LINE_BYTES: usize = 8 * 1024;

/// Lines a follower can fall behind before it misses some.
pub const FOLLOWER_QUEUE: usize = 1024;

/// How important a line is, the least important first (`debug`, the
/// default, is every level). `TRACE` lines (only with `RUST_LOG=trace`) count
/// as `debug`.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, utoipa::ToSchema,
)]
#[serde(rename_all = "lowercase")]
pub enum ServerLogLevel {
    #[default]
    Debug,
    Info,
    Warn,
    Error,
}

/// One line of the server's log: what ferryd prints, as values.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, utoipa::ToSchema)]
pub struct ServerLogLine {
    /// When ferryd logged it.
    pub ts: DateTime<Utc>,
    pub level: ServerLogLevel,
    /// The module that logged it, e.g. `ferry_engine::deploy`.
    pub target: String,
    pub message: String,
    /// The other values of the line (`service`, `deploy`, the `path` of a
    /// request…), as text.
    #[serde(default)]
    pub fields: BTreeMap<String, String>,
}

impl ServerLogLine {
    pub fn new(level: ServerLogLevel, target: impl Into<String>, message: impl Into<String>) -> Self {
        ServerLogLine { ts: Utc::now(), level, target: target.into(), message: message.into(), fields: BTreeMap::new() }
    }

    pub fn with_field(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.fields.insert(key.into(), value.into());
        self
    }

    /// Cut the line to [`MAX_LINE_BYTES`]: the message first, then the fields
    /// in order (key and value). A value that is cut ends with `…`; the
    /// fields that no longer fit are left out.
    fn cut(&mut self) {
        let mut budget = MAX_LINE_BYTES;
        shorten(&mut self.message, budget);
        budget -= self.message.len();
        self.fields.retain(|key, value| {
            if key.len() >= budget {
                budget = 0;
                return false;
            }
            shorten(value, budget - key.len());
            budget -= key.len() + value.len();
            true
        });
    }
}

/// Cut `s` to at most `max` bytes, on a character boundary, ending with `…`
/// when it was cut.
fn shorten(s: &mut String, max: usize) {
    const ELLIPSIS: char = '…';
    if s.len() <= max {
        return;
    }
    let mut end = max.saturating_sub(ELLIPSIS.len_utf8());
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    s.truncate(end);
    if max >= ELLIPSIS.len_utf8() {
        s.push(ELLIPSIS);
    }
}

/// What a reader of the log gets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServerLogEvent {
    Line(ServerLogLine),
    /// This many lines were left out here: the reader fell behind.
    Skipped(u64),
}

/// A boxed stream of the server's log.
pub type ServerLogStream = Pin<Box<dyn Stream<Item = ServerLogEvent> + Send + 'static>>;

/// Which lines of the log to read.
#[derive(Debug, Clone, Copy, Default)]
pub struct ServerLogQuery {
    /// The most recent lines to start with. `None`: all the kept lines.
    pub tail: Option<usize>,
    /// Then send the new lines as they are logged.
    pub follow: bool,
    /// Only the lines at this level or above.
    pub level: ServerLogLevel,
}

/// The last lines of the server's log, and the readers that follow it.
/// Cheap to clone: every clone is the same log.
#[derive(Clone)]
pub struct ServerLogs {
    inner: Arc<Mutex<Inner>>,
}

struct Inner {
    capacity: usize,
    lines: VecDeque<ServerLogLine>,
    followers: Vec<Follower>,
}

struct Follower {
    level: ServerLogLevel,
    queue: mpsc::Sender<Queued>,
    /// Lines that did not fit in the queue since the last one that did.
    skipped: u64,
}

/// A line in a follower's queue, with the lines skipped just before it.
struct Queued {
    skipped: u64,
    line: ServerLogLine,
}

impl Default for ServerLogs {
    fn default() -> Self {
        ServerLogs::new(SERVER_LOG_LINES)
    }
}

impl std::fmt::Debug for ServerLogs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let inner = self.lock();
        f.debug_struct("ServerLogs")
            .field("capacity", &inner.capacity)
            .field("lines", &inner.lines.len())
            .field("followers", &inner.followers.len())
            .finish()
    }
}

impl ServerLogs {
    /// An empty log that keeps the last `capacity` lines.
    pub fn new(capacity: usize) -> Self {
        let inner = Inner { capacity, lines: VecDeque::with_capacity(capacity), followers: Vec::new() };
        ServerLogs { inner: Arc::new(Mutex::new(inner)) }
    }

    /// Add a line (cut to [`MAX_LINE_BYTES`]): the oldest one goes when the
    /// log is full. Never waits: a follower whose queue is full misses it.
    pub fn push(&self, mut line: ServerLogLine) {
        line.cut();
        let mut inner = self.lock();
        inner.followers.retain_mut(|f| {
            if line.level < f.level {
                return !f.queue.is_closed();
            }
            match f.queue.try_send(Queued { skipped: f.skipped, line: line.clone() }) {
                Ok(()) => {
                    f.skipped = 0;
                    true
                }
                Err(TrySendError::Full(_)) => {
                    f.skipped += 1;
                    true
                }
                Err(TrySendError::Closed(_)) => false,
            }
        });
        if inner.capacity == 0 {
            return;
        }
        if inner.lines.len() == inner.capacity {
            inner.lines.pop_front();
        }
        inner.lines.push_back(line);
    }

    /// The last `tail` lines at `level` or above (all of them for `None`),
    /// the oldest first.
    pub fn lines(&self, tail: Option<usize>, level: ServerLogLevel) -> Vec<ServerLogLine> {
        recent(&self.lock(), tail, level)
    }

    /// The lines `query` asks for, then, with `follow`, each new one as it is
    /// logged, until the stream is dropped. A follower that falls more than
    /// [`FOLLOWER_QUEUE`] lines behind gets [`ServerLogEvent::Skipped`] where
    /// it missed lines.
    pub fn stream(&self, query: ServerLogQuery) -> ServerLogStream {
        let mut inner = self.lock();
        // The recent lines and the follower under one lock: no line is
        // missed or sent twice between them.
        let backlog =
            futures::stream::iter(recent(&inner, query.tail, query.level).into_iter().map(ServerLogEvent::Line));
        if !query.follow {
            return Box::pin(backlog);
        }
        let (queue, rx) = mpsc::channel(FOLLOWER_QUEUE);
        inner.followers.push(Follower { level: query.level, queue, skipped: 0 });
        drop(inner);
        let live = futures::stream::unfold((rx, None), |(mut rx, pending): (mpsc::Receiver<Queued>, _)| async move {
            if let Some(line) = pending {
                return Some((ServerLogEvent::Line(line), (rx, None)));
            }
            let Queued { skipped, line } = rx.recv().await?;
            if skipped > 0 {
                Some((ServerLogEvent::Skipped(skipped), (rx, Some(line))))
            } else {
                Some((ServerLogEvent::Line(line), (rx, None)))
            }
        });
        Box::pin(backlog.chain(live))
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        // Nothing panics while holding the lock; and logging must not panic.
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

fn recent(inner: &Inner, tail: Option<usize>, level: ServerLogLevel) -> Vec<ServerLogLine> {
    let mut lines: Vec<ServerLogLine> =
        inner.lines.iter().rev().filter(|l| l.level >= level).take(tail.unwrap_or(usize::MAX)).cloned().collect();
    lines.reverse();
    lines
}

#[cfg(test)]
mod tests {
    use futures::FutureExt;

    use super::*;

    fn line(level: ServerLogLevel, message: &str) -> ServerLogLine {
        ServerLogLine::new(level, "ferry_test", message)
    }

    fn messages(lines: &[ServerLogLine]) -> Vec<&str> {
        lines.iter().map(|l| l.message.as_str()).collect()
    }

    /// What `stream` has ready now, without waiting.
    fn ready(stream: &mut ServerLogStream) -> Vec<ServerLogEvent> {
        std::iter::from_fn(|| stream.next().now_or_never().flatten()).collect()
    }

    fn event_text(events: &[ServerLogEvent]) -> Vec<String> {
        events
            .iter()
            .map(|e| match e {
                ServerLogEvent::Line(l) => l.message.clone(),
                ServerLogEvent::Skipped(n) => format!("skipped {n}"),
            })
            .collect()
    }

    #[test]
    fn keeps_the_last_lines() {
        let logs = ServerLogs::new(3);
        assert!(logs.lines(None, ServerLogLevel::Debug).is_empty());
        for i in 1..=5 {
            logs.push(line(ServerLogLevel::Info, &format!("line {i}")));
        }
        assert_eq!(messages(&logs.lines(None, ServerLogLevel::Debug)), ["line 3", "line 4", "line 5"]);
        assert_eq!(messages(&logs.lines(Some(2), ServerLogLevel::Debug)), ["line 4", "line 5"]);
        assert!(logs.lines(Some(0), ServerLogLevel::Debug).is_empty());

        // The default keeps 2,000 lines.
        let logs = ServerLogs::default();
        for i in 0..2500 {
            logs.push(line(ServerLogLevel::Info, &i.to_string()));
        }
        let kept = logs.lines(None, ServerLogLevel::Debug);
        assert_eq!(kept.len(), SERVER_LOG_LINES);
        assert_eq!(kept[0].message, "500");
    }

    #[test]
    fn filters_by_level() {
        let logs = ServerLogs::new(10);
        logs.push(line(ServerLogLevel::Debug, "detail"));
        logs.push(line(ServerLogLevel::Info, "listening"));
        logs.push(line(ServerLogLevel::Warn, "no certificate"));
        logs.push(line(ServerLogLevel::Error, "proxy stopped"));
        logs.push(line(ServerLogLevel::Info, "deploy live"));
        assert_eq!(logs.lines(None, ServerLogLevel::Debug).len(), 5);
        assert_eq!(
            messages(&logs.lines(None, ServerLogLevel::Info)),
            ["listening", "no certificate", "proxy stopped", "deploy live"]
        );
        assert_eq!(messages(&logs.lines(None, ServerLogLevel::Warn)), ["no certificate", "proxy stopped"]);
        assert_eq!(messages(&logs.lines(None, ServerLogLevel::Error)), ["proxy stopped"]);
        // The tail counts the lines that match.
        assert_eq!(messages(&logs.lines(Some(1), ServerLogLevel::Warn)), ["proxy stopped"]);
    }

    #[test]
    fn cuts_long_lines() {
        let logs = ServerLogs::new(10);
        logs.push(line(ServerLogLevel::Info, &"x".repeat(MAX_LINE_BYTES)));
        logs.push(line(ServerLogLevel::Info, &"é".repeat(MAX_LINE_BYTES)));
        logs.push(
            line(ServerLogLevel::Warn, &"m".repeat(MAX_LINE_BYTES - 100))
                .with_field("a", "v".repeat(500))
                .with_field("b", "dropped"),
        );
        logs.push(line(ServerLogLevel::Info, "short").with_field("service", "api"));
        let lines = logs.lines(None, ServerLogLevel::Debug);

        // Exactly the limit: kept whole.
        assert_eq!(lines[0].message.len(), MAX_LINE_BYTES);
        // Past it: cut on a character boundary, marked with an ellipsis.
        assert!(lines[1].message.len() <= MAX_LINE_BYTES, "{}", lines[1].message.len());
        assert!(lines[1].message.ends_with("é…"));
        // The message first, then the fields until the limit.
        let third = &lines[2];
        assert_eq!(third.message.len(), MAX_LINE_BYTES - 100);
        assert_eq!(third.fields.len(), 1, "{:?}", third.fields.keys());
        let a = &third.fields["a"];
        assert!(a.ends_with('…'));
        assert!(third.message.len() + "a".len() + a.len() <= MAX_LINE_BYTES);
        // A short line is untouched.
        assert_eq!(lines[3], line(ServerLogLevel::Info, "short").with_field("service", "api").with_ts(lines[3].ts));
    }

    #[tokio::test]
    async fn streams_the_tail_then_follows() {
        let logs = ServerLogs::new(10);
        logs.push(line(ServerLogLevel::Info, "old"));
        logs.push(line(ServerLogLevel::Warn, "recent warning"));
        logs.push(line(ServerLogLevel::Info, "recent"));

        // Without follow: the tail, then the end.
        let query = ServerLogQuery { tail: Some(2), follow: false, level: ServerLogLevel::Debug };
        let events: Vec<_> = logs.stream(query).collect().await;
        assert_eq!(event_text(&events), ["recent warning", "recent"]);

        // With follow and a level: the matching tail, then the matching new lines.
        let mut warnings = logs.stream(ServerLogQuery { tail: None, follow: true, level: ServerLogLevel::Warn });
        let mut all = logs.stream(ServerLogQuery { tail: Some(0), follow: true, level: ServerLogLevel::Debug });
        assert_eq!(event_text(&ready(&mut warnings)), ["recent warning"]);
        assert!(ready(&mut all).is_empty());
        logs.push(line(ServerLogLevel::Debug, "detail"));
        logs.push(line(ServerLogLevel::Error, "failed"));
        assert_eq!(event_text(&ready(&mut warnings)), ["failed"]);
        assert_eq!(event_text(&ready(&mut all)), ["detail", "failed"]);
        // It waits for the next line: it never ends on its own.
        assert!(warnings.next().now_or_never().is_none());
    }

    #[test]
    fn a_slow_follower_never_blocks_logging() {
        let logs = ServerLogs::new(SERVER_LOG_LINES);
        // Never read until the end: its queue fills up.
        let mut slow = logs.stream(ServerLogQuery { tail: Some(0), follow: true, level: ServerLogLevel::Debug });
        let mut fast = logs.stream(ServerLogQuery { tail: Some(0), follow: true, level: ServerLogLevel::Debug });
        // Only warnings: the info lines it does not want are not "skipped".
        let mut warnings = logs.stream(ServerLogQuery { tail: Some(0), follow: true, level: ServerLogLevel::Warn });

        let total = FOLLOWER_QUEUE + 500;
        let mut fast_got = Vec::new();
        for i in 0..total {
            // `push` returns at once: nothing here waits for `slow`.
            logs.push(line(ServerLogLevel::Info, &i.to_string()));
            fast_got.extend(ready(&mut fast));
        }
        assert_eq!(fast_got.len(), total, "a reader that keeps up misses nothing");
        assert!(fast_got.iter().all(|e| matches!(e, ServerLogEvent::Line(_))));
        assert_eq!(logs.lines(None, ServerLogLevel::Debug).len(), SERVER_LOG_LINES.min(total));

        // The slow one: a full queue, then where the gap is and how big.
        let queued = ready(&mut slow);
        assert_eq!(queued.len(), FOLLOWER_QUEUE);
        logs.push(line(ServerLogLevel::Warn, "after the gap"));
        assert_eq!(event_text(&ready(&mut slow)), ["skipped 500", "after the gap"]);
        assert_eq!(event_text(&ready(&mut warnings)), ["after the gap"]);
    }

    #[test]
    fn a_closed_follower_is_forgotten() {
        let logs = ServerLogs::new(10);
        let a = logs.stream(ServerLogQuery { tail: None, follow: true, level: ServerLogLevel::Error });
        let _b = logs.stream(ServerLogQuery { tail: None, follow: true, level: ServerLogLevel::Debug });
        assert_eq!(logs.lock().followers.len(), 2);
        drop(a);
        // Even a line below its level notices it is gone.
        logs.push(line(ServerLogLevel::Info, "x"));
        assert_eq!(logs.lock().followers.len(), 1);
        // Without follow, nothing is kept.
        drop(logs.stream(ServerLogQuery::default()));
        assert_eq!(logs.lock().followers.len(), 1);
    }

    #[test]
    fn serializes_as_the_api_sends_it() {
        let ts = "2026-10-17T10:00:00Z".parse().unwrap();
        let l = line(ServerLogLevel::Warn, "deploy failed").with_field("service", "api").with_ts(ts);
        assert_eq!(
            serde_json::to_value(&l).unwrap(),
            serde_json::json!({
                "ts": "2026-10-17T10:00:00Z",
                "level": "warn",
                "target": "ferry_test",
                "message": "deploy failed",
                "fields": {"service": "api"},
            })
        );
        let back: ServerLogLine = serde_json::from_value(serde_json::json!({
            "ts": "2026-10-17T10:00:00Z", "level": "error", "target": "t", "message": "m"
        }))
        .unwrap();
        assert!(back.fields.is_empty());
        assert!(ServerLogLevel::Debug < ServerLogLevel::Info && ServerLogLevel::Warn < ServerLogLevel::Error);
    }

    impl ServerLogLine {
        fn with_ts(mut self, ts: DateTime<Utc>) -> Self {
            self.ts = ts;
            self
        }
    }
}
