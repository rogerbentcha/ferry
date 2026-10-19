//! Validation rules shared by the REST handlers and blueprints.

use ferry_core::{Config, EnvVar, Environment, Error, PRODUCTION, Result, Service, Store, env, validate};

/// A service of `production` named like another environment (`staging`)
/// would lose its hostname `staging.<domain>` once the environment's DNS
/// record `*.staging.<domain>` exists: that name then exists in DNS with no
/// record of its own, and the domain's `*.<domain>` no longer answers for it
/// (RFC 4592). So the two never share a name, and neither do an environment
/// and the dashboard's host (`ferry.<domain>`). Call it under the names and
/// domain locks.
pub async fn check_environment_name_free(store: &Store, config: &Config, environment: &str) -> Result<()> {
    if let Some(host) =
        config.claimed_domains().iter().map(|d| format!("{environment}.{d}")).find(|h| is_dashboard_host(config, h))
    {
        return Err(Error::conflict(format!(
            "'{environment}' would hide the Ferry dashboard: its services are served at <service>.{environment}.<domain>, and that DNS record would hide {host}; choose another name"
        )));
    }
    match store.find_service(environment).await? {
        Some(s) if s.environment == PRODUCTION && s.name == environment => Err(Error::conflict(format!(
            "'{environment}' is the name of service '{environment}' of production: an environment of that name would hide its hostname {environment}.<domain> (its services are served at <service>.{environment}.<domain>); choose another name"
        ))),
        _ => Ok(()),
    }
}

/// The other side of [`check_environment_name_free`]: a new service of
/// `production` can't take the name of another environment.
pub async fn check_service_name_free(store: &Store, svc: &Service) -> Result<()> {
    if svc.environment != PRODUCTION || svc.name == PRODUCTION {
        return Ok(());
    }
    match store.find_environment(&svc.name).await? {
        Some(e) if e.name == svc.name => Err(Error::conflict(format!(
            "name '{0}' is the name of an environment: its services are served at <service>.{0}.<domain>, and that DNS record would hide {0}.<domain>; choose another name",
            svc.name
        ))),
        _ => Ok(()),
    }
}

/// The environment a new service or datastore goes to: `requested` (an id
/// or a name), else `production`. Call it under the names lock, which
/// deleting an environment takes too.
pub async fn environment_for(store: &Store, requested: Option<&str>) -> Result<String> {
    match requested.map(str::trim).filter(|e| !e.is_empty()) {
        None => Ok(PRODUCTION.to_string()),
        Some(e) => match store.find_environment(e).await? {
            Some(env) => Ok(env.name),
            None => Err(Error::invalid(format!(
                "environment '{e}' not found (create it first: ferry environments create {e})"
            ))),
        },
    }
}

/// Size of a set of variables as counted by [`validate::env_vars`].
fn env_bytes(vars: &[EnvVar]) -> usize {
    vars.iter().map(|v| v.key.len() + v.value.len() + 2).sum()
}

/// The combined environment of the service `name` must stay within
/// [`validate::MAX_ENV_TOTAL_BYTES`]: it becomes the build args of every
/// build and the container environment. `layers` are the variables of its
/// linked env groups (in link order) followed by its own.
pub fn effective_env_size(name: &str, layers: &[Vec<EnvVar>]) -> Result<()> {
    let total = env_bytes(&env::merge(layers));
    if total > validate::MAX_ENV_TOTAL_BYTES {
        return Err(Error::invalid(format!(
            "the environment of service '{name}' (its own variables plus linked env groups) would total {total} bytes; the maximum is {} bytes",
            validate::MAX_ENV_TOTAL_BYTES
        )));
    }
    Ok(())
}

/// A pending env change, for [`check_service_env`].
#[derive(Debug, Default, Clone, Copy)]
pub struct EnvChange<'a> {
    /// The service's own variables after the change.
    pub own: Option<&'a [EnvVar]>,
    /// The variables of one linked env group after the change: (group id, vars).
    pub group: Option<(&'a str, &'a [EnvVar])>,
    /// The variables of an env group about to be linked (appended last).
    pub link: Option<&'a [EnvVar]>,
}

/// Check the combined environment `svc` would have after `change`.
pub async fn check_service_env(store: &Store, svc: &Service, change: EnvChange<'_>) -> Result<()> {
    let mut layers = Vec::new();
    for g in store.service_env_groups(&svc.id).await? {
        match change.group {
            Some((id, vars)) if id == g.id => layers.push(vars.to_vec()),
            _ => layers.push(store.list_env(&g.id).await?),
        }
    }
    if let Some(vars) = change.link {
        layers.push(vars.to_vec());
    }
    layers.push(match change.own {
        Some(vars) => vars.to_vec(),
        None => store.list_env(&svc.id).await?,
    });
    effective_env_size(&svc.name, &layers)
}

/// `current` with `unset` removed, then `set` upserted — the result of
/// `Store::patch_env`, computed up front so it can be validated.
pub fn patched_env(current: &[EnvVar], set: &[EnvVar], unset: &[String]) -> Vec<EnvVar> {
    let mut out: Vec<EnvVar> = current.iter().filter(|v| !unset.contains(&v.key)).cloned().collect();
    for v in set {
        match out.iter_mut().find(|e| e.key == v.key) {
            Some(e) => e.value = v.value.clone(),
            None => out.push(v.clone()),
        }
    }
    out
}

/// The Dockerfile must stay inside the source directory: `dockerfile_path`
/// may use `..` (it is relative to `root_dir`), but `root_dir` joined with it
/// must not climb above the checkout. (The builder additionally rejects
/// symlinks that point outside.)
pub fn source_paths(svc: &Service) -> Result<()> {
    let Some(dockerfile) = &svc.dockerfile_path else { return Ok(()) };
    let segments = svc.root_dir.iter().flat_map(|r| r.split(['/', '\\'])).chain(dockerfile.split(['/', '\\']));
    let mut depth: usize = 0;
    for seg in segments {
        match seg {
            "" | "." => {}
            ".." => {
                depth = depth.checked_sub(1).ok_or_else(|| {
                    Error::invalid(format!(
                        "invalid dockerfile_path '{dockerfile}': it points outside the source directory (it is relative to the root directory '{}')",
                        svc.root_dir.as_deref().unwrap_or(".")
                    ))
                })?;
            }
            _ => depth += 1,
        }
    }
    Ok(())
}

/// What a `${{kind.name.property}}` reference points at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefKind {
    Datastore,
    Service,
}

/// The `(kind, name)` of every reference in an env var value (same syntax
/// and kind aliases as `ferry_core::env`).
pub fn references(value: &str) -> Vec<(RefKind, String)> {
    let mut out = Vec::new();
    let mut rest = value;
    while let Some(start) = rest.find("${{") {
        let after = &rest[start + 3..];
        let Some(end) = after.find("}}") else { break };
        let parts: Vec<&str> = after[..end].trim().split('.').map(str::trim).collect();
        if let [kind, name, _property] = parts.as_slice() {
            let kind = match kind.to_ascii_lowercase().as_str() {
                "datastore" | "db" | "database" => Some(RefKind::Datastore),
                "service" | "svc" => Some(RefKind::Service),
                _ => None,
            };
            if let Some(kind) = kind {
                out.push((kind, name.to_string()));
            }
        }
        rest = &after[end + 2..];
    }
    out
}

/// Services of `environment` (other than `exclude`) whose environment
/// references the datastore or service `name` of that environment, as
/// `service 'web' (DATABASE_URL, ...)`. Deleting the target would make every
/// later deploy / restart of them fail. Another environment's references to
/// the same name resolve to its own.
pub async fn referencing_services(
    store: &Store,
    kind: RefKind,
    name: &str,
    environment: &str,
    exclude: Option<&str>,
) -> Result<Vec<String>> {
    let mut out = Vec::new();
    for svc in store.environment_members(environment).await?.0 {
        if exclude == Some(svc.id.as_str()) {
            continue;
        }
        let keys: Vec<String> = store
            .effective_env(&svc.id)
            .await?
            .into_iter()
            .filter(|v| references(&v.value).iter().any(|(k, n)| *k == kind && n == name))
            .map(|v| v.key)
            .collect();
        if !keys.is_empty() {
            out.push(format!("service '{}' ({})", svc.name, keys.join(", ")));
        }
    }
    Ok(out)
}

/// Datastore version / image tag prefix: digits and dots only (`16`, `7.2`).
pub fn datastore_version(v: &str) -> Result<()> {
    let ok = !v.is_empty()
        && v.len() <= 16
        && v.bytes().all(|b| b.is_ascii_digit() || b == b'.')
        && !v.starts_with('.')
        && !v.ends_with('.')
        && !v.contains("..");
    if ok {
        Ok(())
    } else {
        Err(Error::invalid(format!("invalid version '{v}': use digits and dots only (e.g. 16 or 7.2)")))
    }
}

/// Postgres database / user names: 1–63 chars of `[A-Za-z0-9_]`, not
/// starting with a digit (they end up in connection URLs and env vars).
pub fn pg_identifier(what: &str, v: &str) -> Result<()> {
    let ok = !v.is_empty()
        && v.len() <= 63
        && !v.as_bytes()[0].is_ascii_digit()
        && v.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_');
    if ok {
        Ok(())
    } else {
        Err(Error::invalid(format!("invalid {what} '{v}': use 1-63 letters, digits or '_', not starting with a digit")))
    }
}

fn is_dashboard_host(config: &Config, host: &str) -> bool {
    config.dashboard_host.as_deref().is_some_and(|h| h.trim().trim_end_matches('.').eq_ignore_ascii_case(host))
}

/// Check that custom `domains` (already normalized) of the service `own`
/// are free: not the dashboard host, not a default host of any service —
/// `<name>.<domain>` (`<name>.<environment>.<domain>` outside production)
/// under every domain of the server, served or still waiting for its DNS
/// (§21) — and not a custom domain of one of the `others` (every service
/// except this one).
pub fn check_domains<'a>(
    config: &Config,
    others: impl IntoIterator<Item = &'a Service> + Clone,
    own: &Service,
    domains: &[String],
) -> Result<()> {
    let own_name = own.name.as_str();
    let own_hosts = config.claimed_hosts(&own.host_label());
    for d in domains {
        if is_dashboard_host(config, d) {
            return Err(Error::conflict(format!("domain '{d}' is reserved for the Ferry dashboard")));
        }
        if own_hosts.contains(d) {
            return Err(Error::conflict(format!("domain '{d}' is already the default host of service '{own_name}'")));
        }
        for s in others.clone() {
            if config.claimed_hosts(&s.host_label()).contains(d) {
                return Err(Error::conflict(format!("domain '{d}' is the default host of service '{}'", s.name)));
            }
            if s.custom_domains.iter().any(|c| c.eq_ignore_ascii_case(d)) {
                return Err(Error::conflict(format!("domain '{d}' is already used by service '{}'", s.name)));
            }
        }
    }
    Ok(())
}

/// Check that the default hosts of a new service `svc` (one under every
/// domain of the server) aren't already claimed as a custom domain by
/// another service (or by the dashboard).
pub fn check_default_host_free<'a>(
    config: &Config,
    others: impl IntoIterator<Item = &'a Service>,
    svc: &Service,
) -> Result<()> {
    let hosts = config.claimed_hosts(&svc.host_label());
    if let Some(host) = hosts.iter().find(|h| is_dashboard_host(config, h)) {
        return Err(Error::conflict(format!("host '{host}' is reserved for the Ferry dashboard; choose another name")));
    }
    for s in others {
        if let Some(host) = hosts.iter().find(|h| s.custom_domains.iter().any(|c| c.eq_ignore_ascii_case(h))) {
            return Err(Error::conflict(format!(
                "host '{host}' is already used as a custom domain by service '{}'",
                s.name
            )));
        }
    }
    Ok(())
}

/// Check that a domain to connect doesn't put an environment other than
/// production at the dashboard's host: `*.staging.<domain>` would hide
/// `staging.<domain>` (see [`check_environment_name_free`]).
pub fn check_domain_free_of_environments(config: &Config, environments: &[Environment], domain: &str) -> Result<()> {
    for e in environments.iter().filter(|e| !e.is_production()) {
        let host = format!("{}.{domain}", e.name);
        if is_dashboard_host(config, &host) {
            return Err(Error::conflict(format!(
                "with '{domain}', the services of environment '{}' would be served at <service>.{host}, and that DNS record would hide the Ferry dashboard at {host}",
                e.name
            )));
        }
    }
    Ok(())
}

/// Check that a domain to connect takes no hostname that is in use: with it
/// every service is served at `<service>.<domain>`, which must not be the
/// dashboard's host or the custom domain of another service.
pub fn check_domain_free(config: &Config, services: &[Service], domain: &str) -> Result<()> {
    for s in services {
        let host = format!("{}.{domain}", s.host_label());
        if is_dashboard_host(config, &host) {
            return Err(Error::conflict(format!(
                "with '{domain}', service '{}' would be served at '{host}', which is the host of the Ferry dashboard",
                s.name
            )));
        }
        if let Some(other) =
            services.iter().find(|o| o.id != s.id && o.custom_domains.iter().any(|c| c.eq_ignore_ascii_case(&host)))
        {
            return Err(Error::conflict(format!(
                "with '{domain}', service '{}' would be served at '{host}', which is a custom domain of service '{}': remove that custom domain first",
                s.name, other.name
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferry_core::ServiceType;

    #[test]
    fn domain_rules() {
        let cfg = Config::default();
        let mut a = Service::new("a", ServiceType::WebService);
        a.custom_domains = vec!["app.example.com".into()];
        let others = [a];
        let (b, c) = (Service::new("b", ServiceType::WebService), Service::new("c", ServiceType::WebService));
        assert!(check_domains(&cfg, &others, &b, &["new.example.com".into()]).is_ok());
        assert!(matches!(check_domains(&cfg, &others, &b, &["app.example.com".into()]), Err(Error::Conflict(_))));
        assert!(matches!(check_domains(&cfg, &others, &b, &["a.localhost".into()]), Err(Error::Conflict(_))));
        assert!(matches!(check_domains(&cfg, &others, &b, &["b.localhost".into()]), Err(Error::Conflict(_))));
        assert!(matches!(check_domains(&cfg, &others, &b, &["ferry.localhost".into()]), Err(Error::Conflict(_))));
        assert!(check_default_host_free(&cfg, &others, &c).is_ok());
        let mut x = Service::new("x", ServiceType::WebService);
        x.custom_domains = vec!["c.localhost".into()];
        assert!(check_default_host_free(&cfg, &[x], &c).is_err());
    }

    #[test]
    fn hosts_outside_production_carry_the_environment() {
        let cfg = Config::default();
        let shop = Service { environment: "staging".into(), ..Service::new("shop", ServiceType::WebService) };
        let others = [shop.clone()];
        let web = Service::new("web", ServiceType::WebService);
        let e = check_domains(&cfg, &others, &web, &["shop.staging.localhost".into()]).unwrap_err().to_string();
        assert!(e.contains("default host of service 'shop'"), "{e}");
        assert!(check_domains(&cfg, &others, &web, &["shop.localhost".into()]).is_ok());
        // Its own staging host is already its default host.
        assert!(check_domains(&cfg, &[], &shop, &["shop.staging.localhost".into()]).is_err());
        // A domain connected later would put it at `shop.staging.<domain>`.
        let mut site = Service::new("site", ServiceType::WebService);
        site.custom_domains = vec!["shop.staging.example.com".into()];
        let e = check_domain_free(&cfg, &[shop, site], "example.com").unwrap_err().to_string();
        assert!(e.contains("shop.staging.example.com"), "{e}");
    }

    #[test]
    fn environments_never_hide_the_dashboard() {
        let cfg = Config { dashboard_host: Some("ferry.apps.example.com".into()), ..Config::default() };
        let envs = [Environment::new("production"), Environment::new("ferry"), Environment::new("staging")];
        let e = check_domain_free_of_environments(&cfg, &envs, "apps.example.com").unwrap_err().to_string();
        assert!(e.contains("environment 'ferry'") && e.contains("ferry.apps.example.com"), "{e}");
        assert!(
            check_domain_free_of_environments(&cfg, &envs[..1], "apps.example.com").is_ok(),
            "production has no record"
        );
        assert!(check_domain_free_of_environments(&cfg, &envs[2..], "apps.example.com").is_ok());
    }

    #[test]
    fn domain_rules_cover_every_domain_of_the_server() {
        use ferry_core::{Domain, DomainSource};
        let cfg = Config::default();
        // One connected domain that still waits for its DNS: its names are taken already.
        let base = Domain { is_default: true, ..Domain::new("localhost", DomainSource::Config) };
        cfg.domains.replace("localhost", &[base, Domain::new("example.com", DomainSource::Connected)]);
        let mut a = Service::new("a", ServiceType::WebService);
        a.custom_domains = vec!["c.example.com".into()];
        let b = Service::new("b", ServiceType::WebService);
        let c = Service::new("c", ServiceType::WebService);
        let others = [a.clone()];
        for taken in ["a.example.com", "b.example.com", "a.localhost"] {
            assert!(matches!(check_domains(&cfg, &others, &b, &[taken.into()]), Err(Error::Conflict(_))), "{taken}");
        }
        assert!(check_domains(&cfg, &others, &b, &["b.example.org".into()]).is_ok());
        // A new service named `c` would be served where `a` has a custom domain.
        let err = check_default_host_free(&cfg, &others, &c).unwrap_err().to_string();
        assert!(err.contains("c.example.com") && err.contains("'a'"), "{err}");
        // Connecting a domain: no service may land on another one's custom domain...
        let all = [a, b];
        assert!(check_domain_free(&cfg, &all, "example.org").is_ok());
        let mut c = Service::new("c", ServiceType::WebService);
        c.custom_domains = vec!["c.example.com".into()];
        let err = check_domain_free(&cfg, &[all[0].clone(), c.clone()], "example.com").unwrap_err().to_string();
        assert!(err.contains("'c.example.com'") && err.contains("service 'a'"), "{err}");
        // ...but its own custom domain under it is fine.
        assert!(check_domain_free(&cfg, std::slice::from_ref(&c), "example.com").is_ok());
        // ...nor on the dashboard's host.
        let dashboard = Config { dashboard_host: Some("c.example.net".into()), ..Config::default() };
        let err = check_domain_free(&dashboard, std::slice::from_ref(&c), "example.net").unwrap_err().to_string();
        assert!(err.contains("'c.example.net'") && err.contains("dashboard"), "{err}");
    }

    #[test]
    fn env_sizes_and_patches() {
        let big = "x".repeat(validate::MAX_ENV_VALUE_BYTES - 100);
        let group: Vec<EnvVar> = (0..3).map(|i| EnvVar::new(format!("G{i}"), big.clone())).collect();
        let own: Vec<EnvVar> = (0..3).map(|i| EnvVar::new(format!("O{i}"), big.clone())).collect();
        assert!(effective_env_size("web", &[group.clone(), own.clone()]).is_ok());
        let group2: Vec<EnvVar> = (0..3).map(|i| EnvVar::new(format!("H{i}"), big.clone())).collect();
        let err = effective_env_size("web", &[group.clone(), group2, own]).unwrap_err().to_string();
        assert!(err.contains("service 'web'") && err.contains("maximum"), "{err}");
        // overridden keys count once
        assert!(effective_env_size("web", &[group.clone(), group.clone(), group]).is_ok());

        let cur = vec![EnvVar::new("A", "1"), EnvVar::new("B", "2")];
        let p = patched_env(&cur, &[EnvVar::new("B", "3"), EnvVar::new("C", "4")], &["A".into(), "C".into()]);
        assert_eq!(p, vec![EnvVar::new("B", "3"), EnvVar::new("C", "4")]);
    }

    #[test]
    fn dockerfile_must_stay_in_the_checkout() {
        let mut s = Service::new("web", ferry_core::ServiceType::WebService);
        for ok in [None, Some("Dockerfile"), Some("./docker/Dockerfile"), Some("a/../Dockerfile")] {
            s.dockerfile_path = ok.map(String::from);
            assert!(source_paths(&s).is_ok(), "{ok:?}");
        }
        s.dockerfile_path = Some("../../../../../etc/hosts".into());
        let err = source_paths(&s).unwrap_err().to_string();
        assert!(err.contains("outside the source directory"), "{err}");
        s.dockerfile_path = Some("../Dockerfile".into());
        assert!(source_paths(&s).is_err());
        s.root_dir = Some("services/api".into());
        assert!(source_paths(&s).is_ok());
        s.dockerfile_path = Some("../../../Dockerfile".into());
        assert!(source_paths(&s).is_err());
    }

    #[test]
    fn reference_scanning() {
        let refs = references(
            "postgres://${{ datastore.db.user }}@${{db.db.host}} ${{svc.api.hostport}} ${{bogus.x.y}} ${{broken",
        );
        assert_eq!(
            refs,
            vec![
                (RefKind::Datastore, "db".to_string()),
                (RefKind::Datastore, "db".to_string()),
                (RefKind::Service, "api".to_string())
            ]
        );
        assert!(references("plain value").is_empty());
    }

    #[test]
    fn datastore_rules() {
        assert!(datastore_version("16").is_ok());
        assert!(datastore_version("7.2").is_ok());
        for bad in ["", "16-alpine", "latest", ".1", "1.", "1..2"] {
            assert!(datastore_version(bad).is_err(), "{bad}");
        }
        assert!(pg_identifier("user", "app_user").is_ok());
        assert!(pg_identifier("user", "1app").is_err());
        assert!(pg_identifier("user", "app-user").is_err());
        assert!(pg_identifier("user", "").is_err());
    }
}
