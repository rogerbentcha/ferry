//! How the server is doing: the machine and its containers, now and over
//! the last 24 hours (DESIGN.md §22).

use axum::Json;
use axum::extract::State;
use ferry_core::dto::{ApiErrorBody, MetricsHistory, MetricsRange, ServerMetrics};
use serde::Deserialize;

use crate::AppState;
use crate::error::ApiResult;
use crate::extract::ApiQuery;

/// `GET /api/v1/server/metrics`
#[utoipa::path(
    get,
    path = "/api/v1/server/metrics",
    tag = "server",
    operation_id = "getServerMetrics",
    summary = "Live server metrics",
    description = "How busy the machine and its containers are right now. The host: CPU use (0–100 % of the whole machine) and CPU count, memory used without the page cache and total, the used and total space and the read / write speed of the disk of the data directory (and of Docker's data directory when it is another local disk), and the network traffic without the loopback and Docker's own interfaces. The containers: every running service instance and datastore with its CPU use (100 = one core), memory use and limits, the busiest first. Speeds are bytes per second since the previous call (within a minute) or over one second, so a call takes about a second. Every call reads again: the last 24 hours are `GET /api/v1/server/metrics/history`. A value the server can't read is `null` (on macOS, most host values); `containers` is `null` when Docker does not answer, and the CPU and memory of a container that does not answer in time are `null`.",
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
    description = "The server's metrics over the last hour, 6 hours or 24 hours. ferryd reads the server every 10 s while it runs and keeps 24 hours of readings in its database. They come back averaged into steps (`step_seconds`: 10, 60 or 240), 360 points at most per series, oldest first, each at the start of its step. A step without a reading — ferryd was stopped — has no point, so a gap between two points is a time without data. The host: CPU use (0–100 % of the machine), memory used and total, the read / write speed of its disks together and the network traffic, like `GET /api/v1/server/metrics`. The containers: the 5 service instances and datastores whose CPU use (100 = one core) was the highest in the range, the busiest first, with their memory. A value that could not be read in a whole step is `null`.",
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
