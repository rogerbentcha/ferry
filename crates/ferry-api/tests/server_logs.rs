//! `GET /api/v1/server/logs`: the server's own log, from the in-memory
//! `ServerLogs` ferryd keeps (DESIGN.md §22).

mod common;

use std::time::Duration;

use axum::body::Body;
use common::{TOKEN, TestApp};
use ferry_core::server_logs::{FOLLOWER_QUEUE, ServerLogLevel, ServerLogLine};
use futures::StreamExt;
use http::{Request, StatusCode};
use serde_json::{Value, json};
use tower::ServiceExt;

fn line(level: ServerLogLevel, message: &str) -> ServerLogLine {
    ServerLogLine::new(level, "ferry_engine", message)
}

/// The events of a finished SSE body: (event, data).
fn events(text: &str) -> Vec<(String, String)> {
    text.split("\n\n")
        .filter(|frame| !frame.is_empty() && !frame.starts_with(':'))
        .map(|frame| {
            let field = |name: &str| {
                frame.lines().find_map(|l| l.strip_prefix(name)).map(|v| v.trim_start().to_string()).unwrap_or_default()
            };
            (field("event:"), field("data:"))
        })
        .collect()
}

fn messages(events: &[(String, String)]) -> Vec<String> {
    events
        .iter()
        .filter(|(event, _)| event == "log")
        .map(|(_, data)| serde_json::from_str::<Value>(data).unwrap()["message"].as_str().unwrap().to_string())
        .collect()
}

/// A streaming response, read frame by frame.
struct Follow {
    body: axum::body::BodyDataStream,
    text: String,
}

impl Follow {
    async fn open(app: &TestApp, query: &str) -> Follow {
        let req = Request::get(format!("/api/v1/server/logs?{query}"))
            .header("authorization", format!("Bearer {TOKEN}"))
            .body(Body::empty())
            .unwrap();
        let resp = app.router.clone().oneshot(req).await.unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(resp.headers()["content-type"], "text/event-stream");
        Follow { body: resp.into_body().into_data_stream(), text: String::new() }
    }

    /// Read until `n` events came since the stream opened (keep-alives
    /// aside), and return them all.
    async fn events(&mut self, n: usize) -> Vec<(String, String)> {
        loop {
            let got = events(&self.text);
            if got.len() >= n {
                return got;
            }
            let chunk = tokio::time::timeout(Duration::from_secs(5), self.body.next())
                .await
                .unwrap_or_else(|_| panic!("no {n} events within 5s: {}", self.text))
                .unwrap_or_else(|| panic!("the stream ended: {}", self.text))
                .unwrap();
            self.text.push_str(std::str::from_utf8(&chunk).unwrap());
        }
    }
}

#[tokio::test]
async fn needs_authentication() {
    let app = TestApp::new().await;
    app.server_logs.push(line(ServerLogLevel::Warn, "a secret-looking line"));
    let r = app.send(Request::get("/api/v1/server/logs").body(Body::empty()).unwrap()).await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
    assert!(!r.text().contains("secret-looking"), "{}", r.text());
    let r = app.send(Request::get("/api/v1/server/logs?access_token=wrong").body(Body::empty()).unwrap()).await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
    // The token in the URL (`EventSource`) works, like for every other stream.
    let r =
        app.send(Request::get(format!("/api/v1/server/logs?access_token={TOKEN}")).body(Body::empty()).unwrap()).await;
    assert_eq!(r.status, StatusCode::OK);
}

#[tokio::test]
async fn sends_the_recent_lines_then_ends() {
    let app = TestApp::new().await;
    app.server_logs.push(line(ServerLogLevel::Debug, "a detail"));
    app.server_logs.push(line(ServerLogLevel::Info, "engine started"));
    app.server_logs
        .push(line(ServerLogLevel::Warn, "certificate request failed").with_field("host", "app.example.com"));
    app.server_logs.push(line(ServerLogLevel::Error, "proxy stopped"));

    // Everything kept, the oldest first, then `end`.
    let r = app.get("/api/v1/server/logs").await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text());
    assert_eq!(r.headers["content-type"], "text/event-stream");
    let all = events(&r.text());
    assert_eq!(messages(&all), ["a detail", "engine started", "certificate request failed", "proxy stopped"]);
    assert_eq!(all.last().unwrap(), &("end".to_string(), String::new()));
    let warning: Value = serde_json::from_str(&all[2].1).unwrap();
    assert_eq!(warning["level"], "warn");
    assert_eq!(warning["target"], "ferry_engine");
    assert_eq!(warning["fields"], json!({"host": "app.example.com"}));
    assert!(warning["ts"].as_str().is_some_and(|ts| ts.ends_with('Z')), "{warning}");

    // The last N.
    let r = app.get("/api/v1/server/logs?tail=2").await;
    assert_eq!(messages(&events(&r.text())), ["certificate request failed", "proxy stopped"]);
    let r = app.get("/api/v1/server/logs?tail=0").await;
    assert_eq!(events(&r.text()), [("end".to_string(), String::new())]);

    // A level and above; the tail counts the lines of that level.
    let r = app.get("/api/v1/server/logs?level=warn").await;
    assert_eq!(messages(&events(&r.text())), ["certificate request failed", "proxy stopped"]);
    let r = app.get("/api/v1/server/logs?level=info&tail=1").await;
    assert_eq!(messages(&events(&r.text())), ["proxy stopped"]);
    let r = app.get("/api/v1/server/logs?level=error").await;
    assert_eq!(messages(&events(&r.text())), ["proxy stopped"]);

    // Another level is a 400.
    let r = app.get("/api/v1/server/logs?level=trace").await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST, "{}", r.text());
    let r = app.get("/api/v1/server/logs?tail=-1").await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST, "{}", r.text());
}

#[tokio::test]
async fn follows_new_lines_at_the_level_asked_until_shutdown() {
    let app = TestApp::new().await;
    app.server_logs.push(line(ServerLogLevel::Info, "engine started"));
    app.server_logs.push(line(ServerLogLevel::Warn, "low disk"));

    let mut all = Follow::open(&app, "follow=true").await;
    let mut warnings = Follow::open(&app, "follow&level=warn&tail=0").await;
    assert_eq!(messages(&all.events(2).await), ["engine started", "low disk"]);

    app.server_logs.push(line(ServerLogLevel::Info, "deploy live"));
    app.server_logs.push(line(ServerLogLevel::Error, "proxy stopped"));
    assert_eq!(messages(&all.events(4).await)[2..], ["deploy live", "proxy stopped"]);
    let got = warnings.events(1).await;
    assert_eq!(messages(&got), ["proxy stopped"], "only the new warnings and errors");

    // No `end` while following: the stream ends when the server shuts down.
    app.shutdown.cancel();
    let rest = tokio::time::timeout(Duration::from_secs(5), async {
        let mut rest = Vec::new();
        while let Some(chunk) = all.body.next().await {
            rest.extend_from_slice(&chunk.unwrap());
        }
        rest
    })
    .await
    .expect("the stream must end on shutdown");
    assert!(!String::from_utf8_lossy(&rest).contains("event: end"));
}

/// A browser that reads slower than the server logs misses lines instead of
/// holding the server up, and is told how many it missed.
#[tokio::test]
async fn a_slow_reader_skips_lines() {
    let app = TestApp::new().await;
    let mut slow = Follow::open(&app, "follow=true&tail=0").await;
    // Nothing reads the response meanwhile: `push` must not wait for it.
    let total = FOLLOWER_QUEUE + 250;
    for i in 0..total {
        app.server_logs.push(line(ServerLogLevel::Info, &format!("line {i}")));
    }
    let queued = slow.events(FOLLOWER_QUEUE).await;
    assert_eq!(messages(&queued).last().unwrap(), &format!("line {}", FOLLOWER_QUEUE - 1));

    app.server_logs.push(line(ServerLogLevel::Warn, "after the gap"));
    let got = slow.events(FOLLOWER_QUEUE + 2).await;
    assert_eq!(got[FOLLOWER_QUEUE], ("skipped".to_string(), r#"{"count":250}"#.to_string()));
    assert_eq!(messages(&got[FOLLOWER_QUEUE + 1..]), ["after the gap"]);
    // The server kept every line.
    assert_eq!(app.server_logs.lines(Some(1), ServerLogLevel::Debug)[0].message, "after the gap");
}
