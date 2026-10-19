-- Environments (DESIGN.md §3): groups of services and datastores, each on a
-- private network of its own, so staging can't reach production.
--
-- Services and datastores name their environment (`environment`, the name:
-- environments are never renamed). A server set up before environments has
-- every row in `production`, which keeps today's network, Docker names and
-- hostnames: the upgrade restarts nothing. No foreign key: SQLite can't add a
-- REFERENCES column with a non-NULL default, so the store checks that the
-- environment exists, and refuses to delete one that still holds anything.
CREATE TABLE environments (
    id         TEXT PRIMARY KEY,
    name       TEXT NOT NULL UNIQUE,
    created_at TEXT NOT NULL
);

INSERT INTO environments (id, name, created_at)
VALUES ('env-' || lower(hex(randomblob(10))), 'production', strftime('%Y-%m-%dT%H:%M:%f', 'now') || '000Z');

ALTER TABLE services ADD COLUMN environment TEXT NOT NULL DEFAULT 'production';
ALTER TABLE datastores ADD COLUMN environment TEXT NOT NULL DEFAULT 'production';

CREATE INDEX idx_services_environment ON services(environment);
CREATE INDEX idx_datastores_environment ON datastores(environment);
