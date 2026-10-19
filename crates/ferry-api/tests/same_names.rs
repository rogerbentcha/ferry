//! The same names in every environment (DESIGN.md §23): a service or
//! datastore name is unique in its environment, and a route that names one
//! takes `?environment=` (omitted: `production`).

mod common;

use common::TestApp;
use http::StatusCode;
use serde_json::{Value, json};

async fn staging(app: &TestApp) -> Value {
    let r = app.post("/api/v1/environments", json!({ "name": "staging" })).await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", r.text());
    r.json()
}

/// `api` and `db` in production and in staging; returns the two `api`.
async fn both(app: &TestApp) -> (Value, Value) {
    staging(app).await;
    let api = app.create_service(json!({ "name": "api", "image": "nginx:alpine", "deploy": false })).await;
    let staging_api = app
        .create_service(json!({ "name": "api", "image": "nginx:alpine", "deploy": false, "environment": "staging" }))
        .await;
    for body in
        [json!({ "name": "db", "kind": "redis" }), json!({ "name": "db", "kind": "redis", "environment": "staging" })]
    {
        let r = app.post("/api/v1/datastores", body).await;
        assert_eq!(r.status, StatusCode::CREATED, "{}", r.text());
    }
    (api, staging_api)
}

#[tokio::test]
async fn the_same_name_runs_in_two_environments() {
    let app = TestApp::new().await;
    let (api, staging_api) = both(&app).await;
    assert_ne!(api["id"], staging_api["id"]);
    assert_eq!(
        (api["url"].as_str(), api["environment"].as_str()),
        (Some("http://api.localhost:8080"), Some("production"))
    );
    assert_eq!(
        (staging_api["url"].as_str(), staging_api["environment"].as_str()),
        (Some("http://api.staging.localhost:8080"), Some("staging"))
    );
    assert_eq!(staging_api["internal_host"], "api", "the same private name, on staging's network");
    let listed: Vec<_> = app
        .get("/api/v1/services")
        .await
        .json()
        .as_array()
        .unwrap()
        .iter()
        .map(|s| (s["name"].as_str().unwrap().to_string(), s["environment"].as_str().unwrap().to_string()))
        .collect();
    let want = [("api", "production"), ("api", "staging")].map(|(n, e)| (n.to_string(), e.to_string()));
    assert_eq!(listed, want, "production's first");

    // Twice in one environment: refused, whatever takes the name.
    let r = app.post("/api/v1/services", json!({ "name": "api", "environment": "staging" })).await;
    assert_eq!(r.status, StatusCode::CONFLICT, "{}", r.text());
    assert!(r.text().contains("name 'api' is already in use in environment 'staging'"), "{}", r.text());
    let r = app.post("/api/v1/datastores", json!({ "name": "api", "kind": "redis", "environment": "staging" })).await;
    assert_eq!(r.status, StatusCode::CONFLICT, "{}", r.text());
    let r = app.post("/api/v1/services", json!({ "name": "db" })).await;
    assert_eq!(r.status, StatusCode::CONFLICT, "{}", r.text());
    assert!(r.text().contains("in environment 'production'"), "{}", r.text());
}

#[tokio::test]
async fn a_name_is_production_s_unless_an_environment_is_named() {
    let app = TestApp::new().await;
    let (api, staging_api) = both(&app).await;
    let staging_id = app.get("/api/v1/environments/staging").await.json()["id"].as_str().unwrap().to_string();
    let id = |v: &Value| v["id"].as_str().unwrap().to_string();

    for (uri, want) in [
        ("/api/v1/services/api".to_string(), id(&api)),
        ("/api/v1/services/api?environment=production".to_string(), id(&api)),
        ("/api/v1/services/api?environment=staging".to_string(), id(&staging_api)),
        (format!("/api/v1/services/api?environment={staging_id}"), id(&staging_api)),
        // An id: in any environment.
        (format!("/api/v1/services/{}", id(&staging_api)), id(&staging_api)),
    ] {
        let r = app.get(&uri).await;
        assert_eq!(r.status, StatusCode::OK, "{uri}: {}", r.text());
        assert_eq!(r.json()["id"].as_str(), Some(want.as_str()), "{uri}");
    }
    // An id with another environment, or an environment that doesn't exist.
    let r = app.get(&format!("/api/v1/services/{}?environment=production", id(&staging_api))).await;
    assert_eq!(r.status, StatusCode::NOT_FOUND, "{}", r.text());
    let r = app.get("/api/v1/services/api?environment=qa").await;
    assert_eq!(r.status, StatusCode::NOT_FOUND, "{}", r.text());
    assert!(r.text().contains("environment 'qa' not found"), "{}", r.text());

    // A change for staging changes staging's only.
    let r = app.patch("/api/v1/services/api?environment=staging", json!({ "start_command": "serve --staging" })).await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text());
    assert_eq!(app.get("/api/v1/services/api?environment=staging").await.json()["start_command"], "serve --staging");
    assert_eq!(app.get("/api/v1/services/api").await.json()["start_command"], Value::Null);
    let r = app
        .put("/api/v1/services/api/env?environment=staging", json!({ "vars": [{ "key": "A", "value": "1" }] }))
        .await;
    assert_eq!(r.status, StatusCode::OK, "{}", r.text());
    assert_eq!(app.get("/api/v1/services/api/env").await.json(), json!([]));
    assert_eq!(
        app.get("/api/v1/services/api/env?environment=staging").await.json(),
        json!([{ "key": "A", "value": "1" }])
    );

    // Old scripts: no environment deploys production's.
    let r = app.post("/api/v1/services/api/deploys", json!({})).await;
    assert_eq!(r.status, StatusCode::ACCEPTED, "{}", r.text());
    let r = app.post("/api/v1/services/api/deploys?environment=staging", json!({})).await;
    assert_eq!(r.status, StatusCode::ACCEPTED, "{}", r.text());
    let deploys: Vec<String> =
        app.engine.calls_with("deploy ").into_iter().map(|c| c.split(' ').nth(1).unwrap().to_string()).collect();
    assert_eq!(deploys, [id(&api), id(&staging_api)]);
    let r = app.get("/api/v1/services/api/deploys?environment=staging").await;
    assert_eq!(r.json().as_array().unwrap().len(), 1, "{}", r.text());

    // Datastores too.
    let db = app.get("/api/v1/datastores/db").await.json();
    let staging_db = app.get("/api/v1/datastores/db?environment=staging").await.json();
    assert_eq!((db["environment"].as_str(), staging_db["environment"].as_str()), (Some("production"), Some("staging")));
    assert_ne!(db["password"], staging_db["password"]);
    assert_eq!(app.delete("/api/v1/datastores/db?environment=staging").await.status, StatusCode::NO_CONTENT);
    assert_eq!(app.get("/api/v1/datastores/db").await.status, StatusCode::OK, "production's stays");
}

/// A name only another environment uses: the 404 says where it is.
#[tokio::test]
async fn a_name_of_another_environment_says_where_it_is() {
    let app = TestApp::new().await;
    staging(&app).await;
    app.create_service(json!({ "name": "shop", "environment": "staging" })).await;
    let r = app.get("/api/v1/services/shop").await;
    assert_eq!(r.status, StatusCode::NOT_FOUND, "{}", r.text());
    assert!(
        r.text().contains("service 'shop' not found in environment 'production' (environment 'staging' has one)")
            && r.text().contains("--environment staging"),
        "{}",
        r.text()
    );
    let r = app.get("/api/v1/services/nope").await;
    assert_eq!(r.json()["error"]["message"], "service 'nope' not found");
    let r = app.get("/api/v1/services/nope?environment=staging").await;
    assert_eq!(r.json()["error"]["message"], "service 'nope' in environment 'staging' not found");
}

/// References to `api` and `db` find each environment's own: deleting
/// staging's isn't held back by production's references, and the reverse.
#[tokio::test]
async fn references_and_deletes_stay_in_their_environment() {
    let app = TestApp::new().await;
    both(&app).await;
    let web = app
        .create_service(json!({ "name": "web", "env": [
        { "key": "API_URL", "value": "${{service.api.url}}" },
        { "key": "DATABASE_URL", "value": "${{datastore.db.connectionString}}" },
    ] }))
        .await;
    assert_eq!(web["environment"], "production");

    assert_eq!(app.delete("/api/v1/services/api?environment=staging").await.status, StatusCode::NO_CONTENT);
    assert_eq!(app.delete("/api/v1/datastores/db?environment=staging").await.status, StatusCode::NO_CONTENT);
    let r = app.delete("/api/v1/services/api").await;
    assert_eq!(r.status, StatusCode::CONFLICT, "{}", r.text());
    assert!(r.text().contains("service 'web' (API_URL)"), "{}", r.text());
    let r = app.delete("/api/v1/datastores/db").await;
    assert_eq!(r.status, StatusCode::CONFLICT, "{}", r.text());
}

/// Each service keeps its own deploy hook.
#[tokio::test]
async fn deploy_hooks_keep_working_for_each() {
    let app = TestApp::new().await;
    let (api, staging_api) = both(&app).await;
    for svc in [&api, &staging_api] {
        let path = svc["deploy_hook_path"].as_str().unwrap();
        let r = app.post(path, json!({})).await;
        assert!(r.status.is_success(), "{path}: {}", r.text());
    }
    let deploys: Vec<String> =
        app.engine.calls_with("deploy ").into_iter().map(|c| c.split(' ').nth(1).unwrap().to_string()).collect();
    assert_eq!(deploys, [api["id"].as_str().unwrap(), staging_api["id"].as_str().unwrap()]);
}

/// Env groups are server-wide: they list their services by qualified name.
#[tokio::test]
async fn env_groups_tell_the_same_names_apart() {
    let app = TestApp::new().await;
    both(&app).await;
    let r = app.post("/api/v1/env-groups", json!({ "name": "shared" })).await;
    assert_eq!(r.status, StatusCode::CREATED, "{}", r.text());
    for uri in ["/api/v1/services/api/env-groups", "/api/v1/services/api/env-groups?environment=staging"] {
        let r = app.post(uri, json!({ "group": "shared" })).await;
        assert_eq!(r.status, StatusCode::OK, "{uri}: {}", r.text());
    }
    assert_eq!(app.get("/api/v1/env-groups/shared").await.json()["services"], json!(["api", "api.staging"]));
}

/// A blueprint applies to production: another environment's `api` neither
/// stops it nor changes.
#[tokio::test]
async fn blueprints_leave_the_same_names_elsewhere_alone() {
    let app = TestApp::new().await;
    staging(&app).await;
    let staging_api = app
        .create_service(json!({ "name": "api", "environment": "staging", "image": "nginx:1", "deploy": false }))
        .await;
    let yaml = "services:\n  - type: web\n    name: api\n    image: nginx:alpine\n";
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
    assert_eq!(r.json()["actions"][0]["action"], "create", "{}", r.text());
    let api = app.get("/api/v1/services/api").await.json();
    assert_eq!((api["environment"].as_str(), api["image"].as_str()), (Some("production"), Some("nginx:alpine")));
    let still = app.get("/api/v1/services/api?environment=staging").await.json();
    assert_eq!((still["id"].clone(), still["image"].as_str()), (staging_api["id"].clone(), Some("nginx:1")));
}
