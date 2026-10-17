//! The server's log: one `tracing` subscriber that writes each line to
//! standard output, as before (the terminal, `ferryd.log` under `ferryd start`,
//! the service manager under `ferryd run`), and keeps it in the in-memory
//! [`ServerLogs`] that Server → Server logs reads (DESIGN.md §22). `RUST_LOG`
//! (`info` by default) filters the lines once, for both; the in-memory log
//! then leaves out the `debug` and `trace` lines of the libraries.

use std::collections::BTreeMap;
use std::fmt;

use chrono::Utc;
use ferry_core::server_logs::{ServerLogLevel, ServerLogLine, ServerLogs};
use tracing::field::{Field, Visit};
use tracing::level_filters::LevelFilter;
use tracing::{Event, Level, Metadata, Subscriber, span};
use tracing_subscriber::filter::filter_fn;
use tracing_subscriber::fmt::MakeWriter;
use tracing_subscriber::layer::{Context, SubscriberExt};
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, Layer};

/// Install the server's subscriber (once, when it starts) and return the log
/// it keeps in memory.
pub fn init(colors: bool) -> ServerLogs {
    let logs = ServerLogs::default();
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let filter = quiet_docker_client(filter, std::env::var("RUST_LOG").ok().as_deref());
    subscriber(filter, colors, std::io::stdout, logs.clone()).init();
    logs
}

/// At `debug`, bollard (the Docker client) logs each answer of Docker whole:
/// the variables of every service and the passwords of the datastores among
/// them. When `RUST_LOG` asks for `debug` or `trace`, bollard stays at `info`,
/// unless `RUST_LOG` names it.
fn quiet_docker_client(filter: EnvFilter, rust_log: Option<&str>) -> EnvFilter {
    let verbose = filter.max_level_hint().is_some_and(|level| level > LevelFilter::INFO);
    if !verbose || rust_log.is_some_and(|directives| directives.contains("bollard")) {
        return filter;
    }
    match "bollard=info".parse() {
        Ok(directive) => filter.add_directive(directive),
        Err(_) => filter,
    }
}

fn subscriber<W>(filter: EnvFilter, colors: bool, writer: W, logs: ServerLogs) -> impl Subscriber + Send + Sync
where
    W: for<'w> MakeWriter<'w> + Send + Sync + 'static,
{
    tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer().with_target(false).with_ansi(colors).with_writer(writer))
        .with(Keep(logs).with_filter(filter_fn(kept)))
}

/// What the in-memory log keeps of the lines `RUST_LOG` lets through: every
/// line at `info` and above, and the `debug` and `trace` lines of Ferry's own
/// crates. Those of the libraries stay in the output only: h2 logs each frame
/// it sends at `debug` (hyper each write at `trace`), so the stream of Server
/// → Server logs would log its own frames, without end.
fn kept(metadata: &Metadata<'_>) -> bool {
    *metadata.level() <= Level::INFO || metadata.target().starts_with("ferry")
}

/// Keeps each line in the server's in-memory log.
struct Keep(ServerLogs);

/// The values of a span, for the lines logged inside it.
struct SpanValues(BTreeMap<String, String>);

impl<S> Layer<S> for Keep
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_new_span(&self, attrs: &span::Attributes<'_>, id: &span::Id, ctx: Context<'_, S>) {
        let mut values = Values::default();
        attrs.record(&mut values);
        if let Some(span) = ctx.span(id) {
            span.extensions_mut().insert(SpanValues(values.fields));
        }
    }

    fn on_record(&self, id: &span::Id, record: &span::Record<'_>, ctx: Context<'_, S>) {
        let Some(span) = ctx.span(id) else { return };
        let mut extensions = span.extensions_mut();
        if let Some(SpanValues(fields)) = extensions.get_mut::<SpanValues>() {
            let mut values = Values::default();
            record.record(&mut values);
            fields.extend(values.fields);
        }
    }

    fn on_event(&self, event: &Event<'_>, ctx: Context<'_, S>) {
        let mut values = Values::default();
        event.record(&mut values);
        // The values of the spans around it, the outermost first, then its
        // own (which win), like the `http{method=… path=…}:` prefix of the
        // printed line.
        let mut fields = BTreeMap::new();
        if let Some(scope) = ctx.event_scope(event) {
            for span in scope.from_root() {
                if let Some(SpanValues(span_fields)) = span.extensions().get::<SpanValues>() {
                    fields.extend(span_fields.iter().map(|(k, v)| (k.clone(), v.clone())));
                }
            }
        }
        fields.extend(values.fields);
        let metadata = event.metadata();
        self.0.push(ServerLogLine {
            ts: Utc::now(),
            level: level(metadata.level()),
            target: values.log_target.unwrap_or_else(|| metadata.target().to_string()),
            message: values.message,
            fields,
        });
    }
}

fn level(level: &Level) -> ServerLogLevel {
    match *level {
        Level::ERROR => ServerLogLevel::Error,
        Level::WARN => ServerLogLevel::Warn,
        Level::INFO => ServerLogLevel::Info,
        _ => ServerLogLevel::Debug,
    }
}

/// The message and the other fields of an event or a span, as text.
#[derive(Default)]
struct Values {
    message: String,
    fields: BTreeMap<String, String>,
    /// The target of a line of the `log` crate (rustls, instant-acme), which
    /// `tracing` reports under its own `log` target.
    log_target: Option<String>,
}

impl Values {
    fn put(&mut self, field: &Field, value: String) {
        match field.name() {
            "message" => self.message = value,
            "log.target" => self.log_target = Some(value),
            // The module, file and line of a `log` crate line.
            name if name.starts_with("log.") => {}
            name => {
                self.fields.insert(name.to_string(), value);
            }
        }
    }
}

impl Visit for Values {
    fn record_str(&mut self, field: &Field, value: &str) {
        self.put(field, value.to_string());
    }

    fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
        self.put(field, format!("{value:?}"));
    }
}

#[cfg(test)]
mod tests {
    use std::io;
    use std::sync::{Arc, Mutex};

    use super::*;

    /// What the server would print to its standard output.
    #[derive(Clone, Default)]
    struct Output(Arc<Mutex<Vec<u8>>>);

    impl Output {
        fn text(&self) -> String {
            String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
        }
    }

    impl io::Write for Output {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    impl<'a> MakeWriter<'a> for Output {
        type Writer = Output;

        fn make_writer(&'a self) -> Self::Writer {
            self.clone()
        }
    }

    /// An output that takes nothing, like a closed pipe.
    struct Closed;

    impl io::Write for Closed {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(io::Error::from(io::ErrorKind::BrokenPipe))
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    fn log_with<W>(filter: &str, writer: W, f: impl FnOnce()) -> ServerLogs
    where
        W: for<'w> MakeWriter<'w> + Send + Sync + 'static,
    {
        let logs = ServerLogs::default();
        tracing::subscriber::with_default(subscriber(EnvFilter::new(filter), false, writer, logs.clone()), f);
        logs
    }

    #[test]
    fn keeps_the_lines_it_prints() {
        let output = Output::default();
        let logs = log_with("info", output.clone(), || {
            tracing::info!(service = "api", instances = 2, "deploy live");
            tracing::debug!("a detail");
            tracing::warn!(host = "app.example.com", error = %"rate limited", "no certificate");
        });
        let lines = logs.lines(None, ServerLogLevel::Debug);
        assert_eq!(lines.len(), 2, "info and above by default: {lines:?}");
        assert_eq!(lines[0].level, ServerLogLevel::Info);
        assert_eq!(lines[0].message, "deploy live");
        assert_eq!(lines[0].target, "ferryd::logging::tests");
        assert_eq!(lines[0].fields["service"], "api");
        assert_eq!(lines[0].fields["instances"], "2");
        assert_eq!(lines[1].level, ServerLogLevel::Warn);
        assert_eq!(lines[1].message, "no certificate");
        assert_eq!(lines[1].fields["error"], "rate limited");

        // The output is what it was: the same lines, no target, no colors.
        let text = output.text();
        assert_eq!(text.lines().count(), 2, "{text}");
        assert!(text.contains(r#"INFO deploy live service="api" instances=2"#), "{text}");
        assert!(text.contains(r#"WARN no certificate host="app.example.com" error=rate limited"#), "{text}");
        assert!(!text.contains("a detail") && !text.contains("ferryd::") && !text.contains('\u{1b}'), "{text}");
    }

    #[test]
    fn debug_lines_only_when_rust_log_asks() {
        let logs = log_with("debug", Output::default(), || {
            tracing::debug!("a detail");
            tracing::trace!("finer");
        });
        let lines = logs.lines(None, ServerLogLevel::Debug);
        assert_eq!(lines.len(), 1);
        assert_eq!((lines[0].level, lines[0].message.as_str()), (ServerLogLevel::Debug, "a detail"));

        let logs = log_with("ferryd=trace", Output::default(), || tracing::trace!("finer"));
        assert_eq!(logs.lines(None, ServerLogLevel::Debug)[0].level, ServerLogLevel::Debug, "trace counts as debug");
    }

    #[test]
    fn docker_answers_stay_out_unless_rust_log_names_bollard() {
        let printed = |rust_log: &str| {
            let output = Output::default();
            let filter = quiet_docker_client(EnvFilter::new(rust_log), Some(rust_log));
            tracing::subscriber::with_default(subscriber(filter, false, output.clone(), ServerLogs::default()), || {
                tracing::debug!(target: "bollard::docker", r#"Decoded into string: {{"Env":["PASSWORD=hunter2"]}}"#);
                tracing::info!(target: "bollard::docker", "an info line of bollard");
                tracing::debug!(target: "ferry_docker", "container started");
            });
            output.text()
        };
        let text = printed("debug");
        assert!(!text.contains("hunter2"), "{text}");
        assert!(text.contains("an info line of bollard") && text.contains("container started"), "{text}");
        assert!(printed("debug,bollard=debug").contains("hunter2"));
        // Below `debug`, nothing changes: bollard is not raised to `info`.
        assert_eq!(printed("warn"), "");
    }

    /// h2 logs each frame it sends at `debug`: kept in memory, the stream of
    /// the dashboard would log its own frames, without end.
    #[test]
    fn only_ferrys_own_debug_lines_are_kept() {
        let output = Output::default();
        let logs = log_with("trace", output.clone(), || {
            tracing::debug!(target: "h2::codec::framed_write", frame = "Data { stream_id: 1 }", "send");
            tracing::trace!(target: "hyper::proto::h1::io", "flushed 120 bytes");
            tracing::debug!(target: "ferry_proxy::handler", "proxy: request");
            tracing::trace!(target: "ferry_engine", "a detail");
            tracing::warn!(target: "h2::proto", "connection error");
        });
        let kept: Vec<_> = logs.lines(None, ServerLogLevel::Debug).into_iter().map(|l| l.target).collect();
        assert_eq!(kept, ["ferry_proxy::handler", "ferry_engine", "h2::proto"]);
        // The output still has them all.
        let text = output.text();
        assert!(text.contains("send frame=") && text.contains("flushed 120 bytes"), "{text}");
    }

    #[test]
    fn lines_carry_the_values_of_their_spans() {
        let logs = log_with("info", Output::default(), || {
            let span =
                tracing::info_span!("http", method = "POST", path = "/api/v1/services", status = tracing::field::Empty);
            let _entered = span.enter();
            span.record("status", 500);
            tracing::warn!(path = "/override", "request failed");
        });
        let line = &logs.lines(None, ServerLogLevel::Debug)[0];
        assert_eq!(line.message, "request failed");
        assert_eq!(line.fields["method"], "POST");
        assert_eq!(line.fields["status"], "500");
        assert_eq!(line.fields["path"], "/override", "the line's own value wins");
    }

    #[test]
    fn a_log_crate_line_keeps_its_target() {
        let logs = log_with("info", Output::default(), || {
            tracing::info!(log.target = "rustls::client", log.module_path = "rustls::client", log.line = 42, "resumed");
        });
        let line = &logs.lines(None, ServerLogLevel::Debug)[0];
        assert_eq!(line.target, "rustls::client");
        assert!(line.fields.is_empty(), "{:?}", line.fields);
    }

    /// `ferryd run` under a service manager writes no log file: what the
    /// dashboard reads never depends on where the output goes.
    #[test]
    fn keeps_the_lines_whatever_the_output() {
        let logs = log_with("info", || Closed, || tracing::warn!("disk almost full"));
        assert_eq!(logs.lines(None, ServerLogLevel::Debug)[0].message, "disk almost full");
    }
}
