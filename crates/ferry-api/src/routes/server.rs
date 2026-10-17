//! How the server is doing: the machine, its containers and the requests of
//! the proxy, now and over the last 24 hours, the server's own log, and the
//! same numbers in the Prometheus format (DESIGN.md §22).

use axum::Json;
use axum::extract::State;
use axum::response::{IntoResponse, Response};
use ferry_core::dto::{ApiErrorBody, MetricsHistory, MetricsRange, ServerMetrics};
use ferry_core::server_logs::{ServerLogLevel, ServerLogQuery};
use http::{HeaderValue, header};
use serde::Deserialize;

use crate::AppState;
use crate::error::ApiResult;
use crate::extract::{ApiQuery, de_flag};
use crate::prometheus;
use crate::sse;

/// `GET /api/v1/server/metrics`
#[utoipa::path(
    get,
    path = "/api/v1/server/metrics",
    tag = "server",
    operation_id = "getServerMetrics",
    summary = "Live server metrics",
    description = "How busy the machine and its containers are right now. The host: CPU use (0–100 % of the whole machine) and CPU count, memory used without the page cache and total, the used and total space and the read / write speed of the disk of the data directory (and of Docker's data directory when it is another local disk), and the network traffic without the loopback and Docker's own interfaces. The containers: every running service instance and datastore with its CPU use (100 = one core), memory use and limits, the busiest first. Speeds are bytes per second since the previous call (within a minute) or over one second, so a call takes about a second. The requests: what the proxy answered in its last reading (the last 10 s), one entry per target that had any — a service (by id), `dashboard` (the dashboard's host) or `unrouted` (a host no service has) — the services busiest first, then `dashboard` and `unrouted`: requests per second, the requests by status class (2xx counts the WebSockets that opened; 4xx the requests whose client went away before the answer, as 499; 5xx the proxy's own 502 and 503 pages) and the p50 and p95 latency in milliseconds, until the response headers went out (WebSockets left out; `null` without a request to time). Every call reads again: the last 24 hours are `GET /api/v1/server/metrics/history`. A value the server can't read is `null` (on macOS, most host values); `containers` is `null` when Docker does not answer, and the CPU and memory of a container that does not answer in time are `null`.",
    responses((status = 200, description = "The server's metrics.", body = ServerMetrics)),
)]
pub async fn metrics(State(st): State<AppState>) -> ApiResult<Json<ServerMetrics>> {
    Ok(Json(st.engine.server_metrics().await?))
}

#[derive(Debug, Default, Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub struct HistoryQuery {
    /// How far back: `1h` (the default; a point every 10 s), `6h` (every
    /// minute) or `24h` (every 4 minutes).
    #[serde(default)]
    pub range: MetricsRange,
}

/// `GET /api/v1/server/metrics/history?range=`
#[utoipa::path(
    get,
    path = "/api/v1/server/metrics/history",
    tag = "server",
    operation_id = "getServerMetricsHistory",
    summary = "Server metrics of the last 24 hours",
    description = "The server's metrics over the last hour, 6 hours or 24 hours. ferryd reads the server every 10 s while it runs and keeps 24 hours of readings in its database. They come back averaged into steps (`step_seconds`: 10, 60 or 240), 360 points at most per series, oldest first, each at the start of its step. A step without a reading — ferryd was stopped — has no point, so a gap between two points is a time without data. The host: CPU use (0–100 % of the machine), memory used and total, the read / write speed of its disks together and the network traffic, like `GET /api/v1/server/metrics`. The containers: the 5 service instances and datastores whose CPU use (100 = one core) was the highest in the range, the busiest first, with their memory. A value that could not be read in a whole step is `null`. The requests, which the proxy counts and ferryd stores every 10 s: `requests` has the services' together (neither the dashboard's nor the unrouted ones) for every step ferryd ran in and stored requests for — the requests per second over `seconds` (the step, or less for the last one, still filling), the requests by status class, and the p50 and p95 latency in milliseconds, `null` in a step without a request to time — and `request_targets` each target that had requests over the whole range, the services busiest first, then `dashboard` and `unrouted`. Percentiles come from a histogram with fixed buckets: they are exact to the bucket.",
    params(HistoryQuery),
    responses(
        (status = 200, description = "The server's metrics over the range.", body = MetricsHistory),
        (status = 400, description = "Unknown range (not `1h`, `6h` or `24h`).", body = ApiErrorBody),
    ),
)]
pub async fn history(
    State(st): State<AppState>,
    ApiQuery(q): ApiQuery<HistoryQuery>,
) -> ApiResult<Json<MetricsHistory>> {
    Ok(Json(st.engine.metrics_history(q.range).await?))
}

#[derive(Debug, Default, Deserialize, utoipa::IntoParams)]
#[into_params(parameter_in = Query)]
pub struct ServerLogsQuery {
    /// Start with at most this many recent lines (at the level asked). By
    /// default all the lines the server keeps (2,000); `0` for none.
    pub tail: Option<usize>,
    /// Keep the stream open and send new lines as they are logged.
    #[serde(default, deserialize_with = "de_flag")]
    pub follow: bool,
    /// Only the lines at this level or above: `error`, `warn`, `info`, or
    /// `debug` (the default: every line).
    #[serde(default)]
    pub level: ServerLogLevel,
}

/// `GET /api/v1/server/logs?tail=&follow=&level=` (SSE)
#[utoipa::path(
    get,
    path = "/api/v1/server/logs",
    tag = "server",
    operation_id = "streamServerLogs",
    summary = "Server log (SSE)",
    description = "ferryd's own log: the lines it prints to its standard output (the terminal, `ferryd.log` or the service manager), as values. The server keeps its last 2,000 lines in memory, from its start, the same with `ferryd start` and `ferryd run`, and writes nothing more to disk: older lines are only in its output. It logs `info` lines and above, and `debug` lines when `RUST_LOG` asks for them: those of Ferry itself are kept here, those of the libraries it uses only in its output. A line longer than 8 KiB (its message and fields together) is cut. A follower that falls behind never slows the server down: it misses lines, and an `event: skipped` says how many.",
    params(ServerLogsQuery),
    responses(
        (status = 200, description = crate::openapi::SSE_SERVER_LOGS, content_type = "text/event-stream", body = String, example = "event: log\ndata: {\"ts\":\"2026-10-17T12:00:00Z\",\"level\":\"warn\",\"target\":\"ferry_tls\",\"message\":\"certificate request failed\",\"fields\":{\"host\":\"app.example.com\"}}\n\nevent: end\ndata: \n\n"),
        (status = 400, description = "Unknown level (not `error`, `warn`, `info` or `debug`).", body = ApiErrorBody),
    ),
)]
pub async fn logs(State(st): State<AppState>, ApiQuery(q): ApiQuery<ServerLogsQuery>) -> Response {
    let stream = st.server_logs.stream(ServerLogQuery { tail: q.tail, follow: q.follow, level: q.level });
    sse::server_log_response(stream, !q.follow, &st.shutdown)
}

/// `GET /metrics`
#[utoipa::path(
    get,
    path = "/metrics",
    tag = "server",
    operation_id = "getPrometheusMetrics",
    summary = "Metrics in the Prometheus format",
    description = "The server's numbers in the Prometheus text format (version 0.0.4), for a Prometheus that already runs somewhere: point a scrape job at this path with an API token as its bearer credentials. It needs an API token like the rest of the API, and answers at once from what ferryd already has: the host and the containers of its last reading (it reads them every 10 s; nothing when that reading is more than 30 s old) and the proxy's request counters. It never waits for Docker. `ferry_build_info{version}` is always 1. The host: `ferry_host_cpu_count`, `ferry_host_cpu_seconds_total{mode}` (the modes of /proc/stat), `ferry_host_memory_total_bytes`, `ferry_host_memory_available_bytes`, `ferry_host_disk_total_bytes`, `ferry_host_disk_used_bytes`, `ferry_host_disk_read_bytes_total` and `ferry_host_disk_written_bytes_total` (by `mount_point`), `ferry_host_network_receive_bytes_total` and `ferry_host_network_transmit_bytes_total` (by `device`: each interface but the loopback and Docker's own) — the CPU seconds and the disks' bytes count from the start of the machine, an interface's bytes from its own start. The containers, every running service instance and datastore by `kind`, `id`, `name` and `instance_id` (services only; not `instance`, which Prometheus gives the target): `ferry_container_cpu_percent` (100 = one core) and `ferry_container_memory_bytes`. The requests, by `target` (a service id, `dashboard` or `unrouted`) and `name`: `ferry_http_requests_total{status_class}` (`2xx` to `5xx`) and the histogram `ferry_http_request_duration_seconds` (until the response headers, WebSockets left out), counted from the start of ferryd. A value the server can't read (on macOS, most host values) is left out, never published as 0.",
    responses((
        status = 200,
        description = "The metrics, in the Prometheus text format 0.0.4.",
        body = String,
        content_type = "text/plain; version=0.0.4",
    )),
)]
pub async fn prometheus(State(st): State<AppState>) -> ApiResult<Response> {
    let snapshot = st.engine.metrics_snapshot().await?;
    let body = prometheus::render(&snapshot, ferry_core::VERSION);
    Ok(([(header::CONTENT_TYPE, HeaderValue::from_static(prometheus::CONTENT_TYPE))], body).into_response())
}
