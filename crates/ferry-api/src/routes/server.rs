//! How the server is doing: the machine and its containers (DESIGN.md §22).

use axum::Json;
use axum::extract::State;
use ferry_core::dto::ServerMetrics;

use crate::AppState;
use crate::error::ApiResult;

/// `GET /api/v1/server/metrics`
#[utoipa::path(
    get,
    path = "/api/v1/server/metrics",
    tag = "server",
    operation_id = "getServerMetrics",
    summary = "Live server metrics",
    description = "How busy the machine and its containers are right now. The host: CPU use (0–100 % of the whole machine) and CPU count, memory used without the page cache and total, the used and total space and the read / write speed of the disk of the data directory (and of Docker's data directory when it is another local disk), and the network traffic without the loopback and Docker's own interfaces. The containers: every running service instance and datastore with its CPU use (100 = one core), memory use and limits, the busiest first. Speeds are bytes per second since the previous call (within a minute) or over one second, so a call takes about a second. Nothing is kept. A value the server can't read is `null` (on macOS, most host values); `containers` is `null` when Docker does not answer, and the CPU and memory of a container that does not answer in time are `null`.",
    responses((status = 200, description = "The server's metrics.", body = ServerMetrics)),
)]
pub async fn metrics(State(st): State<AppState>) -> ApiResult<Json<ServerMetrics>> {
    Ok(Json(st.engine.server_metrics().await?))
}
