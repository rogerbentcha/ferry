//! Environments (DESIGN.md §3): every environment has a private network of
//! its own, `Naming::environment_network`. Its instances, job runs and
//! datastores join it and nothing else, so they reach each other by name and
//! never a container of another environment (Docker isolates bridge
//! networks from each other). `production` keeps the network every workload
//! joined before environments existed.

use ferry_core::{Error, PRODUCTION, Result};
use ferry_docker::{ContainerInfo, ContainerSpec};
use tracing::{info, warn};

use crate::state::Inner;

/// Make the network of `environment` if it doesn't exist.
pub(crate) async fn ensure_network(inner: &Inner, environment: &str) -> Result<()> {
    let network = inner.naming.environment_network(environment);
    inner.docker.ensure_network(&network, &inner.naming.base_labels("network")).await
}

/// At engine start: the networks of every environment (Docker may have lost
/// them, e.g. after a reset of Docker Desktop). One that can't be made is a
/// warning, not a failed start: [`run_container`] makes it when a container
/// of that environment starts. (Production's network is made before, and
/// its failure stops the start, as before environments.)
pub(crate) async fn ensure_networks(inner: &Inner) -> Result<()> {
    for environment in inner.store.list_environments().await? {
        // `--dashboard-host` is set at startup: the API can't refuse it.
        if let Some(host) = hidden_dashboard_host(inner, &environment.name) {
            warn!(
                environment = %environment.name,
                "the DNS record of environment '{0}' (*.{0}.<domain>) hides the dashboard's host {host}: \
                 give the dashboard another host, or delete the environment",
                environment.name
            );
        }
        if let Err(e) = ensure_network(inner, &environment.name).await {
            warn!(environment = %environment.name, "cannot make the network of the environment (retried when a container of it starts): {e}");
        }
    }
    Ok(())
}

/// The dashboard's host when it is `<environment>.<domain>` under a domain
/// of the server: the environment's `*.<environment>.<domain>` hides it.
fn hidden_dashboard_host(inner: &Inner, environment: &str) -> Option<String> {
    let dashboard = inner.config.dashboard_host.as_deref()?.trim().trim_end_matches('.').to_ascii_lowercase();
    (environment != PRODUCTION)
        .then(|| inner.config.claimed_domains())?
        .into_iter()
        .map(|d| format!("{environment}.{d}"))
        .find(|h| *h == dashboard)
}

/// Remove the network of an environment about to be deleted.
pub(crate) async fn remove_network(inner: &Inner, environment: &str) -> Result<()> {
    if environment == PRODUCTION {
        return Err(Error::conflict("environment 'production' can't be deleted"));
    }
    inner.docker.remove_network(&inner.naming.environment_network(environment)).await?;
    info!(environment, "removed the network of an environment");
    Ok(())
}

/// Start a container. When Docker says its network is missing (removed
/// behind Ferry's back, or not made at engine start), make it and try once
/// more.
pub(crate) async fn run_container(inner: &Inner, spec: &ContainerSpec) -> Result<ContainerInfo> {
    match inner.docker.run_container(spec).await {
        Err(Error::NotFound(what))
            if spec.network.as_deref().is_some_and(|n| !n.is_empty() && what == format!("network '{n}'")) =>
        {
            let network = spec.network.as_deref().unwrap_or_default();
            info!(network, container = %spec.name, "the network was missing: creating it");
            inner.docker.ensure_network(network, &inner.naming.base_labels("network")).await?;
            inner.docker.run_container(spec).await
        }
        other => other,
    }
}
