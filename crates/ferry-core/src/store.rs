//! SQLite persistence. [`Store`] is cheap to clone (it wraps a connection pool)
//! and safe to share between the API and the engine.

use std::path::Path;
use std::str::FromStr;
use std::time::Duration;

use chrono::{DateTime, SecondsFormat, Utc};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteRow, SqliteSynchronous};
use sqlx::{Row, SqlitePool};

use crate::config::Config;
use crate::dto::{ContainerKind, ContainerMetrics, ContainerPoint, ContainerSeries, HostPoint};
use crate::env::{self, ServiceRef};
use crate::models::*;
use crate::requests::{LATENCY_BOUNDS_MS, RequestCounts, TargetRequests};
use crate::{Error, Result};

const MIGRATIONS: &[&str] = &[
    include_str!("../migrations/0001_init.sql"),
    include_str!("../migrations/0002_resource_limits.sql"),
    include_str!("../migrations/0003_git_connections.sql"),
    include_str!("../migrations/0004_git_authorization.sql"),
    include_str!("../migrations/0005_accounts.sql"),
    include_str!("../migrations/0006_domains.sql"),
    include_str!("../migrations/0007_metrics.sql"),
    include_str!("../migrations/0008_requests.sql"),
    include_str!("../migrations/0009_environments.sql"),
    include_str!("../migrations/0010_names_per_environment.sql"),
];

/// Handle to the Ferry database.
#[derive(Clone, Debug)]
pub struct Store {
    pool: SqlitePool,
}

// ---------------------------------------------------------------------------
// encoding helpers

fn ts(dt: &DateTime<Utc>) -> String {
    dt.to_rfc3339_opts(SecondsFormat::Micros, true)
}

fn ts_opt(dt: &Option<DateTime<Utc>>) -> Option<String> {
    dt.as_ref().map(ts)
}

fn parse_ts(s: &str) -> Result<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s)
        .map(|d| d.with_timezone(&Utc))
        .map_err(|e| Error::internal(format!("bad timestamp '{s}' in database: {e}")))
}

fn get_ts(r: &SqliteRow, col: &str) -> Result<DateTime<Utc>> {
    parse_ts(&r.try_get::<String, _>(col)?)
}

fn get_ts_opt(r: &SqliteRow, col: &str) -> Result<Option<DateTime<Utc>>> {
    r.try_get::<Option<String>, _>(col)?.map(|s| parse_ts(&s)).transpose()
}

fn get_enum<T: FromStr<Err = Error>>(r: &SqliteRow, col: &str) -> Result<T> {
    r.try_get::<String, _>(col)?.parse()
}

fn get_u16_opt(r: &SqliteRow, col: &str) -> Result<Option<u16>> {
    Ok(r.try_get::<Option<i64>, _>(col)?.and_then(|v| u16::try_from(v).ok()))
}

fn get_u32_opt(r: &SqliteRow, col: &str) -> Result<Option<u32>> {
    Ok(r.try_get::<Option<i64>, _>(col)?.and_then(|v| u32::try_from(v).ok()))
}

fn is_unique_violation(e: &sqlx::Error) -> bool {
    matches!(e, sqlx::Error::Database(db) if db.is_unique_violation())
}

/// The Conflict of a name a service or datastore of `environment` uses.
fn name_in_use(name: &str, environment: &str) -> Error {
    Error::conflict(format!("name '{name}' is already in use in environment '{environment}'"))
}

/// NotFound for a service or datastore looked up in `environment` (`None`
/// = `production`, which the message leaves out).
fn not_found_in(kind: &str, id_or_name: &str, environment: Option<&str>) -> Error {
    match environment.filter(|e| *e != PRODUCTION) {
        Some(e) => Error::NotFound(format!("{kind} '{id_or_name}' in environment '{e}'")),
        None => Error::not_found(kind, id_or_name),
    }
}

fn row_to_service(r: &SqliteRow) -> Result<Service> {
    let domains: String = r.try_get("custom_domains")?;
    Ok(Service {
        id: r.try_get("id")?,
        name: r.try_get("name")?,
        environment: r.try_get("environment")?,
        service_type: get_enum(r, "service_type")?,
        repo_url: r.try_get("repo_url")?,
        branch: r.try_get("branch")?,
        image: r.try_get("image")?,
        runtime: get_enum(r, "runtime")?,
        root_dir: r.try_get("root_dir")?,
        dockerfile_path: r.try_get("dockerfile_path")?,
        build_command: r.try_get("build_command")?,
        start_command: r.try_get("start_command")?,
        publish_dir: r.try_get("publish_dir")?,
        port: get_u16_opt(r, "port")?,
        health_check_path: r.try_get("health_check_path")?,
        schedule: r.try_get("schedule")?,
        instances: r.try_get::<i64, _>("instances")?.clamp(0, u32::MAX as i64) as u32,
        auto_deploy: r.try_get("auto_deploy")?,
        suspended: r.try_get("suspended")?,
        disk_mount_path: r.try_get("disk_mount_path")?,
        memory_limit_mb: get_u32_opt(r, "memory_limit_mb")?,
        cpu_limit: r.try_get("cpu_limit")?,
        custom_domains: serde_json::from_str(&domains)
            .map_err(|e| Error::internal(format!("bad custom_domains json: {e}")))?,
        deploy_hook_key: r.try_get("deploy_hook_key")?,
        live_deploy_id: r.try_get("live_deploy_id")?,
        created_at: get_ts(r, "created_at")?,
        updated_at: get_ts(r, "updated_at")?,
    })
}

fn row_to_deploy(r: &SqliteRow) -> Result<Deploy> {
    let source: String = r.try_get("source")?;
    Ok(Deploy {
        id: r.try_get("id")?,
        service_id: r.try_get("service_id")?,
        status: get_enum(r, "status")?,
        trigger: get_enum(r, "trigger_kind")?,
        source: serde_json::from_str(&source).map_err(|e| Error::internal(format!("bad deploy source json: {e}")))?,
        commit_sha: r.try_get("commit_sha")?,
        commit_message: r.try_get("commit_message")?,
        image: r.try_get("image")?,
        port: get_u16_opt(r, "port")?,
        error: r.try_get("error")?,
        created_at: get_ts(r, "created_at")?,
        started_at: get_ts_opt(r, "started_at")?,
        finished_at: get_ts_opt(r, "finished_at")?,
    })
}

fn row_to_job(r: &SqliteRow) -> Result<JobRun> {
    Ok(JobRun {
        id: r.try_get("id")?,
        service_id: r.try_get("service_id")?,
        trigger: get_enum(r, "trigger_kind")?,
        command: r.try_get("command")?,
        image: r.try_get("image")?,
        status: get_enum(r, "status")?,
        exit_code: r.try_get("exit_code")?,
        error: r.try_get("error")?,
        created_at: get_ts(r, "created_at")?,
        started_at: get_ts_opt(r, "started_at")?,
        finished_at: get_ts_opt(r, "finished_at")?,
    })
}

fn row_to_datastore(r: &SqliteRow) -> Result<Datastore> {
    Ok(Datastore {
        id: r.try_get("id")?,
        name: r.try_get("name")?,
        environment: r.try_get("environment")?,
        kind: get_enum(r, "kind")?,
        version: r.try_get("version")?,
        status: get_enum(r, "status")?,
        username: r.try_get("username")?,
        password: r.try_get("password")?,
        database: r.try_get("database")?,
        host_port: get_u16_opt(r, "host_port")?,
        memory_limit_mb: get_u32_opt(r, "memory_limit_mb")?,
        cpu_limit: r.try_get("cpu_limit")?,
        error: r.try_get("error")?,
        created_at: get_ts(r, "created_at")?,
        updated_at: get_ts(r, "updated_at")?,
    })
}

fn row_to_env_group(r: &SqliteRow) -> Result<EnvGroup> {
    Ok(EnvGroup {
        id: r.try_get("id")?,
        name: r.try_get("name")?,
        created_at: get_ts(r, "created_at")?,
        updated_at: get_ts(r, "updated_at")?,
    })
}

fn row_to_environment(r: &SqliteRow) -> Result<Environment> {
    Ok(Environment { id: r.try_get("id")?, name: r.try_get("name")?, created_at: get_ts(r, "created_at")? })
}

fn row_to_env(r: &SqliteRow) -> Result<EnvVar> {
    Ok(EnvVar { key: r.try_get("key")?, value: r.try_get("value")? })
}

fn get_u64_opt(r: &SqliteRow, col: &str) -> Result<Option<u64>> {
    Ok(r.try_get::<Option<i64>, _>(col)?.and_then(|v| u64::try_from(v).ok()))
}

/// An `AVG` of byte counts, rounded to a whole byte.
fn get_avg_u64(r: &SqliteRow, col: &str) -> Result<Option<u64>> {
    Ok(r.try_get::<Option<f64>, _>(col)?.map(|v| v.round().max(0.0) as u64))
}

/// A Unix time in seconds (the metrics history).
fn get_unix_time(r: &SqliteRow, col: &str) -> Result<DateTime<Utc>> {
    let secs: i64 = r.try_get(col)?;
    DateTime::from_timestamp(secs, 0).ok_or_else(|| Error::internal(format!("bad time {secs} in database")))
}

fn container_kind_str(kind: ContainerKind) -> &'static str {
    match kind {
        ContainerKind::Service => "service",
        ContainerKind::Datastore => "datastore",
    }
}

fn parse_container_kind(s: &str) -> Result<ContainerKind> {
    match s {
        "service" => Ok(ContainerKind::Service),
        "datastore" => Ok(ContainerKind::Datastore),
        _ => Err(Error::internal(format!("bad container kind '{s}' in database"))),
    }
}

fn row_to_git_connection(r: &SqliteRow) -> Result<GitConnection> {
    let scopes: String = r.try_get("scopes")?;
    // An app is its id, slug, page and key together.
    let app = match (
        get_u64_opt(r, "app_id")?,
        r.try_get::<Option<String>, _>("app_slug")?,
        r.try_get::<Option<String>, _>("app_url")?,
        r.try_get::<Option<String>, _>("private_key")?,
    ) {
        (Some(id), Some(slug), Some(url), Some(private_key)) => {
            Some(GithubApp { id, slug, url, private_key, webhook_secret: r.try_get("webhook_secret")? })
        }
        _ => None,
    };
    let installation = get_u64_opt(r, "installation_id")?
        .map(|id| -> Result<GithubInstallation> {
            Ok(GithubInstallation {
                id,
                url: r.try_get("installation_url")?,
                repository_selection: r.try_get("repository_selection")?,
            })
        })
        .transpose()?;
    Ok(GitConnection {
        id: r.try_get("id")?,
        provider: get_enum(r, "provider")?,
        base_url: r.try_get("base_url")?,
        auth: get_enum(r, "auth")?,
        account: r.try_get("account")?,
        account_name: r.try_get("account_name")?,
        token: r.try_get("token")?,
        refresh_token: r.try_get("refresh_token")?,
        scopes: serde_json::from_str(&scopes)
            .map_err(|e| Error::internal(format!("bad git connection scopes json: {e}")))?,
        token_expires_at: get_ts_opt(r, "token_expires_at")?,
        client_id: r.try_get("client_id")?,
        client_secret: r.try_get("client_secret")?,
        app,
        installation,
        created_at: get_ts(r, "created_at")?,
        updated_at: get_ts(r, "updated_at")?,
    })
}

fn row_to_domain(r: &SqliteRow) -> Result<Domain> {
    let checks: String = r.try_get("checks")?;
    Ok(Domain {
        id: r.try_get("id")?,
        name: r.try_get("name")?,
        source: get_enum(r, "source")?,
        status: get_enum(r, "status")?,
        is_default: r.try_get("is_default")?,
        checks: serde_json::from_str(&checks).map_err(|e| Error::internal(format!("bad domain checks json: {e}")))?,
        failures: get_u32_opt(r, "failures")?.unwrap_or(0),
        created_at: get_ts(r, "created_at")?,
        verified_at: get_ts_opt(r, "verified_at")?,
        checked_at: get_ts_opt(r, "checked_at")?,
    })
}

/// SQLite integers are signed: ids GitHub hands out fit (they are far below 2^63).
fn row_to_user(r: &SqliteRow) -> Result<User> {
    Ok(User {
        id: r.try_get("id")?,
        email: r.try_get("email")?,
        password_hash: r.try_get("password_hash")?,
        created_at: get_ts(r, "created_at")?,
        updated_at: get_ts(r, "updated_at")?,
    })
}

fn row_to_session(r: &SqliteRow) -> Result<Session> {
    Ok(Session {
        id: r.try_get("id")?,
        user_id: r.try_get("user_id")?,
        token_hash: r.try_get("token_hash")?,
        user_agent: r.try_get("user_agent")?,
        created_at: get_ts(r, "created_at")?,
        last_used_at: get_ts(r, "last_used_at")?,
        expires_at: get_ts(r, "expires_at")?,
    })
}

fn row_to_api_token(r: &SqliteRow) -> Result<ApiToken> {
    Ok(ApiToken {
        id: r.try_get("id")?,
        name: r.try_get("name")?,
        token_hash: r.try_get("token_hash")?,
        hint: r.try_get("hint")?,
        created_at: get_ts(r, "created_at")?,
        last_used_at: get_ts_opt(r, "last_used_at")?,
        expires_at: get_ts_opt(r, "expires_at")?,
    })
}

fn sql_u64(v: Option<u64>) -> Option<i64> {
    v.and_then(|v| i64::try_from(v).ok())
}

/// A count of `request_samples` (or a `SUM` of them).
fn sql_count(n: u64) -> i64 {
    i64::try_from(n).unwrap_or(i64::MAX)
}

/// The count columns of `request_samples`: the status classes, then the
/// latency histogram in the order of its buckets (`ms_<bound>`, `ms_more`).
fn request_columns() -> Vec<String> {
    ["status_2xx", "status_3xx", "status_4xx", "status_5xx"]
        .into_iter()
        .map(String::from)
        .chain(LATENCY_BOUNDS_MS.iter().map(|ms| format!("ms_{ms}")))
        .chain(std::iter::once("ms_more".to_string()))
        .collect()
}

/// `SUM(c) AS c` for each count column.
fn request_sums() -> String {
    request_columns().iter().map(|c| format!("SUM({c}) AS {c}")).collect::<Vec<_>>().join(", ")
}

/// The counts of a row read with the `columns` of [`request_columns`].
fn row_to_request_counts(r: &SqliteRow, columns: &[String]) -> Result<RequestCounts> {
    let mut values = Vec::with_capacity(columns.len());
    for c in columns {
        values.push(u64::try_from(r.try_get::<i64, _>(c.as_str())?).unwrap_or_default());
    }
    let mut counts = RequestCounts {
        status_2xx: values[0],
        status_3xx: values[1],
        status_4xx: values[2],
        status_5xx: values[3],
        ..RequestCounts::default()
    };
    counts.latency.0.copy_from_slice(&values[4..]);
    Ok(counts)
}

/// The values of [`request_columns`] for `counts`.
fn request_values(counts: &RequestCounts) -> impl Iterator<Item = i64> + '_ {
    [counts.status_2xx, counts.status_3xx, counts.status_4xx, counts.status_5xx]
        .into_iter()
        .chain(counts.latency.0)
        .map(sql_count)
}

impl Store {
    // -----------------------------------------------------------------------
    // lifecycle

    /// Open (creating if needed) the database at `path` and run migrations.
    pub async fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            tokio::fs::create_dir_all(parent).await?;
        }
        let opts = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .synchronous(SqliteSynchronous::Normal)
            .foreign_keys(true)
            .busy_timeout(Duration::from_secs(10));
        // Lazy: the first connection is the one `migrate` holds, so every
        // other connection is opened after the migrations (see `migrate`).
        let pool = SqlitePoolOptions::new().max_connections(8).connect_lazy_with(opts);
        let store = Store { pool };
        store.migrate().await?;
        Ok(store)
    }

    /// A private in-memory database (for tests).
    pub async fn open_in_memory() -> Result<Self> {
        let opts = SqliteConnectOptions::from_str("sqlite::memory:")?.foreign_keys(true);
        // One connection: every in-memory connection would be its own database.
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .idle_timeout(None)
            .max_lifetime(None)
            .connect_with(opts)
            .await?;
        let store = Store { pool };
        store.migrate().await?;
        Ok(store)
    }

    /// Every migration runs on one connection: a connection that loaded the
    /// schema between two migrations would prepare `SELECT *` statements with
    /// the old column list, which sqlx caches (it then fails on rows with the
    /// new columns).
    ///
    /// Foreign keys are off while they run (SQLite ignores the pragma inside
    /// a transaction): a migration that rebuilds a table drops the old one,
    /// which would otherwise delete the rows that reference it. Each
    /// migration checks the keys before it commits.
    async fn migrate(&self) -> Result<()> {
        let mut conn = self.pool.acquire().await?;
        let version: i64 = sqlx::query_scalar("PRAGMA user_version").fetch_one(&mut *conn).await?;
        if version.max(0) as usize >= MIGRATIONS.len() {
            return Ok(());
        }
        sqlx::query("PRAGMA foreign_keys = OFF").execute(&mut *conn).await?;
        let applied = Self::apply_migrations(&mut conn, version.max(0) as usize).await;
        // Back on whatever happened: the connection returns to the pool.
        let restored = sqlx::query("PRAGMA foreign_keys = ON").execute(&mut *conn).await;
        if restored.is_err() {
            // Never hand out a connection without foreign keys: close it.
            drop(conn.detach());
        }
        applied?;
        restored?;
        Ok(())
    }

    async fn apply_migrations(conn: &mut sqlx::SqliteConnection, from: usize) -> Result<()> {
        use sqlx::Connection;
        for (i, sql) in MIGRATIONS.iter().enumerate().skip(from) {
            let mut tx = conn.begin().await?;
            sqlx::raw_sql(sql).execute(&mut *tx).await?;
            let broken: Vec<SqliteRow> = sqlx::query("PRAGMA foreign_key_check").fetch_all(&mut *tx).await?;
            if let Some(row) = broken.first() {
                let table: String = row.try_get(0)?;
                return Err(Error::internal(format!(
                    "database migration {} left {} broken foreign key(s), in table '{table}'",
                    i + 1,
                    broken.len()
                )));
            }
            sqlx::query(&format!("PRAGMA user_version = {}", i + 1)).execute(&mut *tx).await?;
            tx.commit().await?;
            tracing::info!(version = i + 1, "applied database migration");
        }
        Ok(())
    }

    /// Raw pool access (escape hatch).
    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    // -----------------------------------------------------------------------
    // names

    /// True if a service or datastore of `production` already uses `name`
    /// ([`Store::name_taken_in`]).
    pub async fn name_taken(&self, name: &str) -> Result<bool> {
        self.name_taken_in(PRODUCTION, name).await
    }

    /// True if a service or datastore of `environment` (a name) already uses
    /// `name`: they share the private-network hostnames of their environment.
    /// Another environment may use the same name.
    pub async fn name_taken_in(&self, environment: &str, name: &str) -> Result<bool> {
        let n: i64 = sqlx::query_scalar(
            "SELECT (SELECT COUNT(*) FROM services WHERE environment = ?1 AND name = ?2)
                  + (SELECT COUNT(*) FROM datastores WHERE environment = ?1 AND name = ?2)",
        )
        .bind(environment)
        .bind(name)
        .fetch_one(&self.pool)
        .await?;
        Ok(n > 0)
    }

    // -----------------------------------------------------------------------
    // services

    /// Insert a new service. Conflict if the name is taken by a service or
    /// datastore of its environment; NotFound if its environment doesn't exist.
    pub async fn create_service(&self, s: &Service) -> Result<()> {
        if self.name_taken_in(&s.environment, &s.name).await? {
            return Err(name_in_use(&s.name, &s.environment));
        }
        self.require_environment_named(&s.environment).await?;
        let res = sqlx::query(
            "INSERT INTO services (id, name, environment, service_type, repo_url, branch, image, runtime, root_dir,
                dockerfile_path, build_command, start_command, publish_dir, port, health_check_path, schedule, instances,
                auto_deploy, suspended, disk_mount_path, memory_limit_mb, cpu_limit, custom_domains, deploy_hook_key,
                live_deploy_id, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&s.id)
        .bind(&s.name)
        .bind(&s.environment)
        .bind(s.service_type.as_str())
        .bind(&s.repo_url)
        .bind(&s.branch)
        .bind(&s.image)
        .bind(s.runtime.as_str())
        .bind(&s.root_dir)
        .bind(&s.dockerfile_path)
        .bind(&s.build_command)
        .bind(&s.start_command)
        .bind(&s.publish_dir)
        .bind(s.port.map(i64::from))
        .bind(&s.health_check_path)
        .bind(&s.schedule)
        .bind(i64::from(s.instances))
        .bind(s.auto_deploy)
        .bind(s.suspended)
        .bind(&s.disk_mount_path)
        .bind(s.memory_limit_mb.map(i64::from))
        .bind(s.cpu_limit)
        .bind(serde_json::to_string(&s.custom_domains).unwrap_or_else(|_| "[]".into()))
        .bind(&s.deploy_hook_key)
        .bind(&s.live_deploy_id)
        .bind(ts(&s.created_at))
        .bind(ts(&s.updated_at))
        .execute(&self.pool)
        .await;
        match res {
            Ok(_) => Ok(()),
            Err(e) if is_unique_violation(&e) => Err(name_in_use(&s.name, &s.environment)),
            Err(e) => Err(e.into()),
        }
    }

    /// Persist the user-editable settings of `s` and bump `updated_at`.
    /// Returns the stored row.
    ///
    /// Engine-owned columns — `instances`, `suspended` and `live_deploy_id` —
    /// are NOT written (use [`Store::set_instances`], [`Store::set_suspended`],
    /// [`Store::set_live_deploy`]), so a concurrent settings update can never
    /// revert a scale, suspend or deploy. `name`, `environment` and `id`
    /// are immutable.
    pub async fn update_service(&self, s: &Service) -> Result<Service> {
        let res = sqlx::query(
            "UPDATE services SET service_type = ?, repo_url = ?, branch = ?, image = ?, runtime = ?, root_dir = ?,
                dockerfile_path = ?, build_command = ?, start_command = ?, publish_dir = ?, port = ?,
                health_check_path = ?, schedule = ?, auto_deploy = ?,
                disk_mount_path = ?, memory_limit_mb = ?, cpu_limit = ?, custom_domains = ?, deploy_hook_key = ?,
                updated_at = ?
             WHERE id = ?",
        )
        .bind(s.service_type.as_str())
        .bind(&s.repo_url)
        .bind(&s.branch)
        .bind(&s.image)
        .bind(s.runtime.as_str())
        .bind(&s.root_dir)
        .bind(&s.dockerfile_path)
        .bind(&s.build_command)
        .bind(&s.start_command)
        .bind(&s.publish_dir)
        .bind(s.port.map(i64::from))
        .bind(&s.health_check_path)
        .bind(&s.schedule)
        .bind(s.auto_deploy)
        .bind(&s.disk_mount_path)
        .bind(s.memory_limit_mb.map(i64::from))
        .bind(s.cpu_limit)
        .bind(serde_json::to_string(&s.custom_domains).unwrap_or_else(|_| "[]".into()))
        .bind(&s.deploy_hook_key)
        .bind(ts(&Utc::now()))
        .bind(&s.id)
        .execute(&self.pool)
        .await?;
        if res.rows_affected() == 0 {
            return Err(Error::not_found("service", &s.id));
        }
        self.require_service(&s.id).await
    }

    pub async fn get_service(&self, id: &str) -> Result<Option<Service>> {
        sqlx::query("SELECT * FROM services WHERE id = ?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?
            .map(|r| row_to_service(&r))
            .transpose()
    }

    /// Look up by id, or by name in `production` ([`Store::find_service_in`]).
    pub async fn find_service(&self, id_or_name: &str) -> Result<Option<Service>> {
        self.find_service_in(id_or_name, None).await
    }

    /// Look up by id or name. A name is looked up in `environment` (a name;
    /// `None` = `production`), as another environment may use the same one.
    /// An id finds its service whatever its environment, unless `environment`
    /// names another one.
    pub async fn find_service_in(&self, id_or_name: &str, environment: Option<&str>) -> Result<Option<Service>> {
        sqlx::query(
            "SELECT * FROM services
             WHERE (id = ?1 AND (?2 IS NULL OR environment = ?2)) OR (name = ?1 AND environment = COALESCE(?2, ?3))
             ORDER BY (id = ?1) DESC LIMIT 1",
        )
        .bind(id_or_name)
        .bind(environment)
        .bind(PRODUCTION)
        .fetch_optional(&self.pool)
        .await?
        .map(|r| row_to_service(&r))
        .transpose()
    }

    /// Look up by id, or by name in `production`; NotFound if missing.
    pub async fn require_service(&self, id_or_name: &str) -> Result<Service> {
        self.require_service_in(id_or_name, None).await
    }

    /// [`Store::find_service_in`]; NotFound if missing.
    pub async fn require_service_in(&self, id_or_name: &str, environment: Option<&str>) -> Result<Service> {
        self.find_service_in(id_or_name, environment)
            .await?
            .ok_or_else(|| not_found_in("service", id_or_name, environment))
    }

    /// All services ordered by name (one name: `production`'s first, then by
    /// environment).
    pub async fn list_services(&self) -> Result<Vec<Service>> {
        sqlx::query("SELECT * FROM services ORDER BY name, (environment = ?) DESC, environment")
            .bind(PRODUCTION)
            .fetch_all(&self.pool)
            .await?
            .iter()
            .map(row_to_service)
            .collect()
    }

    /// Delete a service with its env vars, env group links, deploys and job runs.
    pub async fn delete_service(&self, id: &str) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("DELETE FROM env_vars WHERE owner_id = ?").bind(id).execute(&mut *tx).await?;
        sqlx::query("DELETE FROM service_env_groups WHERE service_id = ?").bind(id).execute(&mut *tx).await?;
        sqlx::query("DELETE FROM deploys WHERE service_id = ?").bind(id).execute(&mut *tx).await?;
        sqlx::query("DELETE FROM job_runs WHERE service_id = ?").bind(id).execute(&mut *tx).await?;
        let res = sqlx::query("DELETE FROM services WHERE id = ?").bind(id).execute(&mut *tx).await?;
        tx.commit().await?;
        if res.rows_affected() == 0 {
            return Err(Error::not_found("service", id));
        }
        Ok(())
    }

    pub async fn set_live_deploy(&self, service_id: &str, deploy_id: Option<&str>) -> Result<()> {
        self.exec_service_update("live_deploy_id = ?", service_id, deploy_id.map(str::to_string)).await
    }

    pub async fn set_suspended(&self, service_id: &str, suspended: bool) -> Result<()> {
        let res = sqlx::query("UPDATE services SET suspended = ?, updated_at = ? WHERE id = ?")
            .bind(suspended)
            .bind(ts(&Utc::now()))
            .bind(service_id)
            .execute(&self.pool)
            .await?;
        if res.rows_affected() == 0 {
            return Err(Error::not_found("service", service_id));
        }
        Ok(())
    }

    pub async fn set_instances(&self, service_id: &str, instances: u32) -> Result<()> {
        let res = sqlx::query("UPDATE services SET instances = ?, updated_at = ? WHERE id = ?")
            .bind(i64::from(instances))
            .bind(ts(&Utc::now()))
            .bind(service_id)
            .execute(&self.pool)
            .await?;
        if res.rows_affected() == 0 {
            return Err(Error::not_found("service", service_id));
        }
        Ok(())
    }

    async fn exec_service_update(&self, set_clause: &str, service_id: &str, value: Option<String>) -> Result<()> {
        let sql = format!("UPDATE services SET {set_clause}, updated_at = ? WHERE id = ?");
        let res = sqlx::query(&sql).bind(value).bind(ts(&Utc::now())).bind(service_id).execute(&self.pool).await?;
        if res.rows_affected() == 0 {
            return Err(Error::not_found("service", service_id));
        }
        Ok(())
    }

    // -----------------------------------------------------------------------
    // env vars (owner = service id or env group id)

    /// Variables of an owner, sorted by key.
    pub async fn list_env(&self, owner_id: &str) -> Result<Vec<EnvVar>> {
        sqlx::query("SELECT key, value FROM env_vars WHERE owner_id = ? ORDER BY key")
            .bind(owner_id)
            .fetch_all(&self.pool)
            .await?
            .iter()
            .map(row_to_env)
            .collect()
    }

    /// Insert or update one variable.
    pub async fn set_env(&self, owner_id: &str, key: &str, value: &str) -> Result<()> {
        sqlx::query(
            "INSERT INTO env_vars (owner_id, key, value) VALUES (?, ?, ?)
             ON CONFLICT(owner_id, key) DO UPDATE SET value = excluded.value",
        )
        .bind(owner_id)
        .bind(key)
        .bind(value)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Delete one variable; returns whether it existed.
    pub async fn unset_env(&self, owner_id: &str, key: &str) -> Result<bool> {
        let res = sqlx::query("DELETE FROM env_vars WHERE owner_id = ? AND key = ?")
            .bind(owner_id)
            .bind(key)
            .execute(&self.pool)
            .await?;
        Ok(res.rows_affected() > 0)
    }

    /// Replace all variables of an owner atomically.
    pub async fn replace_env(&self, owner_id: &str, vars: &[EnvVar]) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("DELETE FROM env_vars WHERE owner_id = ?").bind(owner_id).execute(&mut *tx).await?;
        for v in vars {
            sqlx::query(
                "INSERT INTO env_vars (owner_id, key, value) VALUES (?, ?, ?)
                 ON CONFLICT(owner_id, key) DO UPDATE SET value = excluded.value",
            )
            .bind(owner_id)
            .bind(&v.key)
            .bind(&v.value)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    /// Upsert `set` and delete `unset` atomically.
    pub async fn patch_env(&self, owner_id: &str, set: &[EnvVar], unset: &[String]) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        for k in unset {
            sqlx::query("DELETE FROM env_vars WHERE owner_id = ? AND key = ?")
                .bind(owner_id)
                .bind(k)
                .execute(&mut *tx)
                .await?;
        }
        for v in set {
            sqlx::query(
                "INSERT INTO env_vars (owner_id, key, value) VALUES (?, ?, ?)
                 ON CONFLICT(owner_id, key) DO UPDATE SET value = excluded.value",
            )
            .bind(owner_id)
            .bind(&v.key)
            .bind(&v.value)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    /// The service's unresolved environment: linked env groups (in link
    /// order) overridden by the service's own variables.
    pub async fn effective_env(&self, service_id: &str) -> Result<Vec<EnvVar>> {
        let mut layers = Vec::new();
        for g in self.service_env_groups(service_id).await? {
            layers.push(self.list_env(&g.id).await?);
        }
        layers.push(self.list_env(service_id).await?);
        Ok(env::merge(&layers))
    }

    // -----------------------------------------------------------------------
    // env groups

    pub async fn create_env_group(&self, g: &EnvGroup) -> Result<()> {
        let res = sqlx::query("INSERT INTO env_groups (id, name, created_at, updated_at) VALUES (?, ?, ?, ?)")
            .bind(&g.id)
            .bind(&g.name)
            .bind(ts(&g.created_at))
            .bind(ts(&g.updated_at))
            .execute(&self.pool)
            .await;
        match res {
            Ok(_) => Ok(()),
            Err(e) if is_unique_violation(&e) => Err(Error::conflict(format!("env group '{}' already exists", g.name))),
            Err(e) => Err(e.into()),
        }
    }

    pub async fn find_env_group(&self, id_or_name: &str) -> Result<Option<EnvGroup>> {
        sqlx::query("SELECT * FROM env_groups WHERE id = ?1 OR name = ?1 ORDER BY (id = ?1) DESC LIMIT 1")
            .bind(id_or_name)
            .fetch_optional(&self.pool)
            .await?
            .map(|r| row_to_env_group(&r))
            .transpose()
    }

    pub async fn require_env_group(&self, id_or_name: &str) -> Result<EnvGroup> {
        self.find_env_group(id_or_name).await?.ok_or_else(|| Error::not_found("env group", id_or_name))
    }

    pub async fn list_env_groups(&self) -> Result<Vec<EnvGroup>> {
        sqlx::query("SELECT * FROM env_groups ORDER BY name")
            .fetch_all(&self.pool)
            .await?
            .iter()
            .map(row_to_env_group)
            .collect()
    }

    /// Bump an env group's `updated_at` (call after changing its variables).
    pub async fn touch_env_group(&self, id: &str) -> Result<()> {
        sqlx::query("UPDATE env_groups SET updated_at = ? WHERE id = ?")
            .bind(ts(&Utc::now()))
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Delete an env group, its variables and its service links.
    pub async fn delete_env_group(&self, id: &str) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("DELETE FROM env_vars WHERE owner_id = ?").bind(id).execute(&mut *tx).await?;
        sqlx::query("DELETE FROM service_env_groups WHERE group_id = ?").bind(id).execute(&mut *tx).await?;
        let res = sqlx::query("DELETE FROM env_groups WHERE id = ?").bind(id).execute(&mut *tx).await?;
        tx.commit().await?;
        if res.rows_affected() == 0 {
            return Err(Error::not_found("env group", id));
        }
        Ok(())
    }

    /// Link an env group to a service (idempotent; appended last).
    pub async fn link_env_group(&self, service_id: &str, group_id: &str) -> Result<()> {
        sqlx::query(
            "INSERT OR IGNORE INTO service_env_groups (service_id, group_id, position)
             VALUES (?1, ?2, (SELECT COALESCE(MAX(position), 0) + 1 FROM service_env_groups WHERE service_id = ?1))",
        )
        .bind(service_id)
        .bind(group_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Unlink; returns whether a link existed.
    pub async fn unlink_env_group(&self, service_id: &str, group_id: &str) -> Result<bool> {
        let res = sqlx::query("DELETE FROM service_env_groups WHERE service_id = ? AND group_id = ?")
            .bind(service_id)
            .bind(group_id)
            .execute(&self.pool)
            .await?;
        Ok(res.rows_affected() > 0)
    }

    /// Env groups linked to a service, in link order.
    pub async fn service_env_groups(&self, service_id: &str) -> Result<Vec<EnvGroup>> {
        sqlx::query(
            "SELECT g.* FROM env_groups g JOIN service_env_groups l ON l.group_id = g.id
             WHERE l.service_id = ? ORDER BY l.position",
        )
        .bind(service_id)
        .fetch_all(&self.pool)
        .await?
        .iter()
        .map(row_to_env_group)
        .collect()
    }

    /// Services linked to an env group (ordered by name).
    pub async fn env_group_services(&self, group_id: &str) -> Result<Vec<Service>> {
        sqlx::query(
            "SELECT s.* FROM services s JOIN service_env_groups l ON l.service_id = s.id
             WHERE l.group_id = ? ORDER BY s.name, (s.environment = ?) DESC, s.environment",
        )
        .bind(group_id)
        .bind(PRODUCTION)
        .fetch_all(&self.pool)
        .await?
        .iter()
        .map(row_to_service)
        .collect()
    }

    // -----------------------------------------------------------------------
    // deploys

    pub async fn create_deploy(&self, d: &Deploy) -> Result<()> {
        sqlx::query(
            "INSERT INTO deploys (id, service_id, status, trigger_kind, source, commit_sha, commit_message, image,
                port, error, created_at, started_at, finished_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&d.id)
        .bind(&d.service_id)
        .bind(d.status.as_str())
        .bind(d.trigger.as_str())
        .bind(serde_json::to_string(&d.source).map_err(|e| Error::internal(e.to_string()))?)
        .bind(&d.commit_sha)
        .bind(&d.commit_message)
        .bind(&d.image)
        .bind(d.port.map(i64::from))
        .bind(&d.error)
        .bind(ts(&d.created_at))
        .bind(ts_opt(&d.started_at))
        .bind(ts_opt(&d.finished_at))
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Persist every mutable column of a deploy.
    pub async fn update_deploy(&self, d: &Deploy) -> Result<()> {
        let res = sqlx::query(
            "UPDATE deploys SET status = ?, source = ?, commit_sha = ?, commit_message = ?, image = ?, port = ?,
                error = ?, started_at = ?, finished_at = ?
             WHERE id = ?",
        )
        .bind(d.status.as_str())
        .bind(serde_json::to_string(&d.source).map_err(|e| Error::internal(e.to_string()))?)
        .bind(&d.commit_sha)
        .bind(&d.commit_message)
        .bind(&d.image)
        .bind(d.port.map(i64::from))
        .bind(&d.error)
        .bind(ts_opt(&d.started_at))
        .bind(ts_opt(&d.finished_at))
        .bind(&d.id)
        .execute(&self.pool)
        .await?;
        if res.rows_affected() == 0 {
            return Err(Error::not_found("deploy", &d.id));
        }
        Ok(())
    }

    /// Change a deploy's status, stamping `started_at` when it starts building
    /// or deploying and `finished_at` when it reaches a terminal status
    /// (each only if not already set). `error` replaces the stored error when
    /// `Some`. Returns the updated deploy.
    pub async fn set_deploy_status(&self, id: &str, status: DeployStatus, error: Option<&str>) -> Result<Deploy> {
        let mut d = self.require_deploy(id).await?;
        let now = Utc::now();
        d.status = status;
        if matches!(status, DeployStatus::Building | DeployStatus::Deploying) && d.started_at.is_none() {
            d.started_at = Some(now);
        }
        if status.is_terminal() && d.finished_at.is_none() {
            d.finished_at = Some(now);
        }
        if let Some(e) = error {
            d.error = Some(e.to_string());
        }
        self.update_deploy(&d).await?;
        Ok(d)
    }

    pub async fn get_deploy(&self, id: &str) -> Result<Option<Deploy>> {
        sqlx::query("SELECT * FROM deploys WHERE id = ?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?
            .map(|r| row_to_deploy(&r))
            .transpose()
    }

    pub async fn require_deploy(&self, id: &str) -> Result<Deploy> {
        self.get_deploy(id).await?.ok_or_else(|| Error::not_found("deploy", id))
    }

    /// Newest first.
    pub async fn list_deploys(&self, service_id: &str, limit: u32) -> Result<Vec<Deploy>> {
        sqlx::query("SELECT * FROM deploys WHERE service_id = ? ORDER BY created_at DESC, rowid DESC LIMIT ?")
            .bind(service_id)
            .bind(i64::from(limit))
            .fetch_all(&self.pool)
            .await?
            .iter()
            .map(row_to_deploy)
            .collect()
    }

    pub async fn latest_deploy(&self, service_id: &str) -> Result<Option<Deploy>> {
        Ok(self.list_deploys(service_id, 1).await?.into_iter().next())
    }

    /// Queued / building / deploying deploys of all services, oldest first.
    pub async fn active_deploys(&self) -> Result<Vec<Deploy>> {
        sqlx::query(
            "SELECT * FROM deploys WHERE status IN ('queued', 'building', 'deploying') ORDER BY created_at, rowid",
        )
        .fetch_all(&self.pool)
        .await?
        .iter()
        .map(row_to_deploy)
        .collect()
    }

    /// Deploys of a service that recorded an image, newest first.
    pub async fn deploys_with_images(&self, service_id: &str) -> Result<Vec<Deploy>> {
        sqlx::query(
            "SELECT * FROM deploys WHERE service_id = ? AND image IS NOT NULL ORDER BY created_at DESC, rowid DESC",
        )
        .bind(service_id)
        .fetch_all(&self.pool)
        .await?
        .iter()
        .map(row_to_deploy)
        .collect()
    }

    // -----------------------------------------------------------------------
    // job runs

    pub async fn create_job_run(&self, j: &JobRun) -> Result<()> {
        sqlx::query(
            "INSERT INTO job_runs (id, service_id, trigger_kind, command, image, status, exit_code, error,
                created_at, started_at, finished_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&j.id)
        .bind(&j.service_id)
        .bind(j.trigger.as_str())
        .bind(&j.command)
        .bind(&j.image)
        .bind(j.status.as_str())
        .bind(j.exit_code)
        .bind(&j.error)
        .bind(ts(&j.created_at))
        .bind(ts_opt(&j.started_at))
        .bind(ts_opt(&j.finished_at))
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn update_job_run(&self, j: &JobRun) -> Result<()> {
        let res = sqlx::query(
            "UPDATE job_runs SET command = ?, image = ?, status = ?, exit_code = ?, error = ?, started_at = ?,
                finished_at = ?
             WHERE id = ?",
        )
        .bind(&j.command)
        .bind(&j.image)
        .bind(j.status.as_str())
        .bind(j.exit_code)
        .bind(&j.error)
        .bind(ts_opt(&j.started_at))
        .bind(ts_opt(&j.finished_at))
        .bind(&j.id)
        .execute(&self.pool)
        .await?;
        if res.rows_affected() == 0 {
            return Err(Error::not_found("job", &j.id));
        }
        Ok(())
    }

    pub async fn get_job_run(&self, id: &str) -> Result<Option<JobRun>> {
        sqlx::query("SELECT * FROM job_runs WHERE id = ?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?
            .map(|r| row_to_job(&r))
            .transpose()
    }

    pub async fn require_job_run(&self, id: &str) -> Result<JobRun> {
        self.get_job_run(id).await?.ok_or_else(|| Error::not_found("job", id))
    }

    /// Newest first.
    pub async fn list_job_runs(&self, service_id: &str, limit: u32) -> Result<Vec<JobRun>> {
        sqlx::query("SELECT * FROM job_runs WHERE service_id = ? ORDER BY created_at DESC, rowid DESC LIMIT ?")
            .bind(service_id)
            .bind(i64::from(limit))
            .fetch_all(&self.pool)
            .await?
            .iter()
            .map(row_to_job)
            .collect()
    }

    /// Delete the finished job runs of a service beyond the newest `keep`
    /// finished ones. Returns the ids of the deleted runs (so their log files
    /// can be removed). Pending/running jobs are never deleted.
    pub async fn prune_job_runs(&self, service_id: &str, keep: u32) -> Result<Vec<String>> {
        let ids: Vec<String> = sqlx::query_scalar(
            "SELECT id FROM job_runs WHERE service_id = ?1 AND status NOT IN ('pending', 'running')
             ORDER BY created_at DESC, rowid DESC LIMIT -1 OFFSET ?2",
        )
        .bind(service_id)
        .bind(i64::from(keep))
        .fetch_all(&self.pool)
        .await?;
        for id in &ids {
            sqlx::query("DELETE FROM job_runs WHERE id = ?").bind(id).execute(&self.pool).await?;
        }
        Ok(ids)
    }

    /// Pending / running jobs of all services, oldest first.
    pub async fn active_job_runs(&self) -> Result<Vec<JobRun>> {
        sqlx::query("SELECT * FROM job_runs WHERE status IN ('pending', 'running') ORDER BY created_at, rowid")
            .fetch_all(&self.pool)
            .await?
            .iter()
            .map(row_to_job)
            .collect()
    }

    // -----------------------------------------------------------------------
    // datastores

    /// Insert a datastore. Conflict if the name is taken by a service or
    /// datastore of its environment; NotFound if its environment doesn't exist.
    pub async fn create_datastore(&self, d: &Datastore) -> Result<()> {
        if self.name_taken_in(&d.environment, &d.name).await? {
            return Err(name_in_use(&d.name, &d.environment));
        }
        self.require_environment_named(&d.environment).await?;
        let res = sqlx::query(
            "INSERT INTO datastores (id, name, environment, kind, version, status, username, password, database,
                host_port, memory_limit_mb, cpu_limit, error, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&d.id)
        .bind(&d.name)
        .bind(&d.environment)
        .bind(d.kind.as_str())
        .bind(&d.version)
        .bind(d.status.as_str())
        .bind(&d.username)
        .bind(&d.password)
        .bind(&d.database)
        .bind(d.host_port.map(i64::from))
        .bind(d.memory_limit_mb.map(i64::from))
        .bind(d.cpu_limit)
        .bind(&d.error)
        .bind(ts(&d.created_at))
        .bind(ts(&d.updated_at))
        .execute(&self.pool)
        .await;
        match res {
            Ok(_) => Ok(()),
            Err(e) if is_unique_violation(&e) => Err(name_in_use(&d.name, &d.environment)),
            Err(e) => Err(e.into()),
        }
    }

    /// Persist the engine-owned columns (version, status, credentials,
    /// database, host port, error) and bump `updated_at`. Returns the stored
    /// row. Resource limits are NOT written (use
    /// [`Store::set_datastore_limits`]), so the engine persisting a stale row
    /// can never revert a limits change.
    pub async fn update_datastore(&self, d: &Datastore) -> Result<Datastore> {
        let res = sqlx::query(
            "UPDATE datastores SET version = ?, status = ?, username = ?, password = ?, database = ?, host_port = ?,
                error = ?, updated_at = ?
             WHERE id = ?",
        )
        .bind(&d.version)
        .bind(d.status.as_str())
        .bind(&d.username)
        .bind(&d.password)
        .bind(&d.database)
        .bind(d.host_port.map(i64::from))
        .bind(&d.error)
        .bind(ts(&Utc::now()))
        .bind(&d.id)
        .execute(&self.pool)
        .await?;
        if res.rows_affected() == 0 {
            return Err(Error::not_found("datastore", &d.id));
        }
        self.require_datastore(&d.id).await
    }

    /// Set a datastore's resource limits (`None` = the server default) and
    /// bump `updated_at`. Returns the stored row.
    pub async fn set_datastore_limits(
        &self,
        id: &str,
        memory_limit_mb: Option<u32>,
        cpu_limit: Option<f64>,
    ) -> Result<Datastore> {
        let res = sqlx::query("UPDATE datastores SET memory_limit_mb = ?, cpu_limit = ?, updated_at = ? WHERE id = ?")
            .bind(memory_limit_mb.map(i64::from))
            .bind(cpu_limit)
            .bind(ts(&Utc::now()))
            .bind(id)
            .execute(&self.pool)
            .await?;
        if res.rows_affected() == 0 {
            return Err(Error::not_found("datastore", id));
        }
        self.require_datastore(id).await
    }

    /// Look up by id, or by name in `production` ([`Store::find_datastore_in`]).
    pub async fn find_datastore(&self, id_or_name: &str) -> Result<Option<Datastore>> {
        self.find_datastore_in(id_or_name, None).await
    }

    /// Look up by id or name, as [`Store::find_service_in`]: a name in
    /// `environment` (`None` = `production`), an id in any environment unless
    /// `environment` names another one.
    pub async fn find_datastore_in(&self, id_or_name: &str, environment: Option<&str>) -> Result<Option<Datastore>> {
        sqlx::query(
            "SELECT * FROM datastores
             WHERE (id = ?1 AND (?2 IS NULL OR environment = ?2)) OR (name = ?1 AND environment = COALESCE(?2, ?3))
             ORDER BY (id = ?1) DESC LIMIT 1",
        )
        .bind(id_or_name)
        .bind(environment)
        .bind(PRODUCTION)
        .fetch_optional(&self.pool)
        .await?
        .map(|r| row_to_datastore(&r))
        .transpose()
    }

    /// Look up by id, or by name in `production`; NotFound if missing.
    pub async fn require_datastore(&self, id_or_name: &str) -> Result<Datastore> {
        self.require_datastore_in(id_or_name, None).await
    }

    /// [`Store::find_datastore_in`]; NotFound if missing.
    pub async fn require_datastore_in(&self, id_or_name: &str, environment: Option<&str>) -> Result<Datastore> {
        self.find_datastore_in(id_or_name, environment)
            .await?
            .ok_or_else(|| not_found_in("datastore", id_or_name, environment))
    }

    /// All datastores, ordered as [`Store::list_services`].
    pub async fn list_datastores(&self) -> Result<Vec<Datastore>> {
        sqlx::query("SELECT * FROM datastores ORDER BY name, (environment = ?) DESC, environment")
            .bind(PRODUCTION)
            .fetch_all(&self.pool)
            .await?
            .iter()
            .map(row_to_datastore)
            .collect()
    }

    pub async fn delete_datastore(&self, id: &str) -> Result<()> {
        let res = sqlx::query("DELETE FROM datastores WHERE id = ?").bind(id).execute(&self.pool).await?;
        if res.rows_affected() == 0 {
            return Err(Error::not_found("datastore", id));
        }
        Ok(())
    }

    // -----------------------------------------------------------------------
    // environments

    /// Insert an environment. Conflict if the name is taken.
    pub async fn create_environment(&self, e: &Environment) -> Result<()> {
        let res = sqlx::query("INSERT INTO environments (id, name, created_at) VALUES (?, ?, ?)")
            .bind(&e.id)
            .bind(&e.name)
            .bind(ts(&e.created_at))
            .execute(&self.pool)
            .await;
        match res {
            Ok(_) => Ok(()),
            Err(err) if is_unique_violation(&err) => {
                Err(Error::conflict(format!("environment '{}' already exists", e.name)))
            }
            Err(err) => Err(err.into()),
        }
    }

    /// Look up by id or name.
    pub async fn find_environment(&self, id_or_name: &str) -> Result<Option<Environment>> {
        sqlx::query("SELECT * FROM environments WHERE id = ?1 OR name = ?1 ORDER BY (id = ?1) DESC LIMIT 1")
            .bind(id_or_name)
            .fetch_optional(&self.pool)
            .await?
            .map(|r| row_to_environment(&r))
            .transpose()
    }

    /// Look up by id or name; NotFound if missing.
    pub async fn require_environment(&self, id_or_name: &str) -> Result<Environment> {
        self.find_environment(id_or_name).await?.ok_or_else(|| Error::not_found("environment", id_or_name))
    }

    /// The environment named `name` (never an id: rows hold names); NotFound
    /// if missing.
    async fn require_environment_named(&self, name: &str) -> Result<Environment> {
        sqlx::query("SELECT * FROM environments WHERE name = ?")
            .bind(name)
            .fetch_optional(&self.pool)
            .await?
            .map(|r| row_to_environment(&r))
            .transpose()?
            .ok_or_else(|| Error::not_found("environment", name))
    }

    /// Every environment: `production` first, then by name.
    pub async fn list_environments(&self) -> Result<Vec<Environment>> {
        sqlx::query("SELECT * FROM environments ORDER BY (name = ?) DESC, name")
            .bind(PRODUCTION)
            .fetch_all(&self.pool)
            .await?
            .iter()
            .map(row_to_environment)
            .collect()
    }

    /// The services and datastores of an environment (by name), each ordered
    /// by name.
    pub async fn environment_members(&self, environment: &str) -> Result<(Vec<Service>, Vec<Datastore>)> {
        let services = sqlx::query("SELECT * FROM services WHERE environment = ? ORDER BY name")
            .bind(environment)
            .fetch_all(&self.pool)
            .await?
            .iter()
            .map(row_to_service)
            .collect::<Result<Vec<_>>>()?;
        let datastores = sqlx::query("SELECT * FROM datastores WHERE environment = ? ORDER BY name")
            .bind(environment)
            .fetch_all(&self.pool)
            .await?
            .iter()
            .map(row_to_datastore)
            .collect::<Result<Vec<_>>>()?;
        Ok((services, datastores))
    }

    /// Delete an empty environment. Conflict for `production`, or while it
    /// holds services or datastores (the message lists them).
    pub async fn delete_environment(&self, id: &str) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        let env = match sqlx::query("SELECT * FROM environments WHERE id = ?").bind(id).fetch_optional(&mut *tx).await?
        {
            Some(row) => row_to_environment(&row)?,
            None => return Err(Error::not_found("environment", id)),
        };
        if env.is_production() {
            return Err(Error::conflict("environment 'production' can't be deleted"));
        }
        let mut held = Vec::new();
        for row in sqlx::query(
            "SELECT 'service' AS kind, name FROM services WHERE environment = ?1
             UNION ALL SELECT 'datastore' AS kind, name FROM datastores WHERE environment = ?1
             ORDER BY kind DESC, name",
        )
        .bind(&env.name)
        .fetch_all(&mut *tx)
        .await?
        {
            let (kind, name): (String, String) = (row.try_get("kind")?, row.try_get("name")?);
            held.push(format!("{kind} '{name}'"));
        }
        if !held.is_empty() {
            return Err(Error::conflict(format!(
                "environment '{}' still holds {}: delete them first",
                env.name,
                held.join(", ")
            )));
        }
        sqlx::query("DELETE FROM environments WHERE id = ?").bind(id).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // git connections

    /// Insert a git connection. Conflict if the same account of the same
    /// provider instance is already connected.
    pub async fn create_git_connection(&self, c: &GitConnection) -> Result<()> {
        let res = sqlx::query(
            "INSERT INTO git_connections (id, provider, base_url, auth, account, account_name, token, refresh_token,
                token_expires_at, scopes, client_id, client_secret, app_id, app_slug, app_url, private_key,
                webhook_secret, installation_id, installation_url, repository_selection, created_at, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&c.id)
        .bind(c.provider.as_str())
        .bind(&c.base_url)
        .bind(c.auth.as_str())
        .bind(&c.account)
        .bind(&c.account_name)
        .bind(&c.token)
        .bind(&c.refresh_token)
        .bind(ts_opt(&c.token_expires_at))
        .bind(serde_json::to_string(&c.scopes).unwrap_or_else(|_| "[]".into()))
        .bind(&c.client_id)
        .bind(&c.client_secret)
        .bind(sql_u64(c.app.as_ref().map(|a| a.id)))
        .bind(c.app.as_ref().map(|a| a.slug.as_str()))
        .bind(c.app.as_ref().map(|a| a.url.as_str()))
        .bind(c.app.as_ref().map(|a| a.private_key.as_str()))
        .bind(c.app.as_ref().and_then(|a| a.webhook_secret.as_deref()))
        .bind(sql_u64(c.installation.as_ref().map(|i| i.id)))
        .bind(c.installation.as_ref().and_then(|i| i.url.as_deref()))
        .bind(c.installation.as_ref().and_then(|i| i.repository_selection.as_deref()))
        .bind(ts(&c.created_at))
        .bind(ts(&c.updated_at))
        .execute(&self.pool)
        .await;
        match res {
            Ok(_) => Ok(()),
            Err(e) if is_unique_violation(&e) => {
                Err(Error::conflict(format!("the {} is already connected", c.describe())))
            }
            Err(e) => Err(e.into()),
        }
    }

    /// Persist everything a (re)authorization changes — the account, how it
    /// is authorized and its secrets — and bump `updated_at`. Returns the
    /// stored row. Provider and instance are immutable. Conflict if the
    /// account became one that has another connection.
    pub async fn update_git_connection(&self, c: &GitConnection) -> Result<GitConnection> {
        let res = sqlx::query(
            "UPDATE git_connections SET auth = ?, account = ?, account_name = ?, token = ?, refresh_token = ?,
                token_expires_at = ?, scopes = ?, client_id = ?, client_secret = ?, app_id = ?, app_slug = ?,
                app_url = ?, private_key = ?, webhook_secret = ?, installation_id = ?, installation_url = ?,
                repository_selection = ?, updated_at = ?
             WHERE id = ?",
        )
        .bind(c.auth.as_str())
        .bind(&c.account)
        .bind(&c.account_name)
        .bind(&c.token)
        .bind(&c.refresh_token)
        .bind(ts_opt(&c.token_expires_at))
        .bind(serde_json::to_string(&c.scopes).unwrap_or_else(|_| "[]".into()))
        .bind(&c.client_id)
        .bind(&c.client_secret)
        .bind(sql_u64(c.app.as_ref().map(|a| a.id)))
        .bind(c.app.as_ref().map(|a| a.slug.as_str()))
        .bind(c.app.as_ref().map(|a| a.url.as_str()))
        .bind(c.app.as_ref().map(|a| a.private_key.as_str()))
        .bind(c.app.as_ref().and_then(|a| a.webhook_secret.as_deref()))
        .bind(sql_u64(c.installation.as_ref().map(|i| i.id)))
        .bind(c.installation.as_ref().and_then(|i| i.url.as_deref()))
        .bind(c.installation.as_ref().and_then(|i| i.repository_selection.as_deref()))
        .bind(ts(&Utc::now()))
        .bind(&c.id)
        .execute(&self.pool)
        .await;
        let res = match res {
            Ok(res) => res,
            Err(e) if is_unique_violation(&e) => {
                return Err(Error::conflict(format!("the {} is already connected", c.describe())));
            }
            Err(e) => return Err(e.into()),
        };
        if res.rows_affected() == 0 {
            return Err(Error::not_found("git connection", &c.id));
        }
        self.require_git_connection(&c.id).await
    }

    /// Store a renewed OAuth access token, with the refresh token that
    /// replaces the one just used. Unlike [`Store::update_git_connection`]
    /// this leaves `updated_at` alone: nothing about the connection changed
    /// for its users.
    pub async fn set_git_tokens(
        &self,
        id: &str,
        token: &str,
        refresh_token: Option<&str>,
        expires_at: Option<DateTime<Utc>>,
    ) -> Result<()> {
        let res =
            sqlx::query("UPDATE git_connections SET token = ?, refresh_token = ?, token_expires_at = ? WHERE id = ?")
                .bind(token)
                .bind(refresh_token)
                .bind(ts_opt(&expires_at))
                .bind(id)
                .execute(&self.pool)
                .await?;
        if res.rows_affected() == 0 {
            return Err(Error::not_found("git connection", id));
        }
        Ok(())
    }

    pub async fn get_git_connection(&self, id: &str) -> Result<Option<GitConnection>> {
        sqlx::query("SELECT * FROM git_connections WHERE id = ?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?
            .map(|r| row_to_git_connection(&r))
            .transpose()
    }

    pub async fn require_git_connection(&self, id: &str) -> Result<GitConnection> {
        self.get_git_connection(id).await?.ok_or_else(|| Error::not_found("git connection", id))
    }

    /// The connection of an account (its login compared case-insensitively)
    /// on a provider instance, if any. The empty account is the instance's
    /// OAuth application that waits for its first authorization.
    pub async fn find_git_connection(
        &self,
        provider: GitProvider,
        base_url: &str,
        account: &str,
    ) -> Result<Option<GitConnection>> {
        sqlx::query("SELECT * FROM git_connections WHERE provider = ? AND base_url = ? AND account = ?")
            .bind(provider.as_str())
            .bind(base_url)
            .bind(account)
            .fetch_optional(&self.pool)
            .await?
            .map(|r| row_to_git_connection(&r))
            .transpose()
    }

    /// All git connections, ordered by provider, instance and account.
    pub async fn list_git_connections(&self) -> Result<Vec<GitConnection>> {
        sqlx::query("SELECT * FROM git_connections ORDER BY provider, base_url, account")
            .fetch_all(&self.pool)
            .await?
            .iter()
            .map(row_to_git_connection)
            .collect()
    }

    /// Delete a git connection. The repositories it served are cloned
    /// without credentials from then on.
    pub async fn delete_git_connection(&self, id: &str) -> Result<()> {
        let res = sqlx::query("DELETE FROM git_connections WHERE id = ?").bind(id).execute(&self.pool).await?;
        if res.rows_affected() == 0 {
            return Err(Error::not_found("git connection", id));
        }
        Ok(())
    }

    // -----------------------------------------------------------------------
    // domains (DESIGN.md §21)

    /// Insert a domain. Conflict if a domain of that name is already there.
    pub async fn create_domain(&self, d: &Domain) -> Result<()> {
        let res = sqlx::query(
            "INSERT INTO domains (id, name, source, status, is_default, checks, failures, created_at, verified_at,
                checked_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&d.id)
        .bind(&d.name)
        .bind(d.source.as_str())
        .bind(d.status.as_str())
        .bind(d.is_default)
        .bind(serde_json::to_string(&d.checks).unwrap_or_else(|_| "[]".into()))
        .bind(i64::from(d.failures))
        .bind(ts(&d.created_at))
        .bind(ts_opt(&d.verified_at))
        .bind(ts_opt(&d.checked_at))
        .execute(&self.pool)
        .await;
        match res {
            Ok(_) => Ok(()),
            Err(e) if is_unique_violation(&e) => {
                Err(Error::conflict(format!("domain '{}' is already connected", d.name)))
            }
            Err(e) => Err(e.into()),
        }
    }

    pub async fn get_domain(&self, id: &str) -> Result<Option<Domain>> {
        sqlx::query("SELECT * FROM domains WHERE id = ?")
            .bind(id)
            .fetch_optional(&self.pool)
            .await?
            .map(|r| row_to_domain(&r))
            .transpose()
    }

    /// A domain by id, else by name (names are stored in lowercase).
    pub async fn find_domain(&self, id_or_name: &str) -> Result<Option<Domain>> {
        if let Some(d) = self.get_domain(id_or_name).await? {
            return Ok(Some(d));
        }
        let name = id_or_name.trim().trim_end_matches('.').to_ascii_lowercase();
        sqlx::query("SELECT * FROM domains WHERE name = ?")
            .bind(name)
            .fetch_optional(&self.pool)
            .await?
            .map(|r| row_to_domain(&r))
            .transpose()
    }

    pub async fn require_domain(&self, id_or_name: &str) -> Result<Domain> {
        self.find_domain(id_or_name).await?.ok_or_else(|| Error::not_found("domain", id_or_name))
    }

    /// Every domain: the default one first, then in the order they were added.
    pub async fn list_domains(&self) -> Result<Vec<Domain>> {
        sqlx::query("SELECT * FROM domains ORDER BY is_default DESC, created_at, rowid")
            .fetch_all(&self.pool)
            .await?
            .iter()
            .map(row_to_domain)
            .collect()
    }

    /// Store what a verification found: `status`, `checks`, `failures`,
    /// `verified_at` and `checked_at` of `d`. Returns the stored row.
    pub async fn record_domain_check(&self, d: &Domain) -> Result<Domain> {
        let res = sqlx::query(
            "UPDATE domains SET status = ?, checks = ?, failures = ?, verified_at = ?, checked_at = ? WHERE id = ?",
        )
        .bind(d.status.as_str())
        .bind(serde_json::to_string(&d.checks).unwrap_or_else(|_| "[]".into()))
        .bind(i64::from(d.failures))
        .bind(ts_opt(&d.verified_at))
        .bind(ts_opt(&d.checked_at))
        .bind(&d.id)
        .execute(&self.pool)
        .await?;
        if res.rows_affected() == 0 {
            return Err(Error::not_found("domain", &d.id));
        }
        self.get_domain(&d.id).await?.ok_or_else(|| Error::not_found("domain", &d.id))
    }

    /// Make `id` the default domain, and no other.
    pub async fn set_default_domain(&self, id: &str) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        let found: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM domains WHERE id = ?").bind(id).fetch_one(&mut *tx).await?;
        if found == 0 {
            return Err(Error::not_found("domain", id));
        }
        sqlx::query("UPDATE domains SET is_default = 0 WHERE is_default = 1").execute(&mut *tx).await?;
        sqlx::query("UPDATE domains SET is_default = 1 WHERE id = ?").bind(id).execute(&mut *tx).await?;
        tx.commit().await?;
        Ok(())
    }

    /// Delete a domain. When it was the default one, the base domain is again.
    pub async fn delete_domain(&self, id: &str) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        let res = sqlx::query("DELETE FROM domains WHERE id = ?").bind(id).execute(&mut *tx).await?;
        if res.rows_affected() == 0 {
            return Err(Error::not_found("domain", id));
        }
        sqlx::query(
            "UPDATE domains SET is_default = 1
             WHERE source = ? AND NOT EXISTS (SELECT 1 FROM domains WHERE is_default = 1)",
        )
        .bind(DomainSource::Config.as_str())
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    /// Keep the row of the server's `--base-domain` in step with the flag,
    /// at every start: the row of a previous value goes, a domain of that
    /// name connected earlier becomes the base domain, and the base domain
    /// is the default one unless another domain is. Returns its row.
    pub async fn sync_base_domain(&self, base_domain: &str) -> Result<Domain> {
        let name = base_domain.trim().trim_end_matches('.').to_ascii_lowercase();
        let config = DomainSource::Config.as_str();
        let mut tx = self.pool.begin().await?;
        sqlx::query("DELETE FROM domains WHERE source = ? AND name != ?")
            .bind(config)
            .bind(&name)
            .execute(&mut *tx)
            .await?;
        let existing: Option<String> =
            sqlx::query_scalar("SELECT id FROM domains WHERE name = ?").bind(&name).fetch_optional(&mut *tx).await?;
        let id = match existing {
            Some(id) => {
                sqlx::query("UPDATE domains SET source = ? WHERE id = ?")
                    .bind(config)
                    .bind(&id)
                    .execute(&mut *tx)
                    .await?;
                id
            }
            None => {
                let d = Domain::new(name.clone(), DomainSource::Config);
                sqlx::query("INSERT INTO domains (id, name, source, status, created_at) VALUES (?, ?, ?, ?, ?)")
                    .bind(&d.id)
                    .bind(&d.name)
                    .bind(config)
                    .bind(d.status.as_str())
                    .bind(ts(&d.created_at))
                    .execute(&mut *tx)
                    .await?;
                d.id
            }
        };
        let defaults: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM domains WHERE is_default = 1").fetch_one(&mut *tx).await?;
        if defaults == 0 {
            sqlx::query("UPDATE domains SET is_default = 1 WHERE id = ?").bind(&id).execute(&mut *tx).await?;
        }
        let row = sqlx::query("SELECT * FROM domains WHERE id = ?").bind(&id).fetch_one(&mut *tx).await?;
        let domain = row_to_domain(&row)?;
        tx.commit().await?;
        Ok(domain)
    }

    // -----------------------------------------------------------------------
    // accounts, sessions and API tokens (DESIGN.md §20)

    /// The administrator's account, once the server is set up.
    pub async fn first_user(&self) -> Result<Option<User>> {
        let row = sqlx::query("SELECT * FROM users ORDER BY created_at, id LIMIT 1").fetch_optional(&self.pool).await?;
        row.as_ref().map(row_to_user).transpose()
    }

    /// Create the account of a server that has none. `false` when one exists
    /// already (nothing is written): of two setups at once, one wins.
    pub async fn create_first_user(&self, u: &User) -> Result<bool> {
        let res = sqlx::query(
            "INSERT INTO users (id, email, password_hash, created_at, updated_at)
             SELECT ?, ?, ?, ?, ? WHERE NOT EXISTS (SELECT 1 FROM users)",
        )
        .bind(&u.id)
        .bind(&u.email)
        .bind(&u.password_hash)
        .bind(ts(&u.created_at))
        .bind(ts(&u.updated_at))
        .execute(&self.pool)
        .await?;
        Ok(res.rows_affected() > 0)
    }

    pub async fn get_user(&self, id: &str) -> Result<Option<User>> {
        let row = sqlx::query("SELECT * FROM users WHERE id = ?").bind(id).fetch_optional(&self.pool).await?;
        row.as_ref().map(row_to_user).transpose()
    }

    /// `email` as [`crate::auth::normalize_email`] returns it.
    pub async fn find_user_by_email(&self, email: &str) -> Result<Option<User>> {
        let row = sqlx::query("SELECT * FROM users WHERE email = ?").bind(email).fetch_optional(&self.pool).await?;
        row.as_ref().map(row_to_user).transpose()
    }

    /// Replace the password of an account and bump `updated_at`.
    pub async fn set_password(&self, user_id: &str, password_hash: &str) -> Result<()> {
        let res = sqlx::query("UPDATE users SET password_hash = ?, updated_at = ? WHERE id = ?")
            .bind(password_hash)
            .bind(ts(&now()))
            .bind(user_id)
            .execute(&self.pool)
            .await?;
        if res.rows_affected() == 0 {
            return Err(Error::not_found("account", user_id));
        }
        Ok(())
    }

    pub async fn create_session(&self, s: &Session) -> Result<()> {
        sqlx::query(
            "INSERT INTO sessions (id, user_id, token_hash, user_agent, created_at, last_used_at, expires_at)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&s.id)
        .bind(&s.user_id)
        .bind(&s.token_hash)
        .bind(&s.user_agent)
        .bind(ts(&s.created_at))
        .bind(ts(&s.last_used_at))
        .bind(ts(&s.expires_at))
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// The session of a cookie, by the digest of its value. Expired ones
    /// are returned too: the caller decides.
    pub async fn find_session(&self, token_hash: &str) -> Result<Option<Session>> {
        let row = sqlx::query("SELECT * FROM sessions WHERE token_hash = ?")
            .bind(token_hash)
            .fetch_optional(&self.pool)
            .await?;
        row.as_ref().map(row_to_session).transpose()
    }

    /// Record that a session was used, and until when it now lasts.
    pub async fn touch_session(&self, id: &str, used_at: DateTime<Utc>, expires_at: DateTime<Utc>) -> Result<()> {
        sqlx::query("UPDATE sessions SET last_used_at = ?, expires_at = ? WHERE id = ?")
            .bind(ts(&used_at))
            .bind(ts(&expires_at))
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Sessions of an account that haven't expired, the last used first.
    pub async fn list_sessions(&self, user_id: &str) -> Result<Vec<Session>> {
        let rows =
            sqlx::query("SELECT * FROM sessions WHERE user_id = ? AND expires_at > ? ORDER BY last_used_at DESC, id")
                .bind(user_id)
                .bind(ts(&now()))
                .fetch_all(&self.pool)
                .await?;
        rows.iter().map(row_to_session).collect()
    }

    /// End a session; returns whether it existed.
    pub async fn delete_session(&self, id: &str) -> Result<bool> {
        let res = sqlx::query("DELETE FROM sessions WHERE id = ?").bind(id).execute(&self.pool).await?;
        Ok(res.rows_affected() > 0)
    }

    /// End every session of an account but `except`; returns how many.
    pub async fn delete_sessions(&self, user_id: &str, except: Option<&str>) -> Result<u64> {
        let res = sqlx::query("DELETE FROM sessions WHERE user_id = ? AND id != ?")
            .bind(user_id)
            .bind(except.unwrap_or_default())
            .execute(&self.pool)
            .await?;
        Ok(res.rows_affected())
    }

    /// Forget the sessions that expired; returns how many.
    pub async fn delete_expired_sessions(&self) -> Result<u64> {
        let res =
            sqlx::query("DELETE FROM sessions WHERE expires_at <= ?").bind(ts(&now())).execute(&self.pool).await?;
        Ok(res.rows_affected())
    }

    pub async fn create_api_token(&self, t: &ApiToken) -> Result<()> {
        sqlx::query(
            "INSERT INTO api_tokens (id, name, token_hash, hint, created_at, last_used_at, expires_at)
             VALUES (?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&t.id)
        .bind(&t.name)
        .bind(&t.token_hash)
        .bind(&t.hint)
        .bind(ts(&t.created_at))
        .bind(ts_opt(&t.last_used_at))
        .bind(ts_opt(&t.expires_at))
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// The API token with this digest. An expired one is returned too: the
    /// caller decides.
    pub async fn find_api_token(&self, token_hash: &str) -> Result<Option<ApiToken>> {
        let row = sqlx::query("SELECT * FROM api_tokens WHERE token_hash = ?")
            .bind(token_hash)
            .fetch_optional(&self.pool)
            .await?;
        row.as_ref().map(row_to_api_token).transpose()
    }

    pub async fn touch_api_token(&self, id: &str, used_at: DateTime<Utc>) -> Result<()> {
        sqlx::query("UPDATE api_tokens SET last_used_at = ? WHERE id = ?")
            .bind(ts(&used_at))
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// Every API token, the newest first.
    pub async fn list_api_tokens(&self) -> Result<Vec<ApiToken>> {
        let rows = sqlx::query("SELECT * FROM api_tokens ORDER BY created_at DESC, id").fetch_all(&self.pool).await?;
        rows.iter().map(row_to_api_token).collect()
    }

    /// Revoke an API token.
    pub async fn delete_api_token(&self, id: &str) -> Result<()> {
        let res = sqlx::query("DELETE FROM api_tokens WHERE id = ?").bind(id).execute(&self.pool).await?;
        if res.rows_affected() == 0 {
            return Err(Error::not_found("API token", id));
        }
        Ok(())
    }

    // -----------------------------------------------------------------------
    // settings (small key/value store for server-level state)

    pub async fn get_setting(&self, key: &str) -> Result<Option<String>> {
        Ok(sqlx::query_scalar("SELECT value FROM settings WHERE key = ?").bind(key).fetch_optional(&self.pool).await?)
    }

    pub async fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        sqlx::query(
            "INSERT INTO settings (key, value) VALUES (?, ?) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        )
        .bind(key)
        .bind(value)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Delete a setting; returns whether it existed.
    pub async fn delete_setting(&self, key: &str) -> Result<bool> {
        let res = sqlx::query("DELETE FROM settings WHERE key = ?").bind(key).execute(&self.pool).await?;
        Ok(res.rows_affected() > 0)
    }

    // -----------------------------------------------------------------------
    // metrics history (DESIGN.md §22)

    /// Record one reading of the server, in one transaction: the host at
    /// `host.at` (to the second; a second reading in the same second replaces
    /// the first) and its containers' CPU and memory (their limits are not
    /// kept). A container without either value is left out.
    pub async fn insert_metrics(&self, host: &HostPoint, containers: &[ContainerMetrics]) -> Result<()> {
        let at = host.at.timestamp();
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            "INSERT OR REPLACE INTO host_samples (at, cpu_percent, memory_used_bytes, memory_total_bytes,
                 disk_read_bytes_per_sec, disk_write_bytes_per_sec, network_rx_bytes_per_sec,
                 network_tx_bytes_per_sec)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(at)
        .bind(host.cpu_percent)
        .bind(sql_u64(host.memory_used_bytes))
        .bind(sql_u64(host.memory_total_bytes))
        .bind(sql_u64(host.disk_read_bytes_per_sec))
        .bind(sql_u64(host.disk_write_bytes_per_sec))
        .bind(sql_u64(host.network_rx_bytes_per_sec))
        .bind(sql_u64(host.network_tx_bytes_per_sec))
        .execute(&mut *tx)
        .await?;
        for c in containers.iter().filter(|c| c.cpu_percent.is_some() || c.memory_bytes.is_some()) {
            sqlx::query(
                "INSERT OR REPLACE INTO container_samples
                     (at, kind, owner_id, instance, name, cpu_percent, memory_bytes)
                 VALUES (?, ?, ?, ?, ?, ?, ?)",
            )
            .bind(at)
            .bind(container_kind_str(c.kind))
            .bind(&c.id)
            .bind(c.instance.as_deref().unwrap_or_default())
            .bind(&c.name)
            .bind(c.cpu_percent)
            .bind(sql_u64(c.memory_bytes))
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(())
    }

    /// The readings taken from `from` to `to` (both included), averaged into
    /// steps of `step` seconds counted from the Unix epoch: one point per
    /// step that has a reading, at the start of the step, oldest first. A
    /// value is the average of the readings that have it, `None` when none
    /// has. The containers come in no particular order, each named as in
    /// its last reading.
    pub async fn metrics_history(
        &self,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
        step: u32,
    ) -> Result<(Vec<HostPoint>, Vec<ContainerSeries>)> {
        let step = i64::from(step.max(1));
        let rows = sqlx::query(
            "SELECT (at / ?) * ? AS step_at, AVG(cpu_percent) AS cpu_percent,
                    AVG(memory_used_bytes) AS memory_used_bytes, AVG(memory_total_bytes) AS memory_total_bytes,
                    AVG(disk_read_bytes_per_sec) AS disk_read_bytes_per_sec,
                    AVG(disk_write_bytes_per_sec) AS disk_write_bytes_per_sec,
                    AVG(network_rx_bytes_per_sec) AS network_rx_bytes_per_sec,
                    AVG(network_tx_bytes_per_sec) AS network_tx_bytes_per_sec
             FROM host_samples WHERE at BETWEEN ? AND ?
             GROUP BY step_at ORDER BY step_at",
        )
        .bind(step)
        .bind(step)
        .bind(from.timestamp())
        .bind(to.timestamp())
        .fetch_all(&self.pool)
        .await?;
        let host = rows
            .iter()
            .map(|r| {
                Ok(HostPoint {
                    at: get_unix_time(r, "step_at")?,
                    cpu_percent: r.try_get("cpu_percent")?,
                    memory_used_bytes: get_avg_u64(r, "memory_used_bytes")?,
                    memory_total_bytes: get_avg_u64(r, "memory_total_bytes")?,
                    disk_read_bytes_per_sec: get_avg_u64(r, "disk_read_bytes_per_sec")?,
                    disk_write_bytes_per_sec: get_avg_u64(r, "disk_write_bytes_per_sec")?,
                    network_rx_bytes_per_sec: get_avg_u64(r, "network_rx_bytes_per_sec")?,
                    network_tx_bytes_per_sec: get_avg_u64(r, "network_tx_bytes_per_sec")?,
                })
            })
            .collect::<Result<Vec<_>>>()?;

        // `kind` and `name` are the same in every row of a series (but for a
        // renamed service): SQLite takes them from any row of the step.
        let rows = sqlx::query(
            "SELECT owner_id, instance, kind, name, (at / ?) * ? AS step_at, AVG(cpu_percent) AS cpu_percent,
                    AVG(memory_bytes) AS memory_bytes
             FROM container_samples WHERE at BETWEEN ? AND ?
             GROUP BY owner_id, instance, step_at ORDER BY owner_id, instance, step_at",
        )
        .bind(step)
        .bind(step)
        .bind(from.timestamp())
        .bind(to.timestamp())
        .fetch_all(&self.pool)
        .await?;
        let mut containers: Vec<ContainerSeries> = Vec::new();
        for r in &rows {
            let id: String = r.try_get("owner_id")?;
            let instance: String = r.try_get("instance")?;
            let instance = (!instance.is_empty()).then_some(instance);
            let point = ContainerPoint {
                at: get_unix_time(r, "step_at")?,
                cpu_percent: r.try_get("cpu_percent")?,
                memory_bytes: get_avg_u64(r, "memory_bytes")?,
            };
            let name: String = r.try_get("name")?;
            match containers.last_mut() {
                Some(series) if series.id == id && series.instance == instance => {
                    series.name = name;
                    series.points.push(point);
                }
                _ => containers.push(ContainerSeries {
                    kind: parse_container_kind(&r.try_get::<String, _>("kind")?)?,
                    id,
                    name,
                    instance,
                    points: vec![point],
                }),
            }
        }
        Ok((host, containers))
    }

    /// Delete the readings taken before `before`, the requests' too;
    /// returns how many rows went.
    pub async fn prune_metrics(&self, before: DateTime<Utc>) -> Result<u64> {
        let before = before.timestamp();
        let mut tx = self.pool.begin().await?;
        let mut rows = 0;
        for table in ["host_samples", "container_samples", "request_samples"] {
            let sql = format!("DELETE FROM {table} WHERE at < ?");
            rows += sqlx::query(&sql).bind(before).execute(&mut *tx).await?.rows_affected();
        }
        tx.commit().await?;
        Ok(rows)
    }

    /// Record, in one transaction, the requests each target got since the
    /// previous reading, at `at` (to the second; a second reading in the
    /// same second adds to the first). A target without a request is left
    /// out.
    pub async fn insert_requests(&self, at: DateTime<Utc>, requests: &[TargetRequests]) -> Result<()> {
        let columns = request_columns();
        let sql = format!(
            "INSERT INTO request_samples (at, target, name, {}) VALUES (?, ?, ?{})
             ON CONFLICT (at, target) DO UPDATE SET name = excluded.name, {}",
            columns.join(", "),
            ", ?".repeat(columns.len()),
            columns.iter().map(|c| format!("{c} = {c} + excluded.{c}")).collect::<Vec<_>>().join(", "),
        );
        let mut tx = self.pool.begin().await?;
        for r in requests.iter().filter(|r| !r.counts.is_empty()) {
            let mut query = sqlx::query(&sql).bind(at.timestamp()).bind(&r.target).bind(&r.name);
            for value in request_values(&r.counts) {
                query = query.bind(value);
            }
            query.execute(&mut *tx).await?;
        }
        tx.commit().await?;
        Ok(())
    }

    /// The requests counted from `from` to `to` (both included), each
    /// target's added up in steps of `step` seconds counted from the Unix
    /// epoch: one entry per step and target with requests, at the start of
    /// the step, oldest first (then by target), named as in the step's
    /// newest row.
    pub async fn request_history(
        &self,
        from: DateTime<Utc>,
        to: DateTime<Utc>,
        step: u32,
    ) -> Result<Vec<(DateTime<Utc>, TargetRequests)>> {
        let step = i64::from(step.max(1));
        let columns = request_columns();
        // With a single `MAX`, SQLite takes the bare `name` from the row
        // that has it: the newest of the step.
        let sql = format!(
            "SELECT (at / ?) * ? AS step_at, target, name, MAX(at) AS last_at, {} FROM request_samples
             WHERE at BETWEEN ? AND ?
             GROUP BY step_at, target ORDER BY step_at, target",
            request_sums()
        );
        let rows = sqlx::query(&sql)
            .bind(step)
            .bind(step)
            .bind(from.timestamp())
            .bind(to.timestamp())
            .fetch_all(&self.pool)
            .await?;
        rows.iter()
            .map(|r| {
                Ok((
                    get_unix_time(r, "step_at")?,
                    TargetRequests {
                        target: r.try_get("target")?,
                        name: r.try_get("name")?,
                        counts: row_to_request_counts(r, &columns)?,
                    },
                ))
            })
            .collect()
    }

    // -----------------------------------------------------------------------
    // derived data

    /// Datastores and service references for [`crate::env::RefContext`], of
    /// every environment (one name may then appear more than once: a service
    /// resolves its references with [`Store::environment_reference_targets`]).
    pub async fn reference_targets(&self, config: &Config) -> Result<(Vec<Datastore>, Vec<ServiceRef>)> {
        let datastores = self.list_datastores().await?;
        let refs = self.service_refs(config, self.list_services().await?).await?;
        Ok((datastores, refs))
    }

    /// [`Store::reference_targets`] for a service of `environment`: only the
    /// datastores and services of that environment (references resolve
    /// inside it, see [`crate::env::check_environment`]), so each name is
    /// there once.
    pub async fn environment_reference_targets(
        &self,
        config: &Config,
        environment: &str,
    ) -> Result<(Vec<Datastore>, Vec<ServiceRef>)> {
        let (services, datastores) = self.environment_members(environment).await?;
        let refs = self.service_refs(config, services).await?;
        Ok((datastores, refs))
    }

    /// What references to `services` resolve to. A service's port is its
    /// live deploy's port, else its configured port.
    async fn service_refs(&self, config: &Config, services: Vec<Service>) -> Result<Vec<ServiceRef>> {
        let mut refs = Vec::with_capacity(services.len());
        for s in services {
            // What is actually running wins over a pending (not yet deployed) setting.
            let mut port = None;
            if let Some(live) = &s.live_deploy_id {
                port = self.get_deploy(live).await?.and_then(|d| d.port);
            }
            let port = port.or(s.port);
            refs.push(ServiceRef { public_url: config.service_url(&s), name: s.name, port });
        }
        Ok(refs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn service_roundtrip_and_conflicts() {
        let store = Store::open_in_memory().await.unwrap();
        let mut svc = Service::new("web", ServiceType::WebService);
        svc.repo_url = Some("https://github.com/a/b".into());
        svc.port = Some(3000);
        svc.custom_domains = vec!["a.example.com".into()];
        store.create_service(&svc).await.unwrap();
        let got = store.require_service("web").await.unwrap();
        assert_eq!(got.id, svc.id);
        assert_eq!(got.custom_domains, svc.custom_domains);
        assert_eq!(got.port, Some(3000));
        assert_eq!(got.created_at, svc.created_at);

        let dup = Service::new("web", ServiceType::BackgroundWorker);
        assert!(matches!(store.create_service(&dup).await, Err(Error::Conflict(_))));
        let ds = Datastore::new("web", DatastoreKind::Redis);
        assert!(matches!(store.create_datastore(&ds).await, Err(Error::Conflict(_))));

        let mut upd = got.clone();
        upd.instances = 3;
        upd.suspended = true;
        upd.live_deploy_id = Some("dep-x".into());
        upd.start_command = Some("run".into());
        let upd = store.update_service(&upd).await.unwrap();
        // Engine-owned columns are not written by update_service.
        assert_eq!((upd.instances, upd.suspended, upd.live_deploy_id.as_deref()), (1, false, None));
        assert_eq!(upd.start_command.as_deref(), Some("run"));
        store.set_instances(&svc.id, 2).await.unwrap();
        store.set_suspended(&svc.id, false).await.unwrap();
        let s = store.require_service(&svc.id).await.unwrap();
        assert_eq!((s.instances, s.suspended), (2, false));
        assert_eq!(store.list_services().await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn deploys_and_jobs() {
        let store = Store::open_in_memory().await.unwrap();
        let svc = Service::new("api", ServiceType::WebService);
        store.create_service(&svc).await.unwrap();
        let d1 = Deploy::new(&svc.id, DeployTrigger::Create, DeploySource::Image { image: "nginx".into() });
        store.create_deploy(&d1).await.unwrap();
        let mut d2 = Deploy::new(&svc.id, DeployTrigger::Manual, DeploySource::Image { image: "nginx".into() });
        d2.created_at = d1.created_at + chrono::Duration::milliseconds(5);
        store.create_deploy(&d2).await.unwrap();
        assert_eq!(store.latest_deploy(&svc.id).await.unwrap().unwrap().id, d2.id);
        assert_eq!(store.active_deploys().await.unwrap().len(), 2);

        let b = store.set_deploy_status(&d1.id, DeployStatus::Building, None).await.unwrap();
        assert!(b.started_at.is_some() && b.finished_at.is_none());
        let mut live = store.set_deploy_status(&d1.id, DeployStatus::Live, None).await.unwrap();
        assert!(live.finished_at.is_some());
        live.image = Some("nginx".into());
        live.port = Some(80);
        store.update_deploy(&live).await.unwrap();
        store.set_live_deploy(&svc.id, Some(&d1.id)).await.unwrap();
        let (_, refs) = store.reference_targets(&Config::default()).await.unwrap();
        assert_eq!(refs[0].port, Some(80));
        assert_eq!(store.deploys_with_images(&svc.id).await.unwrap().len(), 1);
        let f = store.set_deploy_status(&d2.id, DeployStatus::BuildFailed, Some("boom")).await.unwrap();
        assert_eq!(f.error.as_deref(), Some("boom"));
        assert!(store.active_deploys().await.unwrap().is_empty());

        let mut j = JobRun::new(&svc.id, JobTrigger::Manual, Some("echo hi".into()));
        store.create_job_run(&j).await.unwrap();
        assert_eq!(store.active_job_runs().await.unwrap().len(), 1);
        j.status = JobStatus::Succeeded;
        j.exit_code = Some(0);
        store.update_job_run(&j).await.unwrap();
        assert_eq!(store.require_job_run(&j.id).await.unwrap().exit_code, Some(0));
        assert_eq!(store.list_job_runs(&svc.id, 10).await.unwrap().len(), 1);
        for _ in 0..3 {
            let mut done = JobRun::new(&svc.id, JobTrigger::Schedule, None);
            done.status = JobStatus::Succeeded;
            store.create_job_run(&done).await.unwrap();
        }
        let running = JobRun::new(&svc.id, JobTrigger::Manual, None);
        store.create_job_run(&running).await.unwrap();
        let pruned = store.prune_job_runs(&svc.id, 2).await.unwrap();
        assert_eq!(pruned.len(), 2, "4 finished runs, keep 2");
        assert!(store.get_job_run(&running.id).await.unwrap().is_some(), "pending runs are kept");

        store.delete_service(&svc.id).await.unwrap();
        assert!(store.get_deploy(&d1.id).await.unwrap().is_none());
        assert!(store.get_job_run(&j.id).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn the_first_account_is_the_only_one() {
        let store = Store::open_in_memory().await.unwrap();
        assert!(store.first_user().await.unwrap().is_none());
        let ada = User::new("ada@example.com", "hash-1");
        assert!(store.create_first_user(&ada).await.unwrap());
        // A second setup changes nothing.
        assert!(!store.create_first_user(&User::new("eve@example.com", "hash-2")).await.unwrap());
        assert_eq!(store.first_user().await.unwrap().unwrap(), ada);
        assert_eq!(store.get_user(&ada.id).await.unwrap().unwrap(), ada);
        assert_eq!(store.find_user_by_email("ada@example.com").await.unwrap().unwrap().id, ada.id);
        assert!(store.find_user_by_email("eve@example.com").await.unwrap().is_none());

        store.set_password(&ada.id, "hash-3").await.unwrap();
        let changed = store.get_user(&ada.id).await.unwrap().unwrap();
        assert_eq!(changed.password_hash, "hash-3");
        assert!(changed.updated_at >= ada.updated_at);
        assert!(store.set_password("usr-missing", "x").await.is_err());
        assert!(!format!("{changed:?}").contains("hash-3"), "no hash in debug output");
    }

    #[tokio::test]
    async fn sessions_round_trip_and_end() {
        let store = Store::open_in_memory().await.unwrap();
        let user = User::new("ada@example.com", "hash");
        store.create_first_user(&user).await.unwrap();
        let (a, b) = (Session::new(&user.id, "secret-a", Some("Firefox")), Session::new(&user.id, "secret-b", None));
        store.create_session(&a).await.unwrap();
        store.create_session(&b).await.unwrap();
        assert_eq!(store.find_session(&crate::auth::digest("secret-a")).await.unwrap().unwrap(), a);
        assert!(store.find_session("secret-a").await.unwrap().is_none(), "found by digest only");
        assert_eq!(store.list_sessions(&user.id).await.unwrap().len(), 2);

        // An expired session is no longer listed, and is swept.
        let past = now() - chrono::Duration::hours(1);
        store.touch_session(&b.id, past, past).await.unwrap();
        let found = store.find_session(&b.token_hash).await.unwrap().unwrap();
        assert!(found.is_expired(now()) && !a.is_expired(now()));
        assert_eq!(store.list_sessions(&user.id).await.unwrap(), vec![a.clone()]);
        assert_eq!(store.delete_expired_sessions().await.unwrap(), 1);
        assert!(store.find_session(&b.token_hash).await.unwrap().is_none());

        let c = Session::new(&user.id, "secret-c", None);
        store.create_session(&c).await.unwrap();
        assert_eq!(store.delete_sessions(&user.id, Some(&a.id)).await.unwrap(), 1, "every one but `a`");
        assert_eq!(store.list_sessions(&user.id).await.unwrap(), vec![a.clone()]);
        assert!(store.delete_session(&a.id).await.unwrap());
        assert!(!store.delete_session(&a.id).await.unwrap());
        store.create_session(&c).await.unwrap();
        assert_eq!(store.delete_sessions(&user.id, None).await.unwrap(), 1);
    }

    #[tokio::test]
    async fn api_tokens_round_trip() {
        let store = Store::open_in_memory().await.unwrap();
        let token = crate::auth::new_api_token();
        let t = ApiToken::new("CI", &token, None);
        store.create_api_token(&t).await.unwrap();
        assert_eq!(store.find_api_token(&crate::auth::digest(&token)).await.unwrap().unwrap(), t);
        assert!(store.find_api_token(&token).await.unwrap().is_none(), "found by digest only");
        assert_eq!(t.hint, token[39..]);

        let used_at = now();
        store.touch_api_token(&t.id, used_at).await.unwrap();
        let listed = store.list_api_tokens().await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].last_used_at, Some(used_at));

        let old = ApiToken::new("old", &crate::auth::new_api_token(), Some(now() - chrono::Duration::days(1)));
        assert!(old.is_expired(now()) && !t.is_expired(now()));
        store.delete_api_token(&t.id).await.unwrap();
        assert!(store.delete_api_token(&t.id).await.is_err());
        assert!(store.list_api_tokens().await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn service_limits_round_trip() {
        let store = Store::open_in_memory().await.unwrap();
        let mut svc = Service::new("api", ServiceType::WebService);
        svc.memory_limit_mb = Some(768);
        svc.cpu_limit = Some(1.5);
        store.create_service(&svc).await.unwrap();
        let got = store.require_service(&svc.id).await.unwrap();
        assert_eq!((got.memory_limit_mb, got.cpu_limit), (Some(768), Some(1.5)));
        let mut changed = got.clone();
        changed.memory_limit_mb = None;
        changed.cpu_limit = Some(0.25);
        let saved = store.update_service(&changed).await.unwrap();
        assert_eq!((saved.memory_limit_mb, saved.cpu_limit), (None, Some(0.25)));
    }

    /// Every pooled connection of a freshly migrated database file sees the
    /// final schema (a connection opened between two migrations used to
    /// prepare `SELECT *` with the old column list).
    #[tokio::test]
    async fn fresh_database_connections_see_every_migration() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("ferry.db")).await.unwrap();
        assert!(store.get_service("nope").await.unwrap().is_none());
        let mut svc = Service::new("web", ServiceType::WebService);
        svc.memory_limit_mb = Some(256);
        store.create_service(&svc).await.unwrap();
        for _ in 0..4 {
            assert_eq!(store.get_service(&svc.id).await.unwrap().unwrap().memory_limit_mb, Some(256));
        }
        let reads = (0..16).map(|_| store.require_service(&svc.id));
        for got in futures::future::join_all(reads).await {
            assert_eq!(got.unwrap().memory_limit_mb, Some(256));
        }
    }

    /// A database written before git connections existed (schema v2) gets
    /// their table; its services are served by the connections made later.
    #[tokio::test]
    async fn upgrades_a_database_without_git_connections() {
        use sqlx::Connection;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ferry.db");
        {
            let opts = SqliteConnectOptions::new().filename(&path).create_if_missing(true).foreign_keys(true);
            let mut conn = sqlx::SqliteConnection::connect_with(&opts).await.unwrap();
            for sql in &MIGRATIONS[..2] {
                sqlx::raw_sql(sql).execute(&mut conn).await.unwrap();
            }
            sqlx::raw_sql(
                "PRAGMA user_version = 2;
                 INSERT INTO services (id, name, service_type, repo_url, deploy_hook_key, created_at, updated_at)
                 VALUES ('srv-00000000000000000001', 'old', 'web_service', 'https://github.com/a/b', 'k',
                         '2026-01-01T00:00:00.000000Z', '2026-01-01T00:00:00.000000Z');",
            )
            .execute(&mut conn)
            .await
            .unwrap();
            conn.close().await.unwrap();
        }
        let store = Store::open(&path).await.unwrap();
        let old = store.require_service("old").await.unwrap();
        assert_eq!(old.repo_url.as_deref(), Some("https://github.com/a/b"));
        assert!(store.list_git_connections().await.unwrap().is_empty());
        let conn = GitConnection::new(GitProvider::Github, "https://github.com", "a", "tok");
        store.create_git_connection(&conn).await.unwrap();
        // The old service's repository is served by the new connection.
        let all = store.list_git_connections().await.unwrap();
        assert_eq!(git_connection_for(&all, old.repo_url.as_deref().unwrap()).map(|c| &c.id), Some(&conn.id));
    }

    /// A database written when git connections were pasted tokens that
    /// services named (schema v3): its connections are token connections,
    /// and its services no longer name one — their repository's URL decides.
    #[tokio::test]
    async fn upgrades_a_database_with_token_connections() {
        use sqlx::Connection;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ferry.db");
        {
            let opts = SqliteConnectOptions::new().filename(&path).create_if_missing(true).foreign_keys(true);
            let mut conn = sqlx::SqliteConnection::connect_with(&opts).await.unwrap();
            for sql in &MIGRATIONS[..3] {
                sqlx::raw_sql(sql).execute(&mut conn).await.unwrap();
            }
            sqlx::raw_sql(
                "PRAGMA user_version = 3;
                 INSERT INTO git_connections (id, provider, base_url, account, account_name, token, scopes,
                                              created_at, updated_at)
                 VALUES ('git-00000000000000000001', 'github', 'https://github.com', 'Octocat', 'The Octocat',
                         'tok', '[\"repo\"]', '2026-01-01T00:00:00.000000Z', '2026-01-01T00:00:00.000000Z');
                 INSERT INTO services (id, name, service_type, repo_url, git_connection_id, deploy_hook_key,
                                       created_at, updated_at)
                 VALUES ('srv-00000000000000000001', 'old', 'web_service', 'https://github.com/octocat/app',
                         'git-00000000000000000001', 'k', '2026-01-01T00:00:00.000000Z',
                         '2026-01-01T00:00:00.000000Z');",
            )
            .execute(&mut conn)
            .await
            .unwrap();
            conn.close().await.unwrap();
        }
        let store = Store::open(&path).await.unwrap();
        let all = store.list_git_connections().await.unwrap();
        let [c] = all.as_slice() else { panic!("{all:?}") };
        assert_eq!((c.auth, c.account.as_str(), c.token.as_str()), (GitAuth::Token, "Octocat", "tok"));
        assert_eq!((c.account_name.as_deref(), c.scopes.as_slice()), (Some("The Octocat"), &["repo".to_string()][..]));
        assert!(c.is_connected());
        assert!(c.refresh_token.is_none() && c.client_id.is_none() && c.app.is_none() && c.installation.is_none());
        // The service kept its repository, which the connection still serves.
        let old = store.require_service("old").await.unwrap();
        assert_eq!(old.repo_url.as_deref(), Some("https://github.com/octocat/app"));
        assert_eq!(git_connection_for(&all, "https://github.com/octocat/app").map(|c| &c.id), Some(&c.id));
        let columns: Vec<String> =
            sqlx::query_scalar("SELECT name FROM pragma_table_info('services')").fetch_all(store.pool()).await.unwrap();
        assert!(!columns.iter().any(|name| name == "git_connection_id"), "{columns:?}");
        // The connection can be authorized another way, and removed.
        let mut again = c.clone();
        again.auth = GitAuth::Oauth;
        again.refresh_token = Some("renew".into());
        again.client_id = Some("app".into());
        let saved = store.update_git_connection(&again).await.unwrap();
        assert_eq!((saved.auth, saved.refresh_token.as_deref()), (GitAuth::Oauth, Some("renew")));
        store.delete_git_connection(&saved.id).await.unwrap();
        assert_eq!(store.require_service("old").await.unwrap().repo_url, old.repo_url);
        // Reopening applies nothing twice.
        store.pool().close().await;
        let store = Store::open(&path).await.unwrap();
        assert!(store.list_git_connections().await.unwrap().is_empty());
        assert_eq!(store.list_services().await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn git_connections() {
        let store = Store::open_in_memory().await.unwrap();
        let mut gh = GitConnection::new(GitProvider::Github, "https://github.com", "Octocat", "ghp_first");
        gh.scopes = vec!["repo".into(), "read:org".into()];
        gh.token_expires_at = Some(now() + chrono::Duration::days(30));
        store.create_git_connection(&gh).await.unwrap();
        let gl = GitConnection::new(GitProvider::Gitlab, "https://gitlab.com", "octocat", "glpat-1");
        store.create_git_connection(&gl).await.unwrap();
        assert_eq!(store.require_git_connection(&gh.id).await.unwrap(), gh);
        // One connection per account of a provider instance, whatever the case of its login.
        let again = GitConnection::new(GitProvider::Github, "https://github.com", "octocat", "ghp_other");
        let err = store.create_git_connection(&again).await.unwrap_err();
        assert!(
            matches!(&err, Error::Conflict(m) if m == "the GitHub account 'octocat' is already connected"),
            "{err}"
        );
        let found = store.find_git_connection(GitProvider::Github, "https://github.com", "OCTOCAT").await.unwrap();
        assert_eq!(found.unwrap().id, gh.id);
        assert!(
            store
                .find_git_connection(GitProvider::Github, "https://ghe.example.com", "octocat")
                .await
                .unwrap()
                .is_none()
        );
        let listed: Vec<String> = store.list_git_connections().await.unwrap().into_iter().map(|c| c.id).collect();
        assert_eq!(listed, vec![gh.id.clone(), gl.id.clone()]);

        // A new token replaces the old one (and what the provider said about it).
        let mut renewed = gh.clone();
        renewed.token = "ghp_second".into();
        renewed.account_name = Some("The Octocat".into());
        renewed.scopes = vec![];
        renewed.token_expires_at = None;
        let saved = store.update_git_connection(&renewed).await.unwrap();
        assert_eq!((saved.token.as_str(), saved.account_name.as_deref()), ("ghp_second", Some("The Octocat")));
        assert!(saved.scopes.is_empty() && saved.token_expires_at.is_none() && saved.updated_at >= gh.updated_at);
        assert!(matches!(
            store.update_git_connection(&GitConnection::new(GitProvider::Github, "x", "y", "z")).await,
            Err(Error::NotFound(_))
        ));

        // Deleting a connection leaves services alone: they name no connection.
        let mut svc = Service::new("app", ServiceType::WebService);
        svc.repo_url = Some("https://gitlab.com/octocat/app.git".into());
        store.create_service(&svc).await.unwrap();
        store.delete_git_connection(&gl.id).await.unwrap();
        assert_eq!(store.require_service("app").await.unwrap(), svc);
        assert!(matches!(store.delete_git_connection(&gl.id).await, Err(Error::NotFound(_))));
        assert_eq!(store.list_git_connections().await.unwrap().len(), 1);
    }

    #[tokio::test]
    async fn git_connections_authorized_in_the_browser() {
        let store = Store::open_in_memory().await.unwrap();
        // An OAuth application waits for its authorization under the empty account.
        let pending = GitConnection::oauth_application(GitProvider::Gitlab, "https://gitlab.com", "app-id", "app-s");
        store.create_git_connection(&pending).await.unwrap();
        let found = store.find_git_connection(GitProvider::Gitlab, "https://gitlab.com", "").await.unwrap().unwrap();
        assert_eq!(found, pending);
        assert!(!found.is_connected());
        // Authorized: the account, its tokens and when the access token expires.
        let mut authorized = found;
        authorized.account = "tanuki".into();
        authorized.token = "access-1".into();
        authorized.refresh_token = Some("refresh-1".into());
        authorized.token_expires_at = Some(now() + chrono::Duration::hours(2));
        authorized.scopes = vec!["read_api".into(), "read_repository".into()];
        let saved = store.update_git_connection(&authorized).await.unwrap();
        assert!(saved.is_connected());
        assert_eq!((saved.auth, saved.client_id.as_deref()), (GitAuth::Oauth, Some("app-id")));
        assert_eq!(saved.refresh_token.as_deref(), Some("refresh-1"));
        // Renewing the tokens doesn't count as a change of the connection.
        let expires = now() + chrono::Duration::hours(4);
        store.set_git_tokens(&saved.id, "access-2", Some("refresh-2"), Some(expires)).await.unwrap();
        let renewed = store.require_git_connection(&saved.id).await.unwrap();
        assert_eq!((renewed.token.as_str(), renewed.refresh_token.as_deref()), ("access-2", Some("refresh-2")));
        assert_eq!((renewed.token_expires_at, renewed.updated_at), (Some(expires), saved.updated_at));
        assert!(matches!(store.set_git_tokens("git-none", "t", None, None).await, Err(Error::NotFound(_))));

        // A GitHub App: its registration, then where it is installed.
        let app = GithubApp {
            id: 4242,
            slug: "ferry-test".into(),
            url: "https://github.com/apps/ferry-test".into(),
            private_key: "pem".into(),
            webhook_secret: Some("hook".into()),
        };
        let mut gh = GitConnection::github_app("https://github.com", "octocat", app.clone(), "Iv1.abc", "s");
        store.create_git_connection(&gh).await.unwrap();
        let stored = store.require_git_connection(&gh.id).await.unwrap();
        assert_eq!(stored, gh);
        assert_eq!(
            (stored.app.as_ref(), stored.installation.as_ref(), stored.is_connected()),
            (Some(&app), None, false)
        );
        gh.installation = Some(GithubInstallation {
            id: 99,
            url: Some("https://github.com/settings/installations/99".into()),
            repository_selection: Some("selected".into()),
        });
        let installed = store.update_git_connection(&gh).await.unwrap();
        assert!(installed.is_connected());
        assert_eq!(installed.installation, gh.installation);
        assert_eq!(installed.token, "");

        // An account can't end up with two connections.
        let mut clash = GitConnection::new(GitProvider::Github, "https://github.com", "hubot", "t");
        store.create_git_connection(&clash).await.unwrap();
        clash.account = "OctoCat".into();
        let err = store.update_git_connection(&clash).await.unwrap_err();
        assert!(matches!(&err, Error::Conflict(m) if m.contains("already connected")), "{err}");
    }

    #[tokio::test]
    async fn env_and_groups() {
        let store = Store::open_in_memory().await.unwrap();
        let svc = Service::new("api", ServiceType::WebService);
        store.create_service(&svc).await.unwrap();
        let g1 = EnvGroup::new("shared");
        let g2 = EnvGroup::new("more");
        store.create_env_group(&g1).await.unwrap();
        store.create_env_group(&g2).await.unwrap();
        assert!(matches!(store.create_env_group(&EnvGroup::new("shared")).await, Err(Error::Conflict(_))));
        store.replace_env(&g1.id, &[EnvVar::new("A", "g1"), EnvVar::new("B", "g1")]).await.unwrap();
        store.replace_env(&g2.id, &[EnvVar::new("B", "g2")]).await.unwrap();
        store.link_env_group(&svc.id, &g1.id).await.unwrap();
        store.link_env_group(&svc.id, &g2.id).await.unwrap();
        store.link_env_group(&svc.id, &g1.id).await.unwrap(); // idempotent
        store.set_env(&svc.id, "C", "svc").await.unwrap();
        store.patch_env(&svc.id, &[EnvVar::new("A", "svc")], &["NOPE".into()]).await.unwrap();
        let env = store.effective_env(&svc.id).await.unwrap();
        assert_eq!(env, vec![EnvVar::new("A", "svc"), EnvVar::new("B", "g2"), EnvVar::new("C", "svc")]);
        let groups = store.service_env_groups(&svc.id).await.unwrap();
        assert_eq!(groups.iter().map(|g| g.name.as_str()).collect::<Vec<_>>(), vec!["shared", "more"]);
        assert_eq!(store.env_group_services(&g1.id).await.unwrap().len(), 1);
        assert!(store.unlink_env_group(&svc.id, &g2.id).await.unwrap());
        store.delete_env_group(&g1.id).await.unwrap();
        assert_eq!(store.effective_env(&svc.id).await.unwrap(), vec![EnvVar::new("A", "svc"), EnvVar::new("C", "svc")]);
        assert!(store.unset_env(&svc.id, "C").await.unwrap());
        assert!(!store.unset_env(&svc.id, "C").await.unwrap());
    }

    #[tokio::test]
    async fn datastores_and_settings() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("sub/ferry.db")).await.unwrap();
        let mut ds = Datastore::new("db", DatastoreKind::Postgres);
        store.create_datastore(&ds).await.unwrap();
        ds.status = DatastoreStatus::Available;
        ds.host_port = Some(54321);
        let ds2 = store.update_datastore(&ds).await.unwrap();
        assert_eq!(ds2.host_port, Some(54321));
        // Limits: set only by set_datastore_limits; update_datastore keeps them.
        assert_eq!((ds2.memory_limit_mb, ds2.cpu_limit), (None, None));
        let limited = store.set_datastore_limits(&ds.id, Some(1024), Some(0.5)).await.unwrap();
        assert_eq!((limited.memory_limit_mb, limited.cpu_limit), (Some(1024), Some(0.5)));
        let stale = store.update_datastore(&ds).await.unwrap();
        assert_eq!((stale.memory_limit_mb, stale.cpu_limit), (Some(1024), Some(0.5)));
        assert!(matches!(store.set_datastore_limits("dbs-nope", None, None).await, Err(Error::NotFound(_))));
        assert_eq!(store.require_datastore("db").await.unwrap().status, DatastoreStatus::Available);
        assert!(store.name_taken("db").await.unwrap());
        store.delete_datastore(&ds.id).await.unwrap();
        assert!(store.list_datastores().await.unwrap().is_empty());
        store.set_setting("k", "v1").await.unwrap();
        store.set_setting("k", "v2").await.unwrap();
        assert_eq!(store.get_setting("k").await.unwrap().as_deref(), Some("v2"));
        drop(store);
        // Reopen: migrations must be idempotent.
        let store = Store::open(&dir.path().join("sub/ferry.db")).await.unwrap();
        assert_eq!(store.get_setting("k").await.unwrap().as_deref(), Some("v2"));
    }

    #[tokio::test]
    async fn domains_roundtrip() {
        let store = Store::open_in_memory().await.unwrap();
        // The base domain: one row, the default one until another is.
        let base = store.sync_base_domain("LocalHost.").await.unwrap();
        assert_eq!(
            (base.name.as_str(), base.source, base.status),
            ("localhost", DomainSource::Config, DomainStatus::Active)
        );
        assert!(base.is_default && base.is_served());
        assert_eq!(store.sync_base_domain("localhost").await.unwrap(), base);

        let mut d = Domain::new("example.com", DomainSource::Connected);
        assert_eq!(d.status, DomainStatus::Pending);
        store.create_domain(&d).await.unwrap();
        assert_eq!(store.require_domain("Example.com.").await.unwrap(), d);
        assert_eq!(store.require_domain(&d.id).await.unwrap(), d);
        let dup = Domain::new("example.com", DomainSource::Connected);
        assert!(matches!(store.create_domain(&dup).await, Err(Error::Conflict(_))));
        assert!(matches!(store.require_domain("nope.example.com").await, Err(Error::NotFound(_))));

        // What a verification found.
        d.status = DomainStatus::Active;
        d.checks = vec![DomainCheck::new(DomainCheckKind::Dns, CheckOutcome::Passed, "resolves to 203.0.113.10")];
        d.failures = 2;
        d.verified_at = Some(now());
        d.checked_at = d.verified_at;
        assert_eq!(store.record_domain_check(&d).await.unwrap(), d);

        // One default at a time; the default one is listed first.
        store.set_default_domain(&d.id).await.unwrap();
        let names: Vec<(String, bool)> =
            store.list_domains().await.unwrap().into_iter().map(|d| (d.name, d.is_default)).collect();
        assert_eq!(names, vec![("example.com".to_string(), true), ("localhost".to_string(), false)]);
        assert!(matches!(store.set_default_domain("dom-nope").await, Err(Error::NotFound(_))));

        // Removing the default domain gives the default back to the base domain.
        let other = Domain::new("example.org", DomainSource::Connected);
        store.create_domain(&other).await.unwrap();
        store.set_default_domain(&other.id).await.unwrap();
        store.delete_domain(&other.id).await.unwrap();
        assert!(store.require_domain("localhost").await.unwrap().is_default);
        store.set_default_domain(&d.id).await.unwrap();

        // Another `--base-domain`: the old row goes, a connected domain of
        // that name becomes the base domain and keeps what it had.
        let base = store.sync_base_domain("example.com").await.unwrap();
        assert_eq!(
            (base.id.as_str(), base.source, base.status),
            (d.id.as_str(), DomainSource::Config, DomainStatus::Active)
        );
        assert!(base.is_default);
        assert_eq!(store.list_domains().await.unwrap().len(), 1);
        // And when the default domain was the old base domain, the new one is.
        let base = store.sync_base_domain("apps.example.org").await.unwrap();
        assert!(base.is_default && base.is_served());
        assert_eq!(base.status, DomainStatus::Pending);
        assert_eq!(store.list_domains().await.unwrap(), vec![base.clone()]);

        store.delete_domain(&base.id).await.unwrap();
        assert!(matches!(store.delete_domain(&base.id).await, Err(Error::NotFound(_))));
        assert!(matches!(store.record_domain_check(&base).await, Err(Error::NotFound(_))));
    }

    fn host_point(at: DateTime<Utc>, cpu: Option<f64>, memory: Option<u64>) -> HostPoint {
        HostPoint {
            at,
            cpu_percent: cpu,
            memory_used_bytes: memory,
            memory_total_bytes: Some(8 << 30),
            disk_read_bytes_per_sec: Some(100),
            disk_write_bytes_per_sec: None,
            network_rx_bytes_per_sec: Some(1000),
            network_tx_bytes_per_sec: Some(10),
        }
    }

    fn container(
        kind: ContainerKind,
        id: &str,
        name: &str,
        instance: Option<&str>,
        cpu: Option<f64>,
    ) -> ContainerMetrics {
        ContainerMetrics {
            kind,
            id: id.into(),
            name: name.into(),
            instance: instance.map(Into::into),
            cpu_percent: cpu,
            memory_bytes: cpu.map(|c| c as u64 * 1000),
            memory_limit_bytes: Some(512 << 20),
            cpu_limit: None,
        }
    }

    /// 2026-10-17T00:00:00Z, a multiple of every step.
    fn midnight() -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-10-17T00:00:00Z").unwrap().with_timezone(&Utc)
    }

    #[tokio::test]
    async fn a_new_database_has_the_metrics_tables() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(&dir.path().join("ferry.db")).await.unwrap();
        let version: i64 = sqlx::query_scalar("PRAGMA user_version").fetch_one(store.pool()).await.unwrap();
        assert_eq!(version, MIGRATIONS.len() as i64);
        let t = midnight();
        store.insert_metrics(&host_point(t, Some(5.0), Some(1 << 30)), &[]).await.unwrap();
        let (host, containers) = store.metrics_history(t, t, 10).await.unwrap();
        assert_eq!(host, vec![host_point(t, Some(5.0), Some(1 << 30))]);
        assert!(containers.is_empty());
    }

    /// A database written before the metrics history (schema v6) gets its
    /// tables, empty, and keeps everything else.
    #[tokio::test]
    async fn upgrades_a_database_without_the_metrics_history() {
        use sqlx::Connection;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ferry.db");
        {
            let opts = SqliteConnectOptions::new().filename(&path).create_if_missing(true).foreign_keys(true);
            let mut conn = sqlx::SqliteConnection::connect_with(&opts).await.unwrap();
            for sql in &MIGRATIONS[..6] {
                sqlx::raw_sql(sql).execute(&mut conn).await.unwrap();
            }
            sqlx::raw_sql(
                "PRAGMA user_version = 6;
                 INSERT INTO services (id, name, service_type, image, deploy_hook_key, created_at, updated_at)
                 VALUES ('srv-00000000000000000001', 'old', 'web_service', 'nginx', 'k',
                         '2026-01-01T00:00:00.000000Z', '2026-01-01T00:00:00.000000Z');",
            )
            .execute(&mut conn)
            .await
            .unwrap();
            conn.close().await.unwrap();
        }
        let store = Store::open(&path).await.unwrap();
        assert_eq!(store.require_service("old").await.unwrap().image.as_deref(), Some("nginx"));
        let t = midnight();
        let day = chrono::Duration::days(1);
        assert_eq!(store.metrics_history(t - day, t, 240).await.unwrap(), (vec![], vec![]));
        let web = container(ContainerKind::Service, "srv-00000000000000000001", "old", Some("aaaaaa"), Some(1.0));
        store.insert_metrics(&host_point(t, Some(5.0), None), &[web]).await.unwrap();
        // Reopening applies nothing twice and keeps the readings.
        store.pool().close().await;
        let store = Store::open(&path).await.unwrap();
        let (host, containers) = store.metrics_history(t - day, t, 240).await.unwrap();
        assert_eq!((host.len(), containers.len()), (1, 1));
    }

    #[tokio::test]
    async fn metrics_history_averages_the_readings_of_each_step() {
        let store = Store::open_in_memory().await.unwrap();
        let t = midnight();
        let secs = |s: i64| t + chrono::Duration::seconds(s);
        let web = |instance, cpu| container(ContainerKind::Service, "srv-1", "web", Some(instance), cpu);
        let db = |cpu| container(ContainerKind::Datastore, "ds-1", "db", None, cpu);
        // Step [0, 60): three readings, one without CPU or memory.
        store
            .insert_metrics(&host_point(secs(0), Some(10.0), Some(100)), &[web("aaaaaa", Some(50.0)), db(Some(1.0))])
            .await
            .unwrap();
        store.insert_metrics(&host_point(secs(10), None, None), &[web("aaaaaa", Some(150.0)), db(None)]).await.unwrap();
        store.insert_metrics(&host_point(secs(20), Some(30.0), Some(201)), &[web("bbbbbb", Some(7.0))]).await.unwrap();
        // Step [60, 120): nothing. Step [120, 180): one reading, in which
        // the service was renamed; a second reading in the same second
        // replaces the first.
        store.insert_metrics(&host_point(secs(130), Some(1.0), Some(1)), &[]).await.unwrap();
        let renamed = container(ContainerKind::Service, "srv-1", "site", Some("aaaaaa"), Some(2.0));
        store.insert_metrics(&host_point(secs(130), Some(90.0), Some(9)), &[renamed]).await.unwrap();
        // Outside the range.
        store.insert_metrics(&host_point(secs(-1), Some(99.0), None), &[web("aaaaaa", Some(99.0))]).await.unwrap();
        store.insert_metrics(&host_point(secs(181), Some(99.0), None), &[db(Some(99.0))]).await.unwrap();

        let (host, mut containers) = store.metrics_history(secs(0), secs(180), 60).await.unwrap();
        let summary: Vec<_> = host.iter().map(|p| (p.at, p.cpu_percent, p.memory_used_bytes)).collect();
        assert_eq!(
            summary,
            vec![(secs(0), Some(20.0), Some(151)), (secs(120), Some(90.0), Some(9))],
            "averages of the values that were read; no point for an empty step"
        );
        assert_eq!(host[0].disk_read_bytes_per_sec, Some(100));
        assert_eq!(host[0].disk_write_bytes_per_sec, None, "never read in the step");
        assert_eq!(host[0].memory_total_bytes, Some(8 << 30));

        containers.sort_by(|a, b| (&a.id, &a.instance).cmp(&(&b.id, &b.instance)));
        let summary: Vec<_> = containers
            .iter()
            .map(|c| {
                let points: Vec<_> = c.points.iter().map(|p| (p.at, p.cpu_percent, p.memory_bytes)).collect();
                (c.kind, c.id.as_str(), c.name.as_str(), c.instance.as_deref(), points)
            })
            .collect();
        assert_eq!(
            summary,
            vec![
                // Without CPU or memory (Docker did not answer): not kept.
                (ContainerKind::Datastore, "ds-1", "db", None, vec![(secs(0), Some(1.0), Some(1000))]),
                (
                    ContainerKind::Service,
                    "srv-1",
                    "site",
                    Some("aaaaaa"),
                    vec![(secs(0), Some(100.0), Some(100_000)), (secs(120), Some(2.0), Some(2000))],
                ),
                (ContainerKind::Service, "srv-1", "web", Some("bbbbbb"), vec![(secs(0), Some(7.0), Some(7000))]),
            ]
        );

        // Steps of 10 s: one point per reading.
        let (host, _) = store.metrics_history(secs(0), secs(180), 10).await.unwrap();
        let at: Vec<_> = host.iter().map(|p| p.at).collect();
        assert_eq!(at, vec![secs(0), secs(10), secs(20), secs(130)]);
        assert_eq!(host[1].cpu_percent, None);
    }

    #[tokio::test]
    async fn pruning_deletes_only_the_older_readings() {
        let store = Store::open_in_memory().await.unwrap();
        let t = midnight();
        let day = chrono::Duration::days(1);
        let web = |cpu| [container(ContainerKind::Service, "srv-1", "web", Some("aaaaaa"), Some(cpu))];
        for at in [t - day - chrono::Duration::seconds(10), t - day, t] {
            store.insert_metrics(&host_point(at, Some(1.0), None), &web(1.0)).await.unwrap();
        }
        // The host's row and the container's of the oldest reading.
        assert_eq!(store.prune_metrics(t - day).await.unwrap(), 2);
        let (host, containers) = store.metrics_history(t - day * 2, t, 10).await.unwrap();
        let at: Vec<_> = host.iter().map(|p| p.at).collect();
        assert_eq!(at, vec![t - day, t], "exactly 24 hours old is kept");
        assert_eq!(containers[0].points.len(), 2);
        assert_eq!(store.prune_metrics(t - day).await.unwrap(), 0);
    }

    fn requests(target: &str, name: &str, ok: u64, errors: u64, ms: u64) -> TargetRequests {
        let mut counts = RequestCounts::default();
        for _ in 0..ok {
            counts.record(200, Some(Duration::from_millis(ms)));
        }
        for _ in 0..errors {
            counts.record(502, Some(Duration::from_millis(ms)));
        }
        TargetRequests { target: target.into(), name: name.into(), counts }
    }

    /// A database written before the requests (schema v7) gets their
    /// table, empty, and keeps the other metrics.
    #[tokio::test]
    async fn upgrades_a_database_without_the_requests() {
        use sqlx::Connection;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ferry.db");
        let t = midnight();
        {
            let opts = SqliteConnectOptions::new().filename(&path).create_if_missing(true).foreign_keys(true);
            let mut conn = sqlx::SqliteConnection::connect_with(&opts).await.unwrap();
            for sql in &MIGRATIONS[..7] {
                sqlx::raw_sql(sql).execute(&mut conn).await.unwrap();
            }
            sqlx::raw_sql(&format!(
                "PRAGMA user_version = 7; INSERT INTO host_samples (at, cpu_percent) VALUES ({}, 5.0);",
                t.timestamp()
            ))
            .execute(&mut conn)
            .await
            .unwrap();
            conn.close().await.unwrap();
        }
        let store = Store::open(&path).await.unwrap();
        let version: i64 = sqlx::query_scalar("PRAGMA user_version").fetch_one(store.pool()).await.unwrap();
        assert_eq!(version, MIGRATIONS.len() as i64);
        assert_eq!(store.metrics_history(t, t, 10).await.unwrap().0.len(), 1, "the host's readings are kept");
        assert_eq!(store.request_history(t, t, 10).await.unwrap(), vec![]);
        store.insert_requests(t, &[requests("srv-1", "web", 3, 0, 5)]).await.unwrap();
        assert_eq!(store.request_history(t, t, 10).await.unwrap().len(), 1);
    }

    /// `(step, target, name, 2xx, 5xx, timed)` of each entry.
    fn request_summary(entries: &[(DateTime<Utc>, TargetRequests)]) -> Vec<(i64, &str, &str, u64, u64, u64)> {
        entries
            .iter()
            .map(|(at, r)| {
                let c = &r.counts;
                (at.timestamp(), r.target.as_str(), r.name.as_str(), c.status_2xx, c.status_5xx, c.latency.count())
            })
            .collect()
    }

    #[tokio::test]
    async fn request_history_adds_up_each_target_by_step() {
        let store = Store::open_in_memory().await.unwrap();
        let t = midnight();
        let secs = |s: i64| t + chrono::Duration::seconds(s);
        let (srv1, srv2) = ("srv-1", "srv-2");
        // Step [0, 60): two readings, a third in the same second as the
        // second (it adds to it).
        store
            .insert_requests(
                secs(0),
                &[
                    requests(srv1, "web", 10, 0, 5),
                    requests(srv2, "api", 1, 1, 400),
                    requests("dashboard", "dashboard", 7, 0, 1),
                ],
            )
            .await
            .unwrap();
        store.insert_requests(secs(10), &[requests(srv1, "web", 0, 2, 5)]).await.unwrap();
        store.insert_requests(secs(10), &[requests(srv1, "web", 1, 0, 5)]).await.unwrap();
        // Nothing is stored for a target without a request.
        store.insert_requests(secs(20), &[requests("unrouted", "unrouted", 0, 0, 1)]).await.unwrap();
        // Step [120, 180): the service was renamed in the step.
        store.insert_requests(secs(130), &[requests(srv1, "site", 4, 0, 5)]).await.unwrap();
        store.insert_requests(secs(120), &[requests(srv1, "web", 1, 0, 5)]).await.unwrap();
        // Outside the range.
        store.insert_requests(secs(-1), &[requests(srv1, "web", 99, 99, 5)]).await.unwrap();
        store.insert_requests(secs(181), &[requests(srv1, "web", 99, 99, 5)]).await.unwrap();

        let entries = store.request_history(secs(0), secs(180), 60).await.unwrap();
        let t0 = t.timestamp();
        assert_eq!(
            request_summary(&entries),
            vec![
                (t0, "dashboard", "dashboard", 7, 0, 7),
                (t0, srv1, "web", 11, 2, 13),
                (t0, srv2, "api", 1, 1, 2),
                (t0 + 120, srv1, "site", 5, 0, 5),
            ],
        );
        let want = requests(srv1, "web", 11, 2, 5).counts.latency;
        assert_eq!(entries[1].1.counts.latency, want, "the histograms are added bucket by bucket");
        assert_eq!(entries[2].1.counts.latency, requests(srv2, "api", 1, 1, 400).counts.latency);

        // Steps of 10 s: one entry per reading.
        let entries = store.request_history(secs(0), secs(180), 10).await.unwrap();
        let steps: Vec<_> = entries.iter().map(|(at, r)| (at.timestamp() - t0, r.target.as_str())).collect();
        assert_eq!(steps, [(0, "dashboard"), (0, srv1), (0, srv2), (10, srv1), (120, srv1), (130, srv1)]);
    }

    /// A database written before environments (schema v8) puts every
    /// service and datastore in `production`, which it creates, and keeps
    /// everything else.
    #[tokio::test]
    async fn upgrades_a_database_without_environments() {
        use sqlx::Connection;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ferry.db");
        {
            let opts = SqliteConnectOptions::new().filename(&path).create_if_missing(true).foreign_keys(true);
            let mut conn = sqlx::SqliteConnection::connect_with(&opts).await.unwrap();
            for sql in &MIGRATIONS[..8] {
                sqlx::raw_sql(sql).execute(&mut conn).await.unwrap();
            }
            sqlx::raw_sql(
                "PRAGMA user_version = 8;
                 INSERT INTO services (id, name, service_type, image, deploy_hook_key, live_deploy_id, created_at, updated_at)
                 VALUES ('srv-00000000000000000001', 'web', 'web_service', 'nginx', 'k', 'dep-00000000000000000001',
                         '2026-01-01T00:00:00.000000Z', '2026-01-01T00:00:00.000000Z'),
                        ('srv-00000000000000000002', 'api', 'private_service', 'nginx', 'k', NULL,
                         '2026-01-01T00:00:00.000000Z', '2026-01-01T00:00:00.000000Z');
                 INSERT INTO datastores (id, name, kind, version, status, username, password, host_port, created_at, updated_at)
                 VALUES ('dbs-00000000000000000001', 'db', 'postgres', '16', 'available', 'db', 'pw', 5555,
                         '2026-01-01T00:00:00.000000Z', '2026-01-01T00:00:00.000000Z');",
            )
            .execute(&mut conn)
            .await
            .unwrap();
            conn.close().await.unwrap();
        }
        let store = Store::open(&path).await.unwrap();
        let version: i64 = sqlx::query_scalar("PRAGMA user_version").fetch_one(store.pool()).await.unwrap();
        assert_eq!(version, MIGRATIONS.len() as i64);
        let envs = store.list_environments().await.unwrap();
        assert_eq!(envs.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(), ["production"]);
        assert!(crate::ids::has_prefix(&envs[0].id, crate::ids::ENVIRONMENT), "{}", envs[0].id);
        let web = store.require_service("web").await.unwrap();
        assert_eq!(
            (web.environment.as_str(), web.live_deploy_id.as_deref()),
            ("production", Some("dep-00000000000000000001"))
        );
        // Its hostnames and URL don't change.
        assert_eq!(Config::default().service_url(&web).unwrap(), "http://web.localhost:8080");
        let db = store.require_datastore("db").await.unwrap();
        assert_eq!((db.environment.as_str(), db.host_port), ("production", Some(5555)));
        let (services, datastores) = store.environment_members("production").await.unwrap();
        assert_eq!(services.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["api", "web"]);
        assert_eq!(datastores.len(), 1);
        // Reopening applies nothing twice.
        store.pool().close().await;
        let store = Store::open(&path).await.unwrap();
        assert_eq!(store.list_environments().await.unwrap(), envs);
    }

    #[tokio::test]
    async fn environments_hold_services_and_datastores() {
        let store = Store::open_in_memory().await.unwrap();
        let production = store.require_environment("production").await.unwrap();
        assert!(production.is_production());
        let staging = Environment::new("staging");
        store.create_environment(&staging).await.unwrap();
        assert!(matches!(store.create_environment(&Environment::new("staging")).await, Err(Error::Conflict(_))));
        assert_eq!(store.require_environment(&staging.id).await.unwrap(), staging);
        let names: Vec<_> = store.list_environments().await.unwrap().into_iter().map(|e| e.name).collect();
        assert_eq!(names, ["production", "staging"], "production first");

        // Created in an environment, which must exist.
        let mut shop = Service::new("shop", ServiceType::WebService);
        shop.environment = "staging".into();
        store.create_service(&shop).await.unwrap();
        let mut shop_db = Datastore::new("shop-db", DatastoreKind::Postgres);
        shop_db.environment = "staging".into();
        store.create_datastore(&shop_db).await.unwrap();
        assert_eq!(store.require_service_in("shop", Some("staging")).await.unwrap().environment, "staging");
        let mut lost = Service::new("lost", ServiceType::WebService);
        lost.environment = "qa".into();
        assert!(matches!(store.create_service(&lost).await, Err(Error::NotFound(_))));
        // A row holds the name, never the id.
        lost.environment = staging.id.clone();
        assert!(matches!(store.create_service(&lost).await, Err(Error::NotFound(_))));
        let mut lost_db = Datastore::new("lost-db", DatastoreKind::Redis);
        lost_db.environment = "qa".into();
        assert!(matches!(store.create_datastore(&lost_db).await, Err(Error::NotFound(_))));
        // A name is unique in its environment.
        let mut twice = Service::new("shop", ServiceType::WebService);
        twice.environment = "staging".into();
        let e = store.create_service(&twice).await.unwrap_err();
        assert!(
            matches!(&e, Error::Conflict(m) if m == "name 'shop' is already in use in environment 'staging'"),
            "{e:?}"
        );
        let mut twice_db = Datastore::new("shop", DatastoreKind::Redis);
        twice_db.environment = "staging".into();
        assert!(matches!(store.create_datastore(&twice_db).await, Err(Error::Conflict(_))));
        // Settings updates never move a service.
        let mut moved = store.require_service_in("shop", Some("staging")).await.unwrap();
        moved.environment = "production".into();
        assert_eq!(store.update_service(&moved).await.unwrap().environment, "staging");

        // References resolve inside the environment.
        store.create_service(&Service::new("api", ServiceType::PrivateService)).await.unwrap();
        let (dss, refs) = store.environment_reference_targets(&Config::default(), "staging").await.unwrap();
        assert_eq!(dss.iter().map(|d| d.name.as_str()).collect::<Vec<_>>(), ["shop-db"]);
        assert_eq!(refs.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(), ["shop"]);
        assert_eq!(refs[0].public_url.as_deref(), Some("http://shop.staging.localhost:8080"));

        // Deleted only once empty; production never.
        let e = store.delete_environment(&staging.id).await.unwrap_err().to_string();
        assert!(e.contains("still holds service 'shop', datastore 'shop-db'"), "{e}");
        let e = store.delete_environment(&production.id).await.unwrap_err();
        assert!(matches!(&e, Error::Conflict(m) if m.contains("can't be deleted")), "{e:?}");
        store.delete_service(&shop.id).await.unwrap();
        store.delete_datastore(&shop_db.id).await.unwrap();
        store.delete_environment(&staging.id).await.unwrap();
        assert!(store.find_environment("staging").await.unwrap().is_none());
        assert!(matches!(store.delete_environment(&staging.id).await, Err(Error::NotFound(_))));
    }

    /// Every environment uses the same names: a name with no environment
    /// is production's, an id is found in any environment.
    #[tokio::test]
    async fn same_names_in_every_environment() {
        let store = Store::open_in_memory().await.unwrap();
        store.create_environment(&Environment::new("staging")).await.unwrap();
        let in_env = |mut s: Service, env: &str| {
            s.environment = env.into();
            s
        };
        let api = Service::new("api", ServiceType::WebService);
        let staging_api = in_env(Service::new("api", ServiceType::WebService), "staging");
        store.create_service(&api).await.unwrap();
        store.create_service(&staging_api).await.unwrap();
        let db = Datastore::new("db", DatastoreKind::Postgres);
        let mut staging_db = Datastore::new("db", DatastoreKind::Postgres);
        staging_db.environment = "staging".into();
        store.create_datastore(&db).await.unwrap();
        store.create_datastore(&staging_db).await.unwrap();
        assert!(store.name_taken_in("staging", "api").await.unwrap());
        assert!(!store.name_taken_in("staging", "web").await.unwrap());
        assert!(store.name_taken("db").await.unwrap());

        // A name: production's, unless an environment is named.
        assert_eq!(store.require_service("api").await.unwrap().id, api.id);
        assert_eq!(store.require_service_in("api", Some("production")).await.unwrap().id, api.id);
        assert_eq!(store.require_service_in("api", Some("staging")).await.unwrap().id, staging_api.id);
        assert_eq!(store.require_datastore("db").await.unwrap().id, db.id);
        assert_eq!(store.require_datastore_in("db", Some("staging")).await.unwrap().id, staging_db.id);
        let e = store.require_service_in("api", Some("qa")).await.unwrap_err().to_string();
        assert_eq!(e, "service 'api' in environment 'qa' not found");
        assert_eq!(store.require_datastore("nope").await.unwrap_err().to_string(), "datastore 'nope' not found");
        // An id: in any environment, unless it names another one.
        assert_eq!(store.require_service(&staging_api.id).await.unwrap().id, staging_api.id);
        assert!(store.find_service_in(&staging_api.id, Some("production")).await.unwrap().is_none());
        assert!(store.find_datastore_in(&db.id, Some("staging")).await.unwrap().is_none());

        // Listed by name, production's first.
        let listed: Vec<_> = store.list_services().await.unwrap().into_iter().map(|s| s.qualified_name()).collect();
        assert_eq!(listed, ["api", "api.staging"]);
        let listed: Vec<_> = store.list_datastores().await.unwrap().into_iter().map(|d| d.qualified_name()).collect();
        assert_eq!(listed, ["db", "db.staging"]);

        // References find their own environment's.
        for (env, url, ds) in [
            ("production", "http://api.localhost:8080", &db.id),
            ("staging", "http://api.staging.localhost:8080", &staging_db.id),
        ] {
            let (dss, refs) = store.environment_reference_targets(&Config::default(), env).await.unwrap();
            assert_eq!(dss.iter().map(|d| &d.id).collect::<Vec<_>>(), [ds], "{env}");
            assert_eq!(refs.len(), 1, "{env}");
            assert_eq!(refs[0].public_url.as_deref(), Some(url), "{env}");
        }
    }

    /// Schema v10 makes names unique per environment by rebuilding the
    /// services and datastores tables: their deploys, job runs, variables
    /// and env group links stay, and foreign keys still hold afterwards.
    #[tokio::test]
    async fn upgrades_names_to_one_per_environment() {
        use sqlx::Connection;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ferry.db");
        {
            let opts = SqliteConnectOptions::new().filename(&path).create_if_missing(true).foreign_keys(true);
            let mut conn = sqlx::SqliteConnection::connect_with(&opts).await.unwrap();
            for sql in &MIGRATIONS[..9] {
                sqlx::raw_sql(sql).execute(&mut conn).await.unwrap();
            }
            sqlx::raw_sql(
                "PRAGMA user_version = 9;
                 INSERT INTO services (id, name, service_type, image, deploy_hook_key, live_deploy_id, memory_limit_mb,
                                       created_at, updated_at)
                 VALUES ('srv-00000000000000000001', 'web', 'web_service', 'nginx', 'k', 'dep-00000000000000000001', 256,
                         '2026-01-01T00:00:00.000000Z', '2026-01-01T00:00:00.000000Z');
                 INSERT INTO deploys (id, service_id, status, trigger_kind, source, created_at)
                 VALUES ('dep-00000000000000000001', 'srv-00000000000000000001', 'live', 'manual',
                         '{\"kind\":\"image\",\"image\":\"nginx\"}', '2026-01-01T00:00:00.000000Z');
                 INSERT INTO job_runs (id, service_id, trigger_kind, status, created_at)
                 VALUES ('job-00000000000000000001', 'srv-00000000000000000001', 'manual', 'succeeded',
                         '2026-01-01T00:00:00.000000Z');
                 INSERT INTO env_groups (id, name, created_at, updated_at)
                 VALUES ('grp-00000000000000000001', 'shared', '2026-01-01T00:00:00.000000Z', '2026-01-01T00:00:00.000000Z');
                 INSERT INTO service_env_groups (service_id, group_id, position)
                 VALUES ('srv-00000000000000000001', 'grp-00000000000000000001', 0);
                 INSERT INTO env_vars (owner_id, key, value) VALUES ('srv-00000000000000000001', 'A', '1');
                 INSERT INTO datastores (id, name, kind, version, status, username, password, host_port, created_at, updated_at)
                 VALUES ('dbs-00000000000000000001', 'db', 'postgres', '16', 'available', 'db', 'pw', 5555,
                         '2026-01-01T00:00:00.000000Z', '2026-01-01T00:00:00.000000Z');",
            )
            .execute(&mut conn)
            .await
            .unwrap();
            conn.close().await.unwrap();
        }
        let store = Store::open(&path).await.unwrap();
        let version: i64 = sqlx::query_scalar("PRAGMA user_version").fetch_one(store.pool()).await.unwrap();
        assert_eq!(version, MIGRATIONS.len() as i64);
        let web = store.require_service("web").await.unwrap();
        assert_eq!((web.environment.as_str(), web.memory_limit_mb), ("production", Some(256)));
        assert_eq!(web.live_deploy_id.as_deref(), Some("dep-00000000000000000001"));
        assert_eq!(store.list_deploys(&web.id, 10).await.unwrap().len(), 1, "the deploys stay");
        assert_eq!(store.list_job_runs(&web.id, 10).await.unwrap().len(), 1, "the job runs stay");
        assert_eq!(store.service_env_groups(&web.id).await.unwrap().len(), 1, "the env group links stay");
        assert_eq!(store.list_env(&web.id).await.unwrap(), [EnvVar::new("A", "1")]);
        assert_eq!(store.require_datastore("db").await.unwrap().host_port, Some(5555));

        // The same names now fit in another environment, not twice in one.
        store.create_environment(&Environment::new("staging")).await.unwrap();
        let mut staging_web = Service::new("web", ServiceType::WebService);
        staging_web.environment = "staging".into();
        store.create_service(&staging_web).await.unwrap();
        let mut staging_db = Datastore::new("db", DatastoreKind::Redis);
        staging_db.environment = "staging".into();
        store.create_datastore(&staging_db).await.unwrap();
        assert!(matches!(
            store.create_service(&Service::new("web", ServiceType::WebService)).await,
            Err(Error::Conflict(_))
        ));

        // Foreign keys are on again, and still point at the rebuilt table.
        let on: i64 = sqlx::query_scalar("PRAGMA foreign_keys").fetch_one(store.pool()).await.unwrap();
        assert_eq!(on, 1);
        let orphan = sqlx::query(
            "INSERT INTO deploys (id, service_id, status, trigger_kind, source, created_at)
             VALUES ('dep-00000000000000000002', 'srv-nope', 'queued', 'manual', '{}', '2026-01-01T00:00:00.000000Z')",
        )
        .execute(store.pool())
        .await;
        assert!(orphan.is_err(), "a deploy of no service is refused");
        sqlx::query("DELETE FROM services WHERE id = ?").bind(&web.id).execute(store.pool()).await.unwrap();
        assert!(store.list_deploys(&web.id, 10).await.unwrap().is_empty(), "deleting a service cascades again");
    }

    #[tokio::test]
    async fn pruning_deletes_the_older_requests_too() {
        let store = Store::open_in_memory().await.unwrap();
        let t = midnight();
        let day = chrono::Duration::days(1);
        for at in [t - day - chrono::Duration::seconds(10), t - day, t] {
            store.insert_requests(at, &[requests("srv-1", "web", 1, 0, 5)]).await.unwrap();
        }
        assert_eq!(store.prune_metrics(t - day).await.unwrap(), 1);
        let entries = store.request_history(t - day * 2, t, 10).await.unwrap();
        assert_eq!(entries.iter().map(|(at, _)| *at).collect::<Vec<_>>(), vec![t - day, t]);
    }
}
