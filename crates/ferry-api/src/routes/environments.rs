//! `/api/v1/environments` — groups of services and datastores, each on a
//! private network of its own (DESIGN.md §3): `production`, which every
//! server has, and the ones created here (`staging`…). A service or
//! datastore picks its environment when it is created; its instances reach
//! the datastores and services of that environment by name, and nothing of
//! another one. Outside `production` a web service answers at
//! `<service>.<environment>.<domain>`.

use std::net::IpAddr;

use axum::Json;
use axum::extract::State;
use ferry_core::dto::{ApiErrorBody, CreateEnvironment, EnvironmentView};
use ferry_core::{Environment, Error, config, validate};
use http::StatusCode;

use crate::AppState;
use crate::checks;
use crate::error::ApiResult;
use crate::extract::{ApiJson, ApiPath};
use crate::locks;
use crate::views::environment_view;

/// The server's public addresses, for the DNS records of environments —
/// not asked for when every domain is local (no record is needed then).
async fn addresses(st: &AppState) -> Vec<IpAddr> {
    if st.config.claimed_domains().iter().all(|d| config::is_local_host(d)) {
        return Vec::new();
    }
    st.engine.public_addresses().await
}

/// `GET /api/v1/environments`
#[utoipa::path(
    get,
    path = "/api/v1/environments",
    tag = "environments",
    operation_id = "listEnvironments",
    summary = "List environments",
    description = "Every environment, `production` first, with the names of its services and datastores and the DNS records its services need.",
    responses((status = 200, description = "The environments.", body = [EnvironmentView])),
)]
pub async fn list(State(st): State<AppState>) -> ApiResult<Json<Vec<EnvironmentView>>> {
    let addresses = addresses(&st).await;
    let mut out = Vec::new();
    for env in st.store.list_environments().await? {
        out.push(environment_view(&st.store, &st.config, env, &addresses).await?);
    }
    Ok(Json(out))
}

/// `POST /api/v1/environments`
#[utoipa::path(
    post,
    path = "/api/v1/environments",
    tag = "environments",
    operation_id = "createEnvironment",
    summary = "Create an environment",
    description = "Creates the environment and its private network. Create services and datastores in it by naming it (`environment`) when creating them. Its web services answer at `<service>.<environment>.<domain>`: for each domain of the server that is not local, create the wildcard DNS record of `dns` (`*.staging.example.com`). Names follow the rules of service names and can't be changed.",
    request_body = CreateEnvironment,
    responses(
        (status = 201, description = "The new environment, with the DNS records its services need.", body = EnvironmentView),
        (status = 400, description = "Invalid name.", body = ApiErrorBody),
        (status = 409, description = "The name is already taken, or it is the name of a service of production or the first label of the dashboard's host under a domain of the server (the environment's DNS record would hide that hostname).", body = ApiErrorBody),
        (status = 502, description = "Docker refused to create its network (nothing was created).", body = ApiErrorBody),
    ),
)]
pub async fn create(
    State(st): State<AppState>,
    ApiJson(req): ApiJson<CreateEnvironment>,
) -> ApiResult<(StatusCode, Json<EnvironmentView>)> {
    let name = req.name.trim().to_string();
    validate::environment_name(&name)?;
    locks::detached(async move {
        let env = Environment::new(name);
        {
            // Nothing can be created in it before its network exists.
            let _names = locks::names().await;
            // Its hostnames are claimed like a domain's (lock order: names, then domain).
            let _domains = locks::domains().await;
            checks::check_environment_name_free(&st.store, &st.config, &env.name).await?;
            st.store.create_environment(&env).await?;
            if let Err(e) = st.engine.create_environment(&env.name).await {
                if let Err(del) = st.store.delete_environment(&env.id).await {
                    tracing::error!(environment = %env.name, "rolling back environment creation failed: {del}");
                }
                return Err(e.into());
            }
        }
        tracing::info!(environment = %env.name, id = %env.id, "created environment");
        let addresses = addresses(&st).await;
        Ok((StatusCode::CREATED, Json(environment_view(&st.store, &st.config, env, &addresses).await?)))
    })
    .await
}

/// `GET /api/v1/environments/{id}`
#[utoipa::path(
    get,
    path = "/api/v1/environments/{id}",
    tag = "environments",
    operation_id = "getEnvironment",
    summary = "Get an environment",
    params(("id" = String, Path, description = "Environment id or name.")),
    responses(
        (status = 200, description = "The environment.", body = EnvironmentView),
        (status = 404, description = "No such environment.", body = ApiErrorBody),
    ),
)]
pub async fn get(State(st): State<AppState>, ApiPath(id): ApiPath<String>) -> ApiResult<Json<EnvironmentView>> {
    let env = st.store.require_environment(&id).await?;
    let addresses = addresses(&st).await;
    Ok(Json(environment_view(&st.store, &st.config, env, &addresses).await?))
}

/// `DELETE /api/v1/environments/{id}`
#[utoipa::path(
    delete,
    path = "/api/v1/environments/{id}",
    tag = "environments",
    operation_id = "deleteEnvironment",
    summary = "Delete an environment",
    description = "Removes an empty environment and its private network. Refused while it holds services or datastores (the message lists them: delete them first), and always for `production`.",
    params(("id" = String, Path, description = "Environment id or name.")),
    responses(
        (status = 204, description = "Deleted."),
        (status = 404, description = "No such environment.", body = ApiErrorBody),
        (status = 409, description = "It is `production`, it still holds services or datastores (the message lists them), or containers are still attached to its network.", body = ApiErrorBody),
        (status = 502, description = "Docker could not remove its network (the environment is kept).", body = ApiErrorBody),
    ),
)]
pub async fn delete(State(st): State<AppState>, ApiPath(id): ApiPath<String>) -> ApiResult<StatusCode> {
    locks::detached(async move {
        // Nothing is created in it meanwhile: creating a service or a
        // datastore takes this lock too.
        let _names = locks::names().await;
        let env = st.store.require_environment(&id).await?;
        if env.is_production() {
            return Err(Error::conflict("environment 'production' can't be deleted").into());
        }
        let (services, datastores) = st.store.environment_members(&env.name).await?;
        let held: Vec<String> = services
            .iter()
            .map(|s| format!("service '{}'", s.name))
            .chain(datastores.iter().map(|d| format!("datastore '{}'", d.name)))
            .collect();
        if !held.is_empty() {
            return Err(Error::conflict(format!(
                "environment '{}' still holds {}: delete them first",
                env.name,
                held.join(", ")
            ))
            .into());
        }
        st.engine.delete_environment(&env.name).await?;
        st.store.delete_environment(&env.id).await?;
        tracing::info!(environment = %env.name, "deleted environment");
        Ok(StatusCode::NO_CONTENT)
    })
    .await
}
