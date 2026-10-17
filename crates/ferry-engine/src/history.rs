//! The last 24 hours of the server's metrics (`Engine::metrics_history`,
//! DESIGN.md §22): a reading of the host and the containers every 10 s,
//! kept in the store, deleted once 24 hours old (the requests of `requests`
//! too), and read back averaged into 360 steps at most. The last reading
//! is also what `GET /metrics` publishes (`metrics::LastReading`).

use std::future::Future;
use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use ferry_core::dto::{ContainerMetrics, ContainerSeries, HostMetrics, HostPoint, MetricsHistory, MetricsRange};
use ferry_core::{CancellationToken, Result, Store};
use tokio::time::MissedTickBehavior;
use tracing::{debug, warn};

use crate::metrics::{self, HostSampler};
use crate::requests;
use crate::state::Inner;

/// Time between two readings.
const SAMPLE_EVERY: Duration = Duration::from_secs(10);
/// How long the readings are kept.
const KEEP: chrono::TimeDelta = chrono::TimeDelta::hours(24);
/// Time between two deletions of the old readings.
const PRUNE_EVERY: Duration = Duration::from_secs(5 * 60);
/// The containers a history shows.
const TOP_CONTAINERS: usize = 5;

/// One reading: the host, and the CPU and memory of the containers.
pub(crate) type Reading = (HostPoint, Vec<ContainerMetrics>);

/// Reads the server every 10 s until shutdown (spawned by `start`).
pub(crate) async fn run_sampler(inner: Arc<Inner>) {
    // Its own sampler: each rate is the average over the 10 s since the
    // previous reading, whoever reads the live metrics meanwhile.
    let sampler = HostSampler::new();
    sample_loop(&inner.store, &inner.shutdown, SAMPLE_EVERY, || read(&inner, &sampler)).await;
}

/// Take a reading with `read` every `every` and store it, until `shutdown`;
/// delete the readings older than 24 hours first, then every few minutes.
/// A reading that fails or is not done before the next one is due is
/// skipped.
async fn sample_loop<F, Fut>(store: &Store, shutdown: &CancellationToken, every: Duration, mut read: F)
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<Reading>>,
{
    let mut tick = tokio::time::interval(every);
    tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
    let bound = every.saturating_sub(every / 10);
    let mut pruned: Option<Instant> = None;
    loop {
        tokio::select! {
            _ = shutdown.cancelled() => return,
            _ = tick.tick() => {}
        }
        if pruned.is_none_or(|at| at.elapsed() >= PRUNE_EVERY) {
            pruned = Some(Instant::now());
            match store.prune_metrics(Utc::now() - KEEP).await {
                Ok(0) => {}
                Ok(rows) => debug!(rows, "deleted the metrics older than 24 hours"),
                Err(e) => warn!("deleting the old metrics failed: {e}"),
            }
        }
        let reading = tokio::select! {
            _ = shutdown.cancelled() => return,
            reading = tokio::time::timeout(bound, read()) => reading,
        };
        match reading {
            Ok(Ok((host, containers))) => {
                if let Err(e) = store.insert_metrics(&host, &containers).await {
                    warn!("storing the server's metrics failed: {e}");
                }
            }
            Ok(Err(e)) => warn!("reading the server's metrics failed: {e}"),
            Err(_) => warn!("reading the server's metrics took more than {}s: skipped", bound.as_secs_f32()),
        }
    }
}

/// One reading of the server, also kept for `GET /metrics`. The containers
/// are left out when Docker does not answer.
pub(crate) async fn read(inner: &Inner, sampler: &HostSampler) -> Result<Reading> {
    let at = Utc::now();
    let (host, containers) = tokio::join!(metrics::host(inner, sampler), metrics::containers(inner, false));
    let containers = containers?;
    inner.last_reading.keep(sampler, containers.as_deref());
    Ok((host_point(at, &host), containers.unwrap_or_default()))
}

/// The host's values to keep: the disks' speeds are added together.
fn host_point(at: DateTime<Utc>, host: &HostMetrics) -> HostPoint {
    let disks = |speed: fn(&ferry_core::dto::DiskMetrics) -> Option<u64>| {
        host.disks.iter().filter_map(speed).reduce(u64::saturating_add)
    };
    HostPoint {
        at,
        cpu_percent: host.cpu_percent,
        memory_used_bytes: host.memory_used_bytes,
        memory_total_bytes: host.memory_total_bytes,
        disk_read_bytes_per_sec: disks(|d| d.read_bytes_per_sec),
        disk_write_bytes_per_sec: disks(|d| d.write_bytes_per_sec),
        network_rx_bytes_per_sec: host.network_rx_bytes_per_sec,
        network_tx_bytes_per_sec: host.network_tx_bytes_per_sec,
    }
}

/// `Engine::metrics_history`.
pub(crate) async fn metrics_history(inner: &Inner, range: MetricsRange) -> Result<MetricsHistory> {
    let to = Utc::now();
    let from = first_step(to, range);
    let step = range.step_seconds();
    let (host, containers) = inner.store.metrics_history(from, to, step).await?;
    let stored_until = inner.requests.stored_until(to);
    let (requests, request_targets) = requests::history(&inner.store, from, to, step, &host, stored_until).await?;
    Ok(MetricsHistory {
        range,
        step_seconds: step,
        from,
        to,
        host,
        containers: busiest(containers, TOP_CONTAINERS),
        requests,
        request_targets,
    })
}

/// The start of the first step of `range` when it ends at `to`: the step
/// after the one `to - range` falls in, so that the range has 360 steps at
/// most (the last one still filling).
fn first_step(to: DateTime<Utc>, range: MetricsRange) -> DateTime<Utc> {
    let step = i64::from(range.step_seconds());
    let start = to.timestamp() - i64::from(range.seconds());
    DateTime::from_timestamp(start.div_euclid(step) * step + step, 0).unwrap_or(to)
}

/// The `n` series whose CPU use peaked highest — so an app that was busy
/// for a few minutes in the night shows, which an average would hide —
/// then by name and instance.
fn busiest(series: Vec<ContainerSeries>, n: usize) -> Vec<ContainerSeries> {
    let mut ranked: Vec<(f64, ContainerSeries)> = series
        .into_iter()
        .map(|s| (s.points.iter().filter_map(|p| p.cpu_percent).fold(f64::NEG_INFINITY, f64::max), s))
        .collect();
    ranked.sort_by(|(a_peak, a), (b_peak, b)| {
        b_peak.total_cmp(a_peak).then_with(|| a.name.cmp(&b.name)).then_with(|| a.instance.cmp(&b.instance))
    });
    ranked.into_iter().take(n).map(|(_, s)| s).collect()
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use ferry_core::Error;
    use ferry_core::dto::{ContainerKind, ContainerPoint, DiskMetrics};

    use super::*;

    fn at(secs: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(secs, 0).unwrap()
    }

    fn reading(at: DateTime<Utc>) -> Reading {
        let web = ContainerMetrics {
            kind: ContainerKind::Service,
            id: "srv-1".into(),
            name: "web".into(),
            instance: Some("aaaaaa".into()),
            cpu_percent: Some(12.5),
            memory_bytes: Some(100 << 20),
            memory_limit_bytes: None,
            cpu_limit: None,
        };
        (HostPoint { at, cpu_percent: Some(40.0), ..HostPoint::default() }, vec![web])
    }

    /// The reader: 1st reading fine, 2nd fails, 3rd never ends, then fine.
    /// Each one a second after the previous (the store keeps one per second).
    fn reader(
        start: i64,
        calls: Arc<AtomicUsize>,
    ) -> impl FnMut() -> std::pin::Pin<Box<dyn Future<Output = Result<Reading>> + Send>> {
        move || {
            let n = calls.fetch_add(1, Ordering::SeqCst);
            Box::pin(async move {
                match n {
                    1 => Err(Error::internal("the store is gone")),
                    2 => std::future::pending().await,
                    _ => Ok(reading(at(start + n as i64))),
                }
            })
        }
    }

    #[tokio::test]
    async fn the_sampler_stores_readings_survives_failures_and_stops_on_shutdown() {
        let store = Store::open_in_memory().await.unwrap();
        let now = Utc::now().timestamp();
        // A reading older than 24 hours: deleted by the first pass.
        store.insert_metrics(&reading(at(now - 25 * 3600)).0, &[]).await.unwrap();

        let shutdown = CancellationToken::new();
        let calls = Arc::new(AtomicUsize::new(0));
        let task = tokio::spawn({
            let (store, shutdown, read) = (store.clone(), shutdown.clone(), reader(now, calls.clone()));
            async move { sample_loop(&store, &shutdown, Duration::from_millis(30), read).await }
        });
        // The 7th call starts once the 6th reading is stored.
        tokio::time::timeout(Duration::from_secs(10), async {
            while calls.load(Ordering::SeqCst) < 7 {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("the sampler keeps reading");
        shutdown.cancel();
        tokio::time::timeout(Duration::from_secs(2), task).await.expect("stops on shutdown").unwrap();
        let after_stop = calls.load(Ordering::SeqCst);
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(calls.load(Ordering::SeqCst), after_stop, "no reading after shutdown");

        let (host, containers) = store.metrics_history(at(now - 2 * 86_400), at(now + 3600), 1).await.unwrap();
        let stored: Vec<i64> = host.iter().map(|p| p.at.timestamp() - now).collect();
        // The 2nd (failed) and the 3rd (too slow) are skipped; the old one is gone.
        assert_eq!(&stored[..4], &[0, 3, 4, 5], "{stored:?}");
        assert!(host.iter().all(|p| p.cpu_percent == Some(40.0)));
        assert_eq!(containers.len(), 1);
        assert_eq!(containers[0].points.len(), stored.len());
    }

    #[test]
    fn the_disks_speeds_are_added_together() {
        let disk = |read, write| DiskMetrics {
            mount_point: "/".into(),
            used_bytes: 1,
            total_bytes: 2,
            read_bytes_per_sec: read,
            write_bytes_per_sec: write,
        };
        let host = HostMetrics {
            cpu_percent: Some(5.0),
            cpu_count: 2,
            memory_used_bytes: Some(1),
            memory_total_bytes: Some(2),
            disks: vec![disk(Some(100), None), disk(Some(50), None)],
            network_rx_bytes_per_sec: Some(3),
            network_tx_bytes_per_sec: None,
        };
        let p = host_point(at(60), &host);
        assert_eq!((p.disk_read_bytes_per_sec, p.disk_write_bytes_per_sec), (Some(150), None));
        assert_eq!((p.cpu_percent, p.memory_used_bytes, p.memory_total_bytes), (Some(5.0), Some(1), Some(2)));
        assert_eq!((p.network_rx_bytes_per_sec, p.network_tx_bytes_per_sec), (Some(3), None));
        let none = host_point(at(60), &HostMetrics { disks: vec![], ..host });
        assert_eq!(none.disk_read_bytes_per_sec, None);
    }

    #[test]
    fn a_range_has_360_steps_at_most() {
        for range in [MetricsRange::Hour, MetricsRange::SixHours, MetricsRange::Day] {
            let step = i64::from(range.step_seconds());
            assert_eq!(i64::from(range.seconds()) / step, 360);
            for to in [at(1_760_000_000), at(1_760_000_000 + 1), at(1_760_000_000 + step - 1)] {
                let from = first_step(to, range);
                assert_eq!(from.timestamp() % step, 0, "{range:?}: steps start at a multiple of the step");
                assert!(to - from < chrono::TimeDelta::seconds(range.seconds().into()), "{range:?}");
                let steps = (to.timestamp().div_euclid(step) * step - from.timestamp()) / step + 1;
                assert_eq!(steps, 360, "{range:?} {to}");
            }
        }
        assert_eq!(
            [MetricsRange::Hour, MetricsRange::SixHours, MetricsRange::Day].map(MetricsRange::step_seconds),
            [10, 60, 240]
        );
    }

    #[test]
    fn the_busiest_containers_are_the_ones_whose_cpu_peaked_highest() {
        let series = |name: &str, cpu: &[Option<f64>]| ContainerSeries {
            kind: ContainerKind::Service,
            id: name.into(),
            name: name.into(),
            instance: Some("aaaaaa".into()),
            points: cpu.iter().map(|&c| ContainerPoint { at: at(0), cpu_percent: c, memory_bytes: None }).collect(),
        };
        let all = vec![
            series("steady", &[Some(20.0), Some(20.0), Some(20.0)]),
            series("night", &[Some(0.0), Some(90.0), Some(0.0)]),
            series("unmeasured", &[None]),
            series("idle-b", &[Some(1.0)]),
            series("idle-a", &[Some(1.0)]),
            series("quiet", &[Some(0.5)]),
            series("tiny", &[Some(0.1)]),
        ];
        let names: Vec<String> = busiest(all, 5).into_iter().map(|s| s.name).collect();
        assert_eq!(names, ["night", "steady", "idle-a", "idle-b", "quiet"]);
    }
}
