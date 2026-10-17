-- The requests the proxy answered (DESIGN.md §22): every 10 s, ferryd stores
-- what each target got since its previous reading, and deletes the rows older
-- than 24 hours with the other metrics.
--
-- `at` is the Unix time (seconds) of the reading. `target` is a service id,
-- 'dashboard' (the dashboard's host) or 'unrouted' (a host no service has);
-- `name` the service's name at the time. A target without a request since the
-- previous reading has no row.
--
-- `status_*` count the requests by status class (a WebSocket that opened is a
-- 2xx). The latency histogram: `ms_<n>` counts the requests whose response
-- headers went out within n ms (and later than the bound of the column
-- before), `ms_more` the ones that took longer than 30 s. WebSockets are not
-- in it.
CREATE TABLE request_samples (
    at            INTEGER NOT NULL,
    target        TEXT NOT NULL,
    name          TEXT NOT NULL,
    status_2xx    INTEGER NOT NULL DEFAULT 0,
    status_3xx    INTEGER NOT NULL DEFAULT 0,
    status_4xx    INTEGER NOT NULL DEFAULT 0,
    status_5xx    INTEGER NOT NULL DEFAULT 0,
    ms_1          INTEGER NOT NULL DEFAULT 0,
    ms_2          INTEGER NOT NULL DEFAULT 0,
    ms_5          INTEGER NOT NULL DEFAULT 0,
    ms_10         INTEGER NOT NULL DEFAULT 0,
    ms_20         INTEGER NOT NULL DEFAULT 0,
    ms_30         INTEGER NOT NULL DEFAULT 0,
    ms_50         INTEGER NOT NULL DEFAULT 0,
    ms_75         INTEGER NOT NULL DEFAULT 0,
    ms_100        INTEGER NOT NULL DEFAULT 0,
    ms_150        INTEGER NOT NULL DEFAULT 0,
    ms_200        INTEGER NOT NULL DEFAULT 0,
    ms_300        INTEGER NOT NULL DEFAULT 0,
    ms_500        INTEGER NOT NULL DEFAULT 0,
    ms_750        INTEGER NOT NULL DEFAULT 0,
    ms_1000       INTEGER NOT NULL DEFAULT 0,
    ms_1500       INTEGER NOT NULL DEFAULT 0,
    ms_2000       INTEGER NOT NULL DEFAULT 0,
    ms_3000       INTEGER NOT NULL DEFAULT 0,
    ms_5000       INTEGER NOT NULL DEFAULT 0,
    ms_10000      INTEGER NOT NULL DEFAULT 0,
    ms_30000      INTEGER NOT NULL DEFAULT 0,
    ms_more       INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (at, target)
) WITHOUT ROWID;
