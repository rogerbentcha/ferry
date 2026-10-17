//! Every request is counted once, for its service or as unrouted, by status
//! class and latency (DESIGN.md §22).

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use bytes::Bytes;
use ferry_core::requests::RequestCounts;
use futures::{SinkExt, StreamExt};
use http::{Request, Response};
use http_body_util::{BodyExt, Full, StreamBody};
use hyper::body::Frame;
use tokio::io::AsyncWriteExt;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Notify;
use tokio_tungstenite::tungstenite::Message;

use super::support::*;
use crate::RequestTarget;
use crate::body::{self, BoxError};

fn counts(proxy: &TestProxy, target: RequestTarget) -> RequestCounts {
    let snapshot = proxy.routes.request_stats().snapshot();
    snapshot.into_iter().find(|t| t.target == target).map(|t| t.counts).unwrap_or_default()
}

fn service(id: &str) -> RequestTarget {
    RequestTarget::Service(id.into())
}

/// `(2xx, 3xx, 4xx, 5xx, in the latency)`
fn summary(c: &RequestCounts) -> (u64, u64, u64, u64, u64) {
    (c.status_2xx, c.status_3xx, c.status_4xx, c.status_5xx, c.latency.count())
}

/// An upstream answering `/<status>` with that status (200 otherwise), and
/// `/slow` with a 200 after 160 ms.
async fn status_upstream() -> Upstream {
    spawn_upstream(|req: Request<hyper::body::Incoming>| async move {
        let path = req.uri().path().trim_start_matches('/');
        if path == "slow" {
            tokio::time::sleep(Duration::from_millis(160)).await;
        }
        let status = path.parse().unwrap_or(200);
        Response::builder().status(status).body(body::full("ok")).unwrap()
    })
    .await
}

#[tokio::test]
async fn requests_are_counted_for_their_service_by_status_class() {
    let a = status_upstream().await;
    let b = status_upstream().await;
    let proxy = TestProxy::start().await;
    proxy.route("srv-a", &["app.test", "www.app.test"], &[a.addr]);
    proxy.route("srv-b", &["other.test"], &[b.addr]);
    let c = client_for(&["app.test", "www.app.test", "other.test"]).build().unwrap();

    for path in ["/", "/204", "/301", "/404", "/500"] {
        c.get(proxy.url("app.test", path)).send().await.unwrap();
    }
    for _ in 0..3 {
        let r = c.get(proxy.url("www.app.test", "/")).send().await.unwrap();
        assert_eq!(r.status(), 200);
    }
    // HTTP/2 too.
    let r = h2c_send(proxy.http, Request::get("/").header("host", "other.test").body(Full::default()).unwrap()).await;
    assert_eq!(r.status, 200);

    assert_eq!(summary(&counts(&proxy, service("srv-a"))), (5, 1, 1, 1, 8), "every host of the service");
    assert_eq!(summary(&counts(&proxy, service("srv-b"))), (1, 0, 0, 0, 1));
    assert_eq!(summary(&counts(&proxy, RequestTarget::Unrouted)), (0, 0, 0, 0, 0));

    // The time is measured: 160 ms falls in the bucket of (150, 200] ms or later.
    let before = counts(&proxy, service("srv-b"));
    c.get(proxy.url("other.test", "/slow")).send().await.unwrap();
    let slow = counts(&proxy, service("srv-b")).since(&before);
    assert!(slow.latency.percentile(0.5).unwrap() > 150.0, "{slow:?}");
}

#[tokio::test]
async fn requests_no_service_has_are_counted_as_unrouted() {
    let up = status_upstream().await;
    let hooks = MockHooks {
        challenges: HashMap::from([("tok".to_string(), "tok.thumbprint".to_string())]),
        ..MockHooks::default()
    };
    let proxy = TestProxy::start_with(Options {
        hooks: Some(Arc::new(hooks)),
        probe_id: Some("probe".into()),
        ..Options::default()
    })
    .await;
    proxy.route("srv", &["app.test"], &[up.addr]);
    let c = client_for(&["app.test", "bots.test"]).build().unwrap();

    for _ in 0..4 {
        let r = c.get(proxy.url("bots.test", "/wp-login.php")).send().await.unwrap();
        assert_eq!(r.status(), 404);
    }
    // Without a valid host.
    let r = h1_send(proxy.http, Request::get("/").body(Full::default()).unwrap()).await;
    assert_eq!(r.status, 400);
    // Answered by the proxy itself, even for a host of the service.
    let r = c.get(proxy.url("app.test", "/.well-known/acme-challenge/tok")).send().await.unwrap();
    assert_eq!(r.status(), 200);
    let token = "a".repeat(32);
    let r = c.get(proxy.url("app.test", &format!("/.well-known/ferry-domain-check/{token}"))).send().await.unwrap();
    assert_eq!(r.status(), 200);

    assert_eq!(summary(&counts(&proxy, RequestTarget::Unrouted)), (2, 0, 5, 0, 7));
    assert_eq!(summary(&counts(&proxy, service("srv"))), (0, 0, 0, 0, 0), "the app's numbers stay clean");
}

#[tokio::test]
async fn an_upstream_that_is_down_and_the_proxys_own_pages_count_as_5xx() {
    let proxy = TestProxy::start().await;
    proxy.route("down", &["down.test"], &[dead_addr().await]);
    proxy.route("empty", &["empty.test"], &[]);
    proxy.routes.set_service_suspended("paused", &["paused.test".to_string()]);
    let c = client_for(&["down.test", "empty.test", "paused.test"]).build().unwrap();

    for (host, status) in [("down.test", 502), ("empty.test", 503), ("paused.test", 503)] {
        let r = c.get(proxy.url(host, "/")).send().await.unwrap();
        assert_eq!(r.status(), status, "{host}");
    }
    for id in ["down", "empty", "paused"] {
        assert_eq!(summary(&counts(&proxy, service(id))), (0, 0, 0, 1, 1), "{id}");
    }
    assert_eq!(counts(&proxy, RequestTarget::Unrouted).total(), 0);
}

/// The time counted is the time until the response headers: a body still
/// streaming does not count.
#[tokio::test]
async fn a_request_is_counted_when_its_response_headers_go_out() {
    let release = Arc::new(Notify::new());
    let up = spawn_upstream({
        let release = release.clone();
        move |_req| {
            let release = release.clone();
            async move {
                let (mut tx, rx) = futures::channel::mpsc::channel::<Result<Frame<Bytes>, BoxError>>(1);
                tokio::spawn(async move {
                    release.notified().await;
                    let _ = tx.send(Ok(Frame::data(Bytes::from_static(b"done")))).await;
                });
                Response::builder().body(StreamBody::new(rx).boxed_unsync()).unwrap()
            }
        }
    })
    .await;
    let proxy = TestProxy::start().await;
    proxy.route("srv", &["app.test"], &[up.addr]);

    let resp = within("headers", client().get(proxy.url("app.test", "/events")).send()).await.unwrap();
    assert_eq!(resp.status(), 200);
    tokio::time::sleep(Duration::from_millis(300)).await;
    let counted = counts(&proxy, service("srv"));
    assert_eq!(summary(&counted), (1, 0, 0, 0, 1), "counted while the body is still on its way");
    let p95 = counted.latency.percentile(0.95).unwrap();
    assert!(p95 < 300.0, "the body's 300 ms are not in the latency: {p95} ms");
    release.notify_one();
    assert_eq!(within("body", resp.text()).await.unwrap(), "done");
    assert_eq!(counts(&proxy, service("srv")), counted, "nothing more once the body ends");
}

#[tokio::test]
async fn a_websocket_counts_once_and_stays_out_of_the_latency() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let echo = tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else { continue };
            tokio::spawn(async move {
                let Ok(mut ws) = tokio_tungstenite::accept_async(stream).await else { return };
                while let Some(Ok(msg)) = ws.next().await {
                    if msg.is_text() && ws.send(msg).await.is_err() {
                        break;
                    }
                }
            });
        }
    });
    let proxy = TestProxy::start().await;
    proxy.route("chat", &["ws.test"], &[addr]);

    let stream = TcpStream::connect(proxy.http).await.unwrap();
    let url = format!("ws://ws.test:{}/socket", proxy.http.port());
    let (mut ws, resp) = within("websocket handshake", tokio_tungstenite::client_async(url, stream)).await.unwrap();
    assert_eq!(resp.status(), 101);
    for i in 0..5 {
        ws.send(Message::text(format!("message {i}"))).await.unwrap();
        within("echo", ws.next()).await.unwrap().unwrap();
    }
    ws.close(None).await.unwrap();

    assert_eq!(summary(&counts(&proxy, service("chat"))), (1, 0, 0, 0, 0), "one 2xx, no latency");
    echo.abort();
}

#[tokio::test]
async fn counters_only_go_up() {
    let up = status_upstream().await;
    let proxy = TestProxy::start().await;
    proxy.route("srv", &["app.test"], &[up.addr]);
    let c = client();
    let mut readings = vec![counts(&proxy, service("srv"))];
    for n in [3, 0, 2] {
        for _ in 0..n {
            c.get(proxy.url("app.test", "/")).send().await.unwrap();
        }
        readings.push(counts(&proxy, service("srv")));
    }
    // A new deploy (new upstreams) keeps the service's counters.
    proxy.route("srv", &["app.test"], &[up.addr]);
    c.get(proxy.url("app.test", "/")).send().await.unwrap();
    readings.push(counts(&proxy, service("srv")));

    let totals: Vec<u64> = readings.iter().map(RequestCounts::total).collect();
    assert_eq!(totals, [0, 3, 3, 5, 6]);
    let between: Vec<u64> = readings.windows(2).map(|w| w[1].since(&w[0]).total()).collect();
    assert_eq!(between, [3, 0, 2, 1]);
}

/// A service that hangs shows as slow, not idle: the request its client
/// gives up on is counted when hyper drops it.
#[tokio::test]
async fn a_request_the_client_gives_up_on_is_counted_as_499() {
    let up = spawn_upstream(|_req| std::future::pending()).await;
    let proxy = TestProxy::start().await;
    proxy.route("stuck", &["app.test"], &[up.addr]);

    let mut stream = TcpStream::connect(proxy.http).await.unwrap();
    stream.write_all(b"GET / HTTP/1.1\r\nHost: app.test\r\n\r\n").await.unwrap();
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert_eq!(counts(&proxy, service("stuck")).total(), 0, "still waiting");
    drop(stream);

    let counted = within("the abandoned request is counted", async {
        loop {
            let c = counts(&proxy, service("stuck"));
            if c.total() > 0 {
                return c;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await;
    assert_eq!(summary(&counted), (0, 0, 1, 0, 1), "one 4xx, in the latency");
    assert!(counted.latency.percentile(0.5).unwrap() > 100.0, "the time the client waited: {counted:?}");
}
