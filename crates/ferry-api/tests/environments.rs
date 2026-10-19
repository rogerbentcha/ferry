//! Environments (`/api/v1/environments`, DESIGN.md §3): created, listed and
//! deleted; services and datastores pick one when they are created, and
//! their hostnames and references follow it.

mod common;

use std::sync::atomic::Ordering;

use common::TestApp;
use http::StatusCode;
use serde_json::{Value, json};

fn names(list: &Value) -> Vec<&str> {
    list.as_array().unwrap().iter().map(|e| e["name"].as_str().unwrap()).collect()
}

async fn create(app: &TestApp, name: &str) -> Value {
    let r = app.post("/api/v1/environments", json!({ "name": name })).await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", r.text());
    r.json()
}

#[tokio::test]
async fn every_server_has_production() {
    let app = TestApp::new().await;
    let list = app.get("/api/v1/environments").await.json();
    assert_eq!(names(&list), ["production"]);
    let production = &list[0];
    assert!(production["id"].as_str().unwrap().starts_with("env-"));
    assert_eq!(production["url_pattern"], "http://<service>.localhost:8080");
    assert_eq!(production["dns"], json!([]));
    assert_eq!((&production["services"], &production["datastores"]), (&json!([]), &json!([])));
    // By id or by name.
    let id = production["id"].as_str().unwrap();
    assert_eq!(app.get(&format!("/api/v1/environments/{id}")).await.json(), *production);
    assert_eq!(app.get("/api/v1/environments/production").await.json(), *production);
    assert_eq!(app.get("/api/v1/environments/nope").await.status, StatusCode::NOT_FOUND);

    // production can never be deleted.
    let r = app.delete("/api/v1/environments/production").await;
    assert_eq!(r.status, StatusCode::CONFLICT, "{}", r.text());
    assert!(r.text().contains("can't be deleted"), "{}", r.text());
    assert!(app.engine.calls_with("delete_environment").is_empty());
}

#[tokio::test]
async fn environments_are_created_with_their_network_and_dns_records() {
    let app = TestApp::new().await;
    let staging = create(&app, "staging").await;
    assert_eq!(staging["name"], "staging");
    assert_eq!(staging["url_pattern"], "http://<service>.staging.localhost:8080");
    assert_eq!(staging["dns"], json!([]), "a local domain needs no DNS record");
    assert_eq!(app.engine.calls_with("create_environment"), ["create_environment staging"]);
    assert_eq!(names(&app.get("/api/v1/environments").await.json()), ["production", "staging"]);

    // Names: the rules of service names, once per server.
    for bad in ["Staging", "-qa", "qa_1", "", "env-0123456789abcdef0123", "ferry"] {
        let r = app.post("/api/v1/environments", json!({ "name": bad })).await;
        assert_eq!(r.status, StatusCode::BAD_REQUEST, "{bad}: {}", r.text());
    }
    let r = app.post("/api/v1/environments", json!({ "name": "staging" })).await;
    assert_eq!(r.status, StatusCode::CONFLICT, "{}", r.text());
    let r = app.post("/api/v1/environments", json!({ "name": "qa", "network": "x" })).await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST, "unknown fields are rejected: {}", r.text());

    // Docker refuses its network: nothing is left behind.
    app.engine.fail_environment.store(true, Ordering::SeqCst);
    let r = app.post("/api/v1/environments", json!({ "name": "qa" })).await;
    assert_eq!(r.status, StatusCode::BAD_GATEWAY, "{}", r.text());
    assert_eq!(app.get("/api/v1/environments/qa").await.status, StatusCode::NOT_FOUND);
}

/// A connected domain needs `*.staging.<domain>`, pointing at the server.
#[tokio::test]
async fn the_dns_record_of_an_environment() {
    let app = TestApp::new().await;
    let r = app.post("/api/v1/domains", json!({ "name": "example.com" })).await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", r.text());
    let staging = create(&app, "staging").await;
    assert_eq!(
        staging["dns"],
        json!([{
            "domain": "example.com",
            "wildcard": "*.staging.example.com",
            "records": [{ "type": "A", "name": "*.staging", "value": "203.0.113.10", "required": true }],
        }])
    );
    let production = app.get("/api/v1/environments/production").await.json();
    assert_eq!(production["dns"], json!([]), "the domain's own `*` covers production");
}

#[tokio::test]
async fn services_and_datastores_are_created_in_an_environment() {
    let app = TestApp::new().await;
    let staging = create(&app, "staging").await;
    let shop = app.create_service(json!({ "name": "shop", "environment": "staging" })).await;
    assert_eq!(shop["environment"], "staging");
    assert_eq!(shop["url"], "http://shop.staging.localhost:8080");
    assert_eq!(shop["hosts"], json!(["shop.staging.localhost"]));
    assert_eq!(shop["internal_host"], "shop", "the same private name inside its environment");
    // By id too; no pick means production.
    let id = staging["id"].as_str().unwrap();
    let api = app.create_service(json!({ "name": "api", "type": "pserv", "environment": id })).await;
    assert_eq!(api["environment"], "staging");
    let web = app.create_service(json!({ "name": "web" })).await;
    assert_eq!(
        (web["environment"].as_str(), web["url"].as_str()),
        (Some("production"), Some("http://web.localhost:8080"))
    );
    let r = app.post("/api/v1/services", json!({ "name": "lost", "environment": "qa" })).await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST, "{}", r.text());
    assert!(r.text().contains("environment 'qa' not found"), "{}", r.text());

    let r = app
        .post("/api/v1/datastores", json!({ "name": "shop-db", "kind": "postgres", "environment": "staging" }))
        .await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", r.text());
    assert_eq!(r.json()["environment"], "staging");
    let r = app.post("/api/v1/datastores", json!({ "name": "db", "kind": "redis" })).await;
    assert_eq!(r.json()["environment"], "production");
    let r = app.post("/api/v1/datastores", json!({ "name": "lost-db", "kind": "redis", "environment": "qa" })).await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST, "{}", r.text());

    // A name is unique in its environment (another one may use it: see
    // `same_names.rs`).
    let r = app.post("/api/v1/services", json!({ "name": "shop", "environment": "staging" })).await;
    assert_eq!(r.status, StatusCode::CONFLICT, "{}", r.text());

    // Listings filter by environment.
    let staged = app.get("/api/v1/services?environment=staging").await.json();
    assert_eq!(names(&staged), ["api", "shop"]);
    assert_eq!(names(&app.get(&format!("/api/v1/services?environment={id}")).await.json()), ["api", "shop"]);
    assert_eq!(names(&app.get("/api/v1/services?environment=production").await.json()), ["web"]);
    assert_eq!(names(&app.get("/api/v1/services").await.json()), ["api", "shop", "web"]);
    assert_eq!(app.get("/api/v1/services?environment=qa").await.status, StatusCode::NOT_FOUND);
    assert_eq!(names(&app.get("/api/v1/datastores?environment=staging").await.json()), ["shop-db"]);
    assert_eq!(app.get("/api/v1/datastores?environment=qa").await.status, StatusCode::NOT_FOUND);

    let view = app.get("/api/v1/environments/staging").await.json();
    assert_eq!((&view["services"], &view["datastores"]), (&json!(["api", "shop"]), &json!(["shop-db"])));

    // The environment can't be changed afterwards.
    let r = app.patch("/api/v1/services/shop?environment=staging", json!({ "environment": "production" })).await;
    assert_eq!(r.status, StatusCode::BAD_REQUEST, "{}", r.text());
}

#[tokio::test]
async fn an_environment_is_deleted_once_empty() {
    let app = TestApp::new().await;
    create(&app, "staging").await;
    app.create_service(json!({ "name": "shop", "environment": "staging" })).await;
    let r =
        app.post("/api/v1/datastores", json!({ "name": "shop-db", "kind": "redis", "environment": "staging" })).await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", r.text());

    let r = app.delete("/api/v1/environments/staging").await;
    assert_eq!(r.status, StatusCode::CONFLICT, "{}", r.text());
    assert!(r.text().contains("still holds service 'shop', datastore 'shop-db'"), "{}", r.text());
    assert!(app.engine.calls_with("delete_environment").is_empty(), "the network stays");

    assert_eq!(app.delete("/api/v1/services/shop?environment=staging").await.status, StatusCode::NO_CONTENT);
    assert_eq!(app.delete("/api/v1/datastores/shop-db?environment=staging").await.status, StatusCode::NO_CONTENT);
    assert_eq!(app.delete("/api/v1/environments/staging").await.status, StatusCode::NO_CONTENT);
    assert_eq!(app.engine.calls_with("delete_environment"), ["delete_environment staging"]);
    assert_eq!(app.get("/api/v1/environments/staging").await.status, StatusCode::NOT_FOUND);
    assert_eq!(app.delete("/api/v1/environments/staging").await.status, StatusCode::NOT_FOUND);
}

/// Hostnames are claimed across environments: a custom domain can't be the
/// default host of a service of another environment.
#[tokio::test]
async fn default_hosts_outside_production_are_claimed() {
    let app = TestApp::new().await;
    create(&app, "staging").await;
    app.create_service(json!({ "name": "shop", "environment": "staging" })).await;
    let r = app.post("/api/v1/services", json!({ "name": "web", "custom_domains": ["shop.staging.localhost"] })).await;
    assert_eq!(r.status, StatusCode::CONFLICT, "{}", r.text());
    assert!(r.text().contains("default host of service 'shop'"), "{}", r.text());
    // Its production-shaped host is free: it isn't its host.
    let r = app.post("/api/v1/services", json!({ "name": "web", "custom_domains": ["shop.localhost"] })).await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", r.text());
}

/// A blueprint applies to production (another environment's services of
/// the same names stay as they are: see `same_names.rs`).
#[tokio::test]
async fn blueprints_stay_in_production() {
    let app = TestApp::new().await;
    create(&app, "staging").await;
    let yaml = "services:\n  - type: web\n    name: site\n    image: nginx:alpine\n";
    let r = app
        .send(
            http::Request::post("/api/v1/blueprints/apply")
                .header("authorization", format!("Bearer {}", common::TOKEN))
                .header("content-type", "application/yaml")
                .body(axum::body::Body::from(yaml))
                .unwrap(),
        )
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text());
    assert_eq!(app.get("/api/v1/services/site").await.json()["environment"], "production");
}

/// `*.staging.<domain>` would hide `staging.<domain>` (RFC 4592: the name
/// then exists, and the domain's own wildcard no longer answers for it): a
/// service of production and an environment never share a name.
#[tokio::test]
async fn a_production_service_and_an_environment_never_share_a_name() {
    let app = TestApp::new().await;
    app.create_service(json!({ "name": "staging" })).await;
    let r = app.post("/api/v1/environments", json!({ "name": "staging" })).await;
    assert_eq!(r.status, StatusCode::CONFLICT, "{}", r.text());
    assert!(r.text().contains("would hide its hostname staging.<domain>"), "{}", r.text());
    assert!(app.engine.calls_with("create_environment").is_empty(), "no network either");

    create(&app, "qa").await;
    let r = app.post("/api/v1/services", json!({ "name": "qa" })).await;
    assert_eq!(r.status, StatusCode::CONFLICT, "{}", r.text());
    assert!(r.text().contains("is the name of an environment"), "{}", r.text());
    // Inside another environment it is served at qa.qa.<domain>: no clash.
    let inside = app.create_service(json!({ "name": "qa", "environment": "qa" })).await;
    assert_eq!(inside["url"], "http://qa.qa.localhost:8080");

    // A blueprint creates in production: the same rule.
    create(&app, "preview").await;
    let yaml = "services:\n  - type: web\n    name: preview\n    image: nginx:alpine\n";
    let r = app
        .send(
            http::Request::post("/api/v1/blueprints/apply?dry_run=true")
                .header("authorization", format!("Bearer {}", common::TOKEN))
                .header("content-type", "application/yaml")
                .body(axum::body::Body::from(yaml))
                .unwrap(),
        )
        .await;
    assert_eq!(r.status, StatusCode::CONFLICT, "{}", r.text());
    assert!(r.text().contains("the name of an environment"), "{}", r.text());
}

/// Nor do an environment and the dashboard's host (`ferry.<domain>`).
#[tokio::test]
async fn an_environment_never_hides_the_dashboard() {
    let app = TestApp::with_config(|c| c.dashboard_host = Some("admin.localhost".into())).await;
    let r = app.post("/api/v1/environments", json!({ "name": "admin" })).await;
    assert_eq!(r.status, StatusCode::CONFLICT, "{}", r.text());
    assert!(
        r.text().contains("would hide the Ferry dashboard") && r.text().contains("admin.localhost"),
        "{}",
        r.text()
    );
    // A domain connected later would put one there too.
    let app = TestApp::with_config(|c| c.dashboard_host = Some("qa.example.com".into())).await;
    create(&app, "qa").await;
    let r = app.post("/api/v1/domains", json!({ "name": "example.com" })).await;
    assert_eq!(r.status, StatusCode::CONFLICT, "{}", r.text());
    assert!(r.text().contains("environment 'qa'"), "{}", r.text());
}
