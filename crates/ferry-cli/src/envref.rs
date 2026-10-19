//! Checks for `${{kind.name.property}}` references in env var values, so a
//! reference that can never resolve is caught when it is set instead of
//! failing the restart a second later.
//!
//! Both checks run `ferry_core::env::resolve_value`, the resolver the server
//! uses at container start, so the CLI accepts exactly what it accepts:
//! * [`check_syntax`] (offline): the shape, kind and property of every
//!   reference, against a context in which every referenced name exists;
//! * [`unresolved`]: against the server's real datastores and services
//!   (unknown targets, services without a known port, …).

use ferry_core::dto::{DatastoreView, ServiceView};
use ferry_core::env::{RefContext, ServiceRef, check_environment, resolve_value};
use ferry_core::{Datastore, DatastoreKind, EnvVar, Service};

/// Opening marker of a reference.
const OPEN: &str = "${{";

/// The `name` part of every well-formed `${{kind.name.property}}` in `value`.
fn referenced_names(value: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut rest = value;
    while let Some(start) = rest.find(OPEN) {
        let after = &rest[start + OPEN.len()..];
        let Some(end) = after.find("}}") else { break };
        if let [_, name, _] = after[..end].split('.').map(str::trim).collect::<Vec<_>>().as_slice() {
            names.push((*name).to_string());
        }
        rest = &after[end + 2..];
    }
    names
}

/// Can the references in `value` ever resolve? Checks their syntax, kind and
/// property, not whether the named datastore/service exists.
pub fn check_syntax(value: &str) -> Result<(), String> {
    if !value.contains(OPEN) {
        return Ok(());
    }
    let names = referenced_names(value);
    let datastores: Vec<Datastore> = names
        .iter()
        .map(|n| {
            let mut d = Datastore::new(n.clone(), DatastoreKind::Postgres);
            d.host_port = Some(1);
            d
        })
        .collect();
    let services: Vec<ServiceRef> = names
        .iter()
        .map(|n| ServiceRef { name: n.clone(), port: Some(1), public_url: Some(format!("http://{n}")) })
        .collect();
    let ctx = RefContext { datastores: &datastores, services: &services, advertise_host: "localhost" };
    resolve_value(value, &ctx).map(|_| ()).map_err(|e| e.to_string())
}

/// Variables whose references don't resolve against the server's current
/// datastores and services, with the reason. With the `environment` of the
/// service they are set on, they resolve inside it, as on the server: a
/// reference to another environment is one too.
pub fn unresolved(
    vars: &[EnvVar],
    datastores: &[DatastoreView],
    services: &[ServiceView],
    environment: Option<&str>,
) -> Vec<(String, String)> {
    let inside = |e: &str| environment.is_none_or(|env| e == env);
    let all_datastores: Vec<Datastore> = datastores.iter().map(|d| d.datastore.clone()).collect();
    let all_services: Vec<Service> = services.iter().map(|v| v.service.clone()).collect();
    let datastores: Vec<Datastore> = all_datastores.iter().filter(|d| inside(&d.environment)).cloned().collect();
    let services: Vec<ServiceRef> = services
        .iter()
        .filter(|v| inside(&v.service.environment))
        .map(|v| ServiceRef {
            name: v.service.name.clone(),
            port: v.internal_port.or(v.service.port),
            public_url: v.url.clone(),
        })
        .collect();
    let ctx = RefContext { datastores: &datastores, services: &services, advertise_host: "localhost" };
    vars.iter()
        .filter(|v| v.value.contains(OPEN))
        .filter_map(|v| {
            let elsewhere = environment
                .and_then(|env| check_environment(std::slice::from_ref(v), env, &all_datastores, &all_services).err());
            let why = match elsewhere {
                // Without the server's "env var KEY: " prefix: the caller names the key.
                Some(e) => e.to_string().split_once(": ").map(|(_, why)| why.to_string()).unwrap_or_default(),
                None => resolve_value(&v.value, &ctx).err()?.to_string(),
            };
            Some((v.key.clone(), why))
        })
        .collect()
}

/// Does any variable use a reference?
pub fn any_reference(vars: &[EnvVar]) -> bool {
    vars.iter().any(|v| v.value.contains(OPEN))
}

#[cfg(test)]
mod tests {
    use super::*;
    use ferry_core::{DatastoreStatus, Service, ServiceState, ServiceType};

    #[test]
    fn syntax_errors_are_caught_offline() {
        for ok in [
            "plain value",
            "${{datastore.main-db.connectionString}}",
            "postgres://${{ db.pg.user }}:${{db.pg.password}}@${{db.pg.host}}/x",
            "${{service.api.hostport}}",
            "http://${{svc.api.host}}:${{svc.api.port}}/v1",
            "${{ service.web.url }}",
            "$HOME and ${PATH} and {{mustache}}",
        ] {
            assert_eq!(check_syntax(ok), Ok(()), "{ok}");
        }
        for (bad, why) in [
            ("Hello ${{ name }}", "expected ${{kind.name.property}}"),
            ("${{datastore.pg}}", "expected ${{kind.name.property}}"),
            ("${{bucket.pg.url}}", "unknown reference kind"),
            ("${{datastore.pg.nope}}", "unknown datastore property"),
            ("${{service.api.password}}", "unknown service property"),
            ("${{service.api.url", "unterminated"),
        ] {
            let err = check_syntax(bad).unwrap_err();
            assert!(err.contains(why), "{bad}: {err}");
        }
    }

    fn service(name: &str, port: Option<u16>) -> ServiceView {
        let mut service = Service::new(name, ServiceType::PrivateService);
        service.id = format!("srv-{name}");
        ServiceView {
            deploy_hook_path: String::new(),
            url: None,
            hosts: vec![],
            internal_host: name.into(),
            internal_port: port,
            env_groups: vec![],
            latest_deploy: None,
            state: ServiceState::Live,
            service,
        }
    }

    #[test]
    fn unknown_targets_are_reported() {
        let mut pg = Datastore::new("pg", DatastoreKind::Postgres);
        pg.status = DatastoreStatus::Available;
        let pg = DatastoreView {
            internal_host: "pg".into(),
            internal_port: 5432,
            internal_url: pg.internal_url(),
            external_url: None,
            datastore: pg,
        };
        let vars = vec![
            EnvVar::new("PLAIN", "x"),
            EnvVar::new("DB", "${{datastore.pg.connectionString}}"),
            EnvVar::new("BAD", "${{datastore.pgx.connectionString}}"),
            EnvVar::new("API", "${{service.api.hostport}}"),
            EnvVar::new("NEW", "${{service.fresh.hostport}}"),
        ];
        let services = [service("api", Some(8080)), service("fresh", None)];
        let problems = unresolved(&vars, &[pg], &services, None);
        let keys: Vec<&str> = problems.iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, vec!["BAD", "NEW"], "{problems:?}");
        assert!(problems[0].1.contains("unknown datastore 'pgx'"), "{problems:?}");
        assert!(problems[1].1.contains("no known port"), "{problems:?}");
        assert!(any_reference(&vars));
        assert!(!any_reference(&vars[..1]));

        // Set on a service of staging: production's datastore is not reachable.
        let mut shop = service("shop", Some(3000));
        shop.service.environment = "staging".into();
        let pg = DatastoreView {
            internal_host: "pg".into(),
            internal_port: 5432,
            internal_url: String::new(),
            external_url: None,
            datastore: Datastore::new("pg", DatastoreKind::Postgres),
        };
        let vars = [EnvVar::new("DB", "${{datastore.pg.connectionString}}"), EnvVar::new("SHOP", "${{svc.shop.host}}")];
        let problems = unresolved(&vars, &[pg], &[shop], Some("staging"));
        assert_eq!(problems.len(), 1, "{problems:?}");
        assert_eq!(problems[0].0, "DB");
        assert!(problems[0].1.starts_with("datastore 'pg' is not in environment 'staging'"), "{problems:?}");
    }
}
