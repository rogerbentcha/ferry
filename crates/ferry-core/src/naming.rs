//! Naming conventions for Docker resources. Every crate that creates or looks
//! up Docker objects MUST go through [`Naming`] so they agree.
//!
//! Services and datastores are named by their qualified name
//! ([`crate::Service::qualified_name`], [`crate::Datastore::qualified_name`]):
//! `api` in `production`, `api.staging` elsewhere. A name is unique in its
//! environment only, so the same `api` in two environments gets two images,
//! two sets of containers, two volumes; production keeps the names of a
//! server set up before environments.

use std::collections::BTreeMap;

use crate::ids;

/// Label present on every Ferry-managed container/volume (`"true"`).
pub const LABEL_MANAGED: &str = "ferry.managed";
/// Label holding the owning server's `name_prefix`.
pub const LABEL_INSTANCE: &str = "ferry.instance";
/// Label: `service` | `job` | `datastore`.
pub const LABEL_ROLE: &str = "ferry.role";
/// Label: owning service id.
pub const LABEL_SERVICE: &str = "ferry.service";
/// Label: deploy id a service container belongs to.
pub const LABEL_DEPLOY: &str = "ferry.deploy";
/// Label: job run id.
pub const LABEL_JOB: &str = "ferry.job";
/// Label: datastore id.
pub const LABEL_DATASTORE: &str = "ferry.datastore";

pub const ROLE_SERVICE: &str = "service";
pub const ROLE_JOB: &str = "job";
pub const ROLE_DATASTORE: &str = "datastore";

/// Derives Docker names from the configured prefix (default `ferry`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Naming {
    prefix: String,
}

impl Naming {
    pub fn new(prefix: &str) -> Self {
        Naming { prefix: prefix.to_string() }
    }

    pub fn prefix(&self) -> &str {
        &self.prefix
    }

    /// The user-defined bridge network of the `production` environment
    /// (private networking), which every workload joined before environments.
    pub fn network(&self) -> String {
        self.prefix.clone()
    }

    /// The private network of an environment: [`Naming::network`] for
    /// `production` (an upgrade keeps it), `ferry-env-staging` for the others.
    /// Containers of different environments share no network, so they can't
    /// reach each other by name or by address.
    pub fn environment_network(&self, environment: &str) -> String {
        if environment == crate::models::PRODUCTION {
            self.network()
        } else {
            format!("{}-env-{}", self.prefix, environment)
        }
    }

    /// Repository part of built images: `ferry/web`, `ferry/web.staging`
    /// (`service_name` is the service's qualified name, as below).
    pub fn image_repo(&self, service_name: &str) -> String {
        format!("{}/{}", self.prefix, service_name)
    }

    /// Tag for an image built by a deploy: `ferry/web:dep-…`.
    pub fn image_tag(&self, service_name: &str, deploy_id: &str) -> String {
        format!("{}:{}", self.image_repo(service_name), deploy_id)
    }

    /// Container name for one instance of a deploy: `ferry-web-1a2b3c4d-9f8e7d`,
    /// `ferry-web.staging-1a2b3c4d-9f8e7d`.
    /// `unique` should be random-ish (e.g. `ids::random_secret(6)`).
    pub fn service_container(&self, service_name: &str, deploy_id: &str, unique: &str) -> String {
        format!("{}-{}-{}-{}", self.prefix, service_name, ids::short(deploy_id), unique)
    }

    /// Container name for a job run: `ferry-job-web-1a2b3c4d`.
    pub fn job_container(&self, service_name: &str, job_id: &str) -> String {
        format!("{}-job-{}-{}", self.prefix, service_name, ids::short(job_id))
    }

    /// Container name for a datastore: `ferry-ds-db`, `ferry-ds-db.staging`
    /// (`datastore_name` is its qualified name, as below).
    pub fn datastore_container(&self, datastore_name: &str) -> String {
        format!("{}-ds-{}", self.prefix, datastore_name)
    }

    /// Data volume of a datastore: `ferry-ds-db-data`, `ferry-ds-db.staging-data`.
    pub fn datastore_volume(&self, datastore_name: &str) -> String {
        format!("{}-ds-{}-data", self.prefix, datastore_name)
    }

    /// Persistent disk volume of a service: `ferry-svc-srv-…-disk`.
    pub fn service_volume(&self, service_id: &str) -> String {
        format!("{}-svc-{}-disk", self.prefix, service_id)
    }

    /// Labels common to every managed resource.
    pub fn base_labels(&self, role: &str) -> BTreeMap<String, String> {
        let mut m = BTreeMap::new();
        m.insert(LABEL_MANAGED.to_string(), "true".to_string());
        m.insert(LABEL_INSTANCE.to_string(), self.prefix.clone());
        m.insert(LABEL_ROLE.to_string(), role.to_string());
        m
    }

    /// Labels for a service instance container.
    pub fn service_labels(&self, service_id: &str, deploy_id: &str) -> BTreeMap<String, String> {
        let mut m = self.base_labels(ROLE_SERVICE);
        m.insert(LABEL_SERVICE.to_string(), service_id.to_string());
        m.insert(LABEL_DEPLOY.to_string(), deploy_id.to_string());
        m
    }

    /// Labels for a job container.
    pub fn job_labels(&self, service_id: &str, job_id: &str) -> BTreeMap<String, String> {
        let mut m = self.base_labels(ROLE_JOB);
        m.insert(LABEL_SERVICE.to_string(), service_id.to_string());
        m.insert(LABEL_JOB.to_string(), job_id.to_string());
        m
    }

    /// Labels for a datastore container / volume.
    pub fn datastore_labels(&self, datastore_id: &str) -> BTreeMap<String, String> {
        let mut m = self.base_labels(ROLE_DATASTORE);
        m.insert(LABEL_DATASTORE.to_string(), datastore_id.to_string());
        m
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        let n = Naming::new("ferry");
        assert_eq!(n.image_tag("web", "dep-0123456789abcdef0123"), "ferry/web:dep-0123456789abcdef0123");
        assert_eq!(n.service_container("web", "dep-0123456789abcdef0123", "xyz"), "ferry-web-cdef0123-xyz");
        assert_eq!(n.datastore_volume("db"), "ferry-ds-db-data");
        assert_eq!(n.service_labels("srv-1", "dep-1")[LABEL_INSTANCE], "ferry");
        assert_eq!(n.environment_network("production"), "ferry");
        assert_eq!(n.environment_network("staging"), "ferry-env-staging");
    }

    /// The same name in two environments, and names that a plain `-` join
    /// would mix up (`db` of `qa-1` and `1-db` of `qa`, `db` of `staging`
    /// and `staging-db` of production), never share a Docker name.
    #[test]
    fn names_never_clash_across_environments() {
        use crate::models::qualified_name;
        let n = Naming::new("ferry");
        let pairs = [
            ("production", "db"),
            ("staging", "db"),
            ("production", "staging-db"),
            ("production", "db-staging"),
            ("qa-1", "db"),
            ("qa", "1-db"),
            ("qa", "db"),
        ];
        let deploy = "dep-0123456789abcdef0123";
        let mut seen = std::collections::HashSet::new();
        for (environment, name) in pairs {
            let q = qualified_name(name, environment);
            for docker_name in [
                n.image_repo(&q),
                n.service_container(&q, deploy, "xyz"),
                n.job_container(&q, deploy),
                n.datastore_container(&q),
                n.datastore_volume(&q),
            ] {
                assert!(seen.insert(docker_name.clone()), "{docker_name} ({name} of {environment}) is taken twice");
            }
        }
        // Production keeps the names of a server set up before environments.
        assert_eq!(n.datastore_container(&qualified_name("db", "production")), "ferry-ds-db");
        assert_eq!(n.datastore_volume(&qualified_name("db", "staging")), "ferry-ds-db.staging-data");
        assert_eq!(n.image_tag(&qualified_name("api", "staging"), deploy), format!("ferry/api.staging:{deploy}"));
    }
}
