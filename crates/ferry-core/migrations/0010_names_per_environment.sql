-- The same name in every environment (DESIGN.md §23): a service or datastore
-- name is unique inside its environment, not on the whole server, so staging
-- runs `api` next to production's `api`.
--
-- SQLite can't drop the `UNIQUE` of a column: both tables are rebuilt without
-- it (the store runs migrations with foreign keys off, so dropping `services`
-- doesn't cascade to its deploys, job runs and env group links), then get a
-- unique index on (environment, name). Rows, ids and every other column stay
-- as they are.

CREATE TABLE services_new (
    id                TEXT PRIMARY KEY,
    name              TEXT NOT NULL,
    environment       TEXT NOT NULL DEFAULT 'production',
    service_type      TEXT NOT NULL,
    repo_url          TEXT,
    branch            TEXT NOT NULL DEFAULT 'main',
    image             TEXT,
    runtime           TEXT NOT NULL DEFAULT 'auto',
    root_dir          TEXT,
    dockerfile_path   TEXT,
    build_command     TEXT,
    start_command     TEXT,
    publish_dir       TEXT,
    port              INTEGER,
    health_check_path TEXT,
    schedule          TEXT,
    instances         INTEGER NOT NULL DEFAULT 1,
    auto_deploy       INTEGER NOT NULL DEFAULT 1,
    suspended         INTEGER NOT NULL DEFAULT 0,
    disk_mount_path   TEXT,
    custom_domains    TEXT NOT NULL DEFAULT '[]',
    deploy_hook_key   TEXT NOT NULL,
    live_deploy_id    TEXT,
    created_at        TEXT NOT NULL,
    updated_at        TEXT NOT NULL,
    memory_limit_mb   INTEGER,
    cpu_limit         REAL
);

INSERT INTO services_new (id, name, environment, service_type, repo_url, branch, image, runtime, root_dir,
    dockerfile_path, build_command, start_command, publish_dir, port, health_check_path, schedule, instances,
    auto_deploy, suspended, disk_mount_path, custom_domains, deploy_hook_key, live_deploy_id, created_at, updated_at,
    memory_limit_mb, cpu_limit)
SELECT id, name, environment, service_type, repo_url, branch, image, runtime, root_dir,
    dockerfile_path, build_command, start_command, publish_dir, port, health_check_path, schedule, instances,
    auto_deploy, suspended, disk_mount_path, custom_domains, deploy_hook_key, live_deploy_id, created_at, updated_at,
    memory_limit_mb, cpu_limit
FROM services;

DROP TABLE services;
ALTER TABLE services_new RENAME TO services;
CREATE UNIQUE INDEX idx_services_environment_name ON services(environment, name);

CREATE TABLE datastores_new (
    id              TEXT PRIMARY KEY,
    name            TEXT NOT NULL,
    environment     TEXT NOT NULL DEFAULT 'production',
    kind            TEXT NOT NULL,
    version         TEXT NOT NULL,
    status          TEXT NOT NULL,
    username        TEXT NOT NULL,
    password        TEXT NOT NULL,
    database        TEXT,
    host_port       INTEGER,
    error           TEXT,
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL,
    memory_limit_mb INTEGER,
    cpu_limit       REAL
);

INSERT INTO datastores_new (id, name, environment, kind, version, status, username, password, database, host_port,
    error, created_at, updated_at, memory_limit_mb, cpu_limit)
SELECT id, name, environment, kind, version, status, username, password, database, host_port,
    error, created_at, updated_at, memory_limit_mb, cpu_limit
FROM datastores;

DROP TABLE datastores;
ALTER TABLE datastores_new RENAME TO datastores;
CREATE UNIQUE INDEX idx_datastores_environment_name ON datastores(environment, name);
