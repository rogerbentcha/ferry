//! Auth, liveness, the web client, fallbacks and server info.

mod common;

use axum::body::Body;
use common::{TOKEN, TestApp};
use http::{Method, Request, StatusCode};
use serde_json::{Value, json};

fn req(method: Method, uri: &str) -> http::request::Builder {
    Request::builder().method(method).uri(uri)
}

#[tokio::test]
async fn healthz_and_dashboard_need_no_auth() {
    let app = TestApp::new().await;
    let r = app.send(req(Method::GET, "/healthz").body(Body::empty()).unwrap()).await;
    assert_eq!(r.status, StatusCode::OK);
    assert_eq!(r.text(), "ok");

    // The web client (built or placeholder: these checks don't depend on
    // `web/dist`). Client-side routes get index.html too (SPA fallback).
    let index = ferry_api::web::embedded_file("index.html").expect("an index.html is always embedded");
    for path in ["/", "/index.html", "/services", "/services/web/deploys/dep-1?x=1", "/nope"] {
        let r = app.send(req(Method::GET, path).body(Body::empty()).unwrap()).await;
        assert_eq!(r.status, StatusCode::OK, "{path}");
        assert_eq!(r.headers["content-type"], "text/html; charset=utf-8", "{path}");
        assert_eq!(r.headers["cache-control"], "no-cache", "{path}");
        assert_eq!(r.headers["x-frame-options"], "DENY", "{path}");
        assert_eq!(r.body, index, "{path}");
    }
    // revalidation with the ETag
    let r = app.send(req(Method::GET, "/").body(Body::empty()).unwrap()).await;
    let etag = r.headers["etag"].to_str().unwrap().to_string();
    let r = app.send(req(Method::GET, "/services").header("if-none-match", &etag).body(Body::empty()).unwrap()).await;
    assert_eq!(r.status, StatusCode::NOT_MODIFIED);
    assert!(r.body.is_empty());
    // HEAD
    let r = app.send(req(Method::HEAD, "/").body(Body::empty()).unwrap()).await;
    assert_eq!(r.status, StatusCode::OK);
    assert!(r.body.is_empty());
}

#[tokio::test]
async fn embedded_assets_are_served_with_long_lived_caching() {
    // Whatever `web/dist` contained at build time (maybe nothing).
    let app = TestApp::new().await;
    for path in ferry_api::web::embedded_paths() {
        let r = app.send(req(Method::GET, &format!("/{path}")).body(Body::empty()).unwrap()).await;
        assert_eq!(r.status, StatusCode::OK, "{path}");
        assert_eq!(r.headers["content-type"], ferry_api::web::content_type(path), "{path}");
        assert_eq!(r.body, ferry_api::web::embedded_file(path).unwrap(), "{path}");
        let cache = if path.starts_with("assets/") { "public, max-age=31536000, immutable" } else { "no-cache" };
        assert_eq!(r.headers["cache-control"], cache, "{path}");
    }
    // a missing hashed asset is a 404 (never HTML parsed as a script)
    let r = app.send(req(Method::GET, "/assets/index-00000000.js").body(Body::empty()).unwrap()).await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);
    assert_eq!(r.code(), "not_found");
}

#[tokio::test]
async fn api_requires_a_valid_token() {
    let app = TestApp::new().await;
    // missing
    let r = app.send(req(Method::GET, "/api/v1/info").body(Body::empty()).unwrap()).await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
    assert_eq!(r.code(), "unauthorized");
    // wrong bearer
    let r = app
        .send(req(Method::GET, "/api/v1/info").header("authorization", "Bearer nope").body(Body::empty()).unwrap())
        .await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
    // wrong scheme
    let r = app
        .send(
            req(Method::GET, "/api/v1/info")
                .header("authorization", format!("Basic {TOKEN}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
    // right bearer (scheme is case-insensitive)
    let r = app
        .send(
            req(Method::GET, "/api/v1/info")
                .header("authorization", format!("bearer {TOKEN}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK);
    // access_token on GET
    let r =
        app.send(req(Method::GET, &format!("/api/v1/info?access_token={TOKEN}")).body(Body::empty()).unwrap()).await;
    assert_eq!(r.status, StatusCode::OK);
    let r = app.send(req(Method::GET, "/api/v1/info?access_token=wrong").body(Body::empty()).unwrap()).await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
    // access_token is not accepted for other methods
    let r = app
        .send(
            req(Method::POST, &format!("/api/v1/env-groups?access_token={TOKEN}"))
                .header("content-type", "application/json")
                .body(Body::from(r#"{"name":"g"}"#))
                .unwrap(),
        )
        .await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
    assert!(app.store.list_env_groups().await.unwrap().is_empty());
}

#[tokio::test]
async fn empty_server_token_fails_closed() {
    let app = TestApp::with_config(|c| c.api_token = String::new()).await;
    let r = app
        .send(req(Method::GET, "/api/v1/info").header("authorization", "Bearer ").body(Body::empty()).unwrap())
        .await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn unknown_routes_are_json_404s() {
    let app = TestApp::new().await;
    let r = app.get("/api/v1/nope").await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);
    assert_eq!(r.code(), "not_found");
    for path in ["/api", "/api/", "/api/v1", "/api/v2/services"] {
        let r = app.get(path).await;
        assert_eq!(r.status, StatusCode::NOT_FOUND, "{path}");
        assert_eq!(r.code(), "not_found", "{path}");
    }
    let r = app.get("/api/whatever/deep/path").await;
    assert_eq!(r.status, StatusCode::NOT_FOUND);
    assert_eq!(r.code(), "not_found");
    // unauthenticated unknown API path: auth runs first
    let r = app.send(req(Method::GET, "/api/v1/nope").body(Body::empty()).unwrap()).await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
    // outside /api: webhooks and liveness keep JSON 404s (everything else
    // is the web client's SPA fallback)
    for path in ["/hooks", "/hooks/nope", "/hooks/deploy", "/healthz/x"] {
        let r = app.send(req(Method::GET, path).body(Body::empty()).unwrap()).await;
        assert_eq!(r.status, StatusCode::NOT_FOUND, "{path}");
        assert_eq!(r.code(), "not_found", "{path}");
    }
    // wrong method on a known route
    let r = app.call(Method::PUT, "/api/v1/services", Some(json!({}))).await;
    assert_eq!(r.status, StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(r.code(), "method_not_allowed");
}

#[tokio::test]
async fn extractor_rejections_are_json() {
    let app = TestApp::new().await;
    let r = app
        .send(
            req(Method::POST, "/api/v1/services")
                .header("authorization", format!("Bearer {TOKEN}"))
                .header("content-type", "application/json")
                .body(Body::from("{not json"))
                .unwrap(),
        )
        .await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    assert_eq!(r.code(), "invalid_request");
    // wrong field type
    let r = app.post("/api/v1/services", json!({"name": "web", "instances": "many"})).await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    // missing required field
    let r = app.post("/api/v1/services", json!({})).await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    assert!(r.json()["error"]["message"].as_str().unwrap().contains("name"));
    // bad query
    let r = app.get("/api/v1/services/x/deploys?limit=abc").await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
    assert_eq!(r.code(), "invalid_request");
    // bad enum
    let r = app.post("/api/v1/services", json!({"name": "web", "type": "mainframe"})).await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn server_info() {
    let app = TestApp::with_config(|c| {
        c.github_webhook_secret = Some("s3cret".into());
        c.proxy_addr = "0.0.0.0:8080".parse().unwrap();
    })
    .await;
    let r = app.get("/api/v1/info").await;
    assert_eq!(r.status, StatusCode::OK);
    let v = r.json();
    assert_eq!(v["version"], ferry_core::VERSION);
    assert_eq!(v["base_domain"], "localhost");
    assert_eq!(v["default_domain"], "localhost");
    assert_eq!(v["proxy_url"], "http://localhost:8080");
    assert_eq!(v["tls_enabled"], false);
    assert_eq!(v["dashboard_url"], "http://ferry.localhost:8080");
    assert_eq!((&v["github_webhook_enabled"], &v["github_webhook_secret_set"]), (&json!(true), &json!(true)));
    assert_eq!(v["docker_version"], "27.0.0");
    // resource limits: the server defaults and the Docker host's capacity
    assert_eq!(v["default_memory_limit_mb"], 512);
    assert_eq!(v["default_cpu_limit"], 1.0);
    assert_eq!(v["docker_cpus"], 4);
    assert_eq!(v["docker_memory_bytes"], 8u64 << 30);

    let app = TestApp::new().await;
    let v = app.get("/api/v1/info").await.json();
    assert_eq!((&v["github_webhook_enabled"], &v["github_webhook_secret_set"]), (&json!(false), &json!(false)));

    // 0 = unlimited
    let app = TestApp::with_config(|c| {
        c.default_memory_limit_mb = 0;
        c.default_cpu_limit = 0.0;
    })
    .await;
    let v = app.get("/api/v1/info").await.json();
    assert_eq!((v["default_memory_limit_mb"].clone(), v["default_cpu_limit"].clone()), (json!(0), json!(0.0)));
    let app = TestApp::with_config(|c| {
        c.default_memory_limit_mb = 2048;
        c.default_cpu_limit = 0.5;
    })
    .await;
    let v = app.get("/api/v1/info").await.json();
    assert_eq!((v["default_memory_limit_mb"].clone(), v["default_cpu_limit"].clone()), (json!(2048), json!(0.5)));
}

#[tokio::test]
async fn server_metrics_come_from_the_engine_and_need_auth() {
    let app = TestApp::new().await;
    let r = app.send(req(Method::GET, "/api/v1/server/metrics").body(Body::empty()).unwrap()).await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
    assert!(app.engine.calls_with("server_metrics").is_empty(), "nothing is read without a token");

    let r = app.get("/api/v1/server/metrics").await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text());
    assert_eq!(app.engine.calls_with("server_metrics"), ["server_metrics"]);
    let v = r.json();
    assert!(v["at"].as_str().is_some_and(|at| at.ends_with('Z')), "{v}");
    let host = &v["host"];
    assert_eq!((&host["cpu_percent"], &host["cpu_count"]), (&json!(37.5), &json!(4)));
    assert_eq!((&host["memory_used_bytes"], &host["memory_total_bytes"]), (&json!(3u64 << 30), &json!(8u64 << 30)));
    // A value that can't be read is null, not missing.
    assert!(host.get("network_rx_bytes_per_sec").is_some_and(|v| v.is_null()), "{host}");
    assert_eq!(
        host["disks"],
        json!([{
            "mount_point": "/",
            "used_bytes": 40u64 << 30,
            "total_bytes": 100u64 << 30,
            "read_bytes_per_sec": 1 << 20,
            "write_bytes_per_sec": 512 << 10,
        }])
    );
    let containers = v["containers"].as_array().unwrap();
    assert_eq!(containers.len(), 2);
    assert_eq!(
        containers[0],
        json!({
            "kind": "service",
            "id": "srv-1",
            "name": "web",
            "instance": "9f8e7d",
            "cpu_percent": 120.0,
            "memory_bytes": 300 << 20,
            "memory_limit_bytes": 512 << 20,
            "cpu_limit": 2.0,
        })
    );
    assert_eq!((&containers[1]["kind"], &containers[1]["instance"]), (&json!("datastore"), &Value::Null));
    assert!(containers[1]["cpu_percent"].is_null() && containers[1]["memory_bytes"].is_null());
    let requests = v["requests"].as_array().unwrap();
    assert_eq!(
        requests[0],
        json!({
            "target": "srv-1",
            "name": "web",
            "requests_per_sec": 12.5,
            "status_2xx": 90,
            "status_3xx": 0,
            "status_4xx": 0,
            "status_5xx": 10,
            "p50_ms": 1.0,
            "p95_ms": 4.0,
        })
    );
    // Without a request to time, the percentiles are null, not missing.
    assert_eq!(requests[1]["target"], json!("unrouted"));
    assert!(requests[1].get("p95_ms").is_some_and(Value::is_null), "{}", requests[1]);
}

#[tokio::test]
async fn the_metrics_history_takes_a_range_and_needs_auth() {
    let app = TestApp::new().await;
    let r = app.send(req(Method::GET, "/api/v1/server/metrics/history").body(Body::empty()).unwrap()).await;
    assert_eq!(r.status, StatusCode::UNAUTHORIZED);
    assert!(app.engine.calls_with("metrics_history").is_empty(), "nothing is read without a token");

    // The last hour by default.
    let r = app.get("/api/v1/server/metrics/history").await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text());
    let v = r.json();
    assert_eq!((&v["range"], &v["step_seconds"]), (&json!("1h"), &json!(10)));
    assert!(v["from"].as_str().is_some_and(|at| at.ends_with('Z')), "{v}");
    assert!(v["to"].as_str().is_some_and(|at| at.ends_with('Z')), "{v}");
    let host = v["host"].as_array().unwrap();
    assert_eq!(host.len(), 2);
    assert_eq!((&host[0]["cpu_percent"], &host[0]["memory_used_bytes"]), (&json!(12.5), &json!(1u64 << 30)));
    // A value that wasn't read is null, not missing.
    assert!(host[1].get("cpu_percent").is_some_and(Value::is_null), "{}", host[1]);
    assert!(host[1].get("network_rx_bytes_per_sec").is_some_and(Value::is_null), "{}", host[1]);
    let containers = v["containers"].as_array().unwrap();
    assert_eq!(
        (&containers[0]["kind"], &containers[0]["name"], &containers[0]["instance"]),
        (&json!("service"), &json!("web"), &json!("9f8e7d"))
    );
    assert_eq!(containers[0]["points"][0]["cpu_percent"], json!(150.0));
    let requests = v["requests"].as_array().unwrap();
    assert_eq!(
        requests[0],
        json!({
            "at": host[0]["at"],
            "seconds": 10,
            "requests_per_sec": 2.5,
            "status_2xx": 20,
            "status_3xx": 1,
            "status_4xx": 2,
            "status_5xx": 2,
            "p50_ms": 12.0,
            "p95_ms": 180.0,
        })
    );
    assert!(requests[1].get("p50_ms").is_some_and(Value::is_null), "{}", requests[1]);
    let targets = v["request_targets"].as_array().unwrap();
    assert_eq!(
        (&targets[0]["target"], &targets[0]["name"], &targets[0]["p95_ms"]),
        (&json!("srv-1"), &json!("web"), &json!(180.0))
    );

    for (range, step) in [("1h", 10), ("6h", 60), ("24h", 240)] {
        let r = app.get(&format!("/api/v1/server/metrics/history?range={range}")).await;
        assert_eq!(r.status, StatusCode::OK, "{range}: {}", r.text());
        assert_eq!((&r.json()["range"], &r.json()["step_seconds"]), (&json!(range), &json!(step)));
    }
    assert_eq!(
        app.engine.calls_with("metrics_history"),
        ["metrics_history 1h", "metrics_history 1h", "metrics_history 6h", "metrics_history 24h"]
    );

    for range in ["2h", "1H", "", "7d"] {
        let r = app.get(&format!("/api/v1/server/metrics/history?range={range}")).await;
        assert_eq!(r.status, StatusCode::BAD_REQUEST, "{range}: {}", r.text());
        assert!(r.json()["error"]["message"].as_str().is_some_and(|m| m.contains("range")), "{}", r.text());
    }
    assert_eq!(app.engine.calls_with("metrics_history").len(), 4, "an unknown range reads nothing");
}

#[tokio::test]
async fn every_error_outside_the_api_is_json_too() {
    let app = TestApp::new().await;
    for (method, path) in [
        (Method::POST, "/healthz"),
        (Method::DELETE, "/"),
        (Method::PUT, "/index.html"),
        (Method::POST, "/services/web"),
    ] {
        let r = app.send(req(method.clone(), path).body(Body::empty()).unwrap()).await;
        assert_eq!(r.status, StatusCode::METHOD_NOT_ALLOWED, "{method} {path}");
        assert_eq!(r.headers["content-type"], "application/json", "{method} {path}");
        assert_eq!(r.code(), "method_not_allowed");
    }
    let r = app.send(req(Method::DELETE, "/hooks/github").body(Body::empty()).unwrap()).await;
    assert_eq!((r.status, r.code()), (StatusCode::METHOD_NOT_ALLOWED, "method_not_allowed".to_string()));
    // the API 404 names the full path
    let r = app.get("/api/v1/nope").await;
    assert_eq!(r.json()["error"]["message"], "no API route for /api/v1/nope");
}
