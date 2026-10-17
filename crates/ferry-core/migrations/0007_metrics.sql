-- The last 24 hours of the server's metrics (DESIGN.md §22): ferryd reads
-- them every 10 s and deletes the rows older than 24 hours.
--
-- `at` is a Unix time in seconds; a value that could not be read is NULL.
-- Bytes are integers; CPU is 0–100 % of the machine for the host, 100 = one
-- core for a container.
CREATE TABLE host_samples (
    at                       INTEGER PRIMARY KEY,
    cpu_percent              REAL,
    memory_used_bytes        INTEGER,
    memory_total_bytes       INTEGER,
    disk_read_bytes_per_sec  INTEGER,
    disk_write_bytes_per_sec INTEGER,
    network_rx_bytes_per_sec INTEGER,
    network_tx_bytes_per_sec INTEGER
);

-- One row per running service instance or datastore and reading. `owner_id`
-- is the service or datastore id, `instance` the service instance ('' for a
-- datastore). Keyed by time first: ranges and pruning read the key in order.
-- Rows of a deleted service are kept until they are 24 hours old.
CREATE TABLE container_samples (
    at           INTEGER NOT NULL,
    kind         TEXT NOT NULL,
    owner_id     TEXT NOT NULL,
    instance     TEXT NOT NULL DEFAULT '',
    name         TEXT NOT NULL,
    cpu_percent  REAL,
    memory_bytes INTEGER,
    PRIMARY KEY (at, owner_id, instance)
) WITHOUT ROWID;
