//! The service or datastore a route names: an id, or a name in the
//! environment of `?environment=` (omitted = `production`). Names are unique
//! in an environment only (DESIGN.md §23), so one name may stand for a
//! service of each environment.

use ferry_core::{Datastore, Error, PRODUCTION, Service, Store};

use crate::error::{ApiError, ApiResult};
use crate::extract::ScopeQuery;

/// The name of the environment `scope` names, `None` when it names none.
/// 404 when there is no such environment.
pub async fn environment(store: &Store, scope: &ScopeQuery) -> ApiResult<Option<String>> {
    match scope.environment.as_deref().map(str::trim).filter(|e| !e.is_empty()) {
        None => Ok(None),
        Some(e) => Ok(Some(store.require_environment(e).await?.name)),
    }
}

/// The service `id_or_name` of the route.
pub async fn service(store: &Store, id_or_name: &str, scope: &ScopeQuery) -> ApiResult<Service> {
    let environment = environment(store, scope).await?;
    match store.find_service_in(id_or_name, environment.as_deref()).await? {
        Some(s) => Ok(s),
        None => Err(not_found(store, "service", id_or_name, environment.as_deref()).await?),
    }
}

/// The datastore `id_or_name` of the route.
pub async fn datastore(store: &Store, id_or_name: &str, scope: &ScopeQuery) -> ApiResult<Datastore> {
    let environment = environment(store, scope).await?;
    match store.find_datastore_in(id_or_name, environment.as_deref()).await? {
        Some(d) => Ok(d),
        None => Err(not_found(store, "datastore", id_or_name, environment.as_deref()).await?),
    }
}

/// The 404 of a lookup. A name looked up in `production` because no
/// environment was named, while other environments use it, says which: the
/// caller most likely forgot the environment.
async fn not_found(store: &Store, kind: &str, id_or_name: &str, environment: Option<&str>) -> ApiResult<ApiError> {
    let elsewhere: Vec<String> = if environment.is_some() {
        Vec::new()
    } else if kind == "service" {
        store.list_services().await?.into_iter().filter(|s| s.name == id_or_name).map(|s| s.environment).collect()
    } else {
        store.list_datastores().await?.into_iter().filter(|d| d.name == id_or_name).map(|d| d.environment).collect()
    };
    let Some(first) = elsewhere.first() else {
        return Ok(Error::NotFound(match environment.filter(|e| *e != PRODUCTION) {
            Some(e) => format!("{kind} '{id_or_name}' in environment '{e}'"),
            None => format!("{kind} '{id_or_name}'"),
        })
        .into());
    };
    let named = elsewhere.iter().map(|e| format!("'{e}'")).collect::<Vec<_>>().join(", ");
    let has = if elsewhere.len() == 1 {
        format!("environment {named} has one")
    } else {
        format!("environments {named} have one")
    };
    Ok(ApiError::not_found(format!(
        "{kind} '{id_or_name}' not found in environment 'production' ({has}): name its environment, e.g. --environment {first} in the CLI or ?environment={first} in the API"
    )))
}
