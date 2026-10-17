//! Request counts (DESIGN.md §22). Every request the handler answers is
//! counted for the service its host routes to — or as unrouted — by status
//! class and latency (with the time they took in all), in atomic counters
//! that only go up. A route holds its service's counters, so counting takes
//! no lock and touches no disk.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, PoisonError, RwLock};
use std::time::Duration;

use ferry_core::requests::{LATENCY_BUCKETS, RequestCounts, StatusClass, latency_bucket};

/// Whom requests are counted for.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum RequestTarget {
    /// The service a host routes to: its id, as given to
    /// [`RouteTable::set_service_routes`](crate::RouteTable::set_service_routes).
    Service(String),
    /// No service: a host no route has, a request without a valid host, and
    /// the ACME challenges and domain checks the proxy answers itself.
    Unrouted,
}

/// The requests of one target since the proxy started.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestTotals {
    pub target: RequestTarget,
    pub counts: RequestCounts,
}

/// The counters of one target.
#[derive(Debug, Default)]
pub(crate) struct Counters {
    /// By [`StatusClass::index`].
    status: [AtomicU64; 4],
    /// By [`latency_bucket`].
    latency: [AtomicU64; LATENCY_BUCKETS],
    /// The time the requests of `latency` took, all together, in
    /// microseconds (the `_sum` of a Prometheus histogram).
    latency_micros: AtomicU64,
}

impl Counters {
    /// Count one response. A `101 Switching Protocols` (a WebSocket that
    /// opened) is a success and stays out of the latency.
    pub(crate) fn record(&self, status: u16, elapsed: Duration) {
        self.status[StatusClass::of(status).index()].fetch_add(1, Ordering::Relaxed);
        if status != 101 {
            self.latency[latency_bucket(elapsed)].fetch_add(1, Ordering::Relaxed);
            let micros = u64::try_from(elapsed.as_micros()).unwrap_or(u64::MAX);
            self.latency_micros.fetch_add(micros, Ordering::Relaxed);
        }
    }

    fn read(&self) -> RequestCounts {
        let status = |class: StatusClass| self.status[class.index()].load(Ordering::Relaxed);
        let mut counts = RequestCounts {
            status_2xx: status(StatusClass::Success),
            status_3xx: status(StatusClass::Redirect),
            status_4xx: status(StatusClass::ClientError),
            status_5xx: status(StatusClass::ServerError),
            ..RequestCounts::default()
        };
        for (n, counter) in counts.latency.0.iter_mut().zip(&self.latency) {
            *n = counter.load(Ordering::Relaxed);
        }
        counts
    }
}

/// The requests the proxy answered since it started, per service, and the
/// unrouted ones. The counters are never reset (a deleted service's stay
/// until ferryd stops), so two readings always differ by what happened in
/// between. Cheap to clone: every clone counts into the same counters.
///
/// The [`RouteTable`](crate::RouteTable) holds the one the proxy counts into
/// ([`RouteTable::request_stats`](crate::RouteTable::request_stats)).
#[derive(Debug, Clone, Default)]
pub struct RequestStats {
    inner: Arc<Shared>,
}

#[derive(Debug, Default)]
struct Shared {
    unrouted: Arc<Counters>,
    services: RwLock<HashMap<String, Arc<Counters>>>,
}

impl RequestStats {
    pub fn new() -> Self {
        Self::default()
    }

    /// Count one request answered with `status` after `duration` (until its
    /// response headers went out). A `101 Switching Protocols` (a WebSocket
    /// that opened) counts as a 2xx and stays out of the latency.
    pub fn record(&self, target: &RequestTarget, status: u16, duration: Duration) {
        match target {
            RequestTarget::Service(id) => self.counters(id).record(status, duration),
            RequestTarget::Unrouted => self.inner.unrouted.record(status, duration),
        }
    }

    /// Count into `counters`, or as unrouted.
    pub(crate) fn record_into(&self, counters: Option<&Counters>, status: u16, duration: Duration) {
        counters.unwrap_or(&self.inner.unrouted).record(status, duration);
    }

    /// The counters of a service, created the first time.
    pub(crate) fn counters(&self, service_id: &str) -> Arc<Counters> {
        let services = &self.inner.services;
        if let Some(counters) = services.read().unwrap_or_else(PoisonError::into_inner).get(service_id) {
            return counters.clone();
        }
        services.write().unwrap_or_else(PoisonError::into_inner).entry(service_id.to_string()).or_default().clone()
    }

    /// Every target's requests since the proxy started: the services that
    /// were ever routed or counted (sorted by id), then the unrouted ones.
    pub fn snapshot(&self) -> Vec<RequestTotals> {
        let mut totals: Vec<RequestTotals> = self
            .inner
            .services
            .read()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .map(|(id, counters)| RequestTotals { target: RequestTarget::Service(id.clone()), counts: counters.read() })
            .collect();
        totals.sort_by(|a, b| a.target.cmp(&b.target));
        totals.push(RequestTotals { target: RequestTarget::Unrouted, counts: self.inner.unrouted.read() });
        totals
    }

    /// The time the requests of `target` took since the proxy started, all
    /// together, until their response headers (WebSockets left out): what
    /// goes with the latency of [`snapshot`](Self::snapshot) as the `_sum`
    /// of a Prometheus histogram. Zero for a target never counted.
    pub fn latency_sum(&self, target: &RequestTarget) -> Duration {
        let micros = match target {
            RequestTarget::Service(id) => {
                let services = self.inner.services.read().unwrap_or_else(PoisonError::into_inner);
                services.get(id).map_or(0, |c| c.latency_micros.load(Ordering::Relaxed))
            }
            RequestTarget::Unrouted => self.inner.unrouted.latency_micros.load(Ordering::Relaxed),
        };
        Duration::from_micros(micros)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn counts_of(stats: &RequestStats, target: &RequestTarget) -> RequestCounts {
        stats.snapshot().into_iter().find(|t| &t.target == target).map(|t| t.counts).unwrap_or_default()
    }

    #[test]
    fn counts_by_target_status_class_and_latency() {
        let stats = RequestStats::new();
        let web = RequestTarget::Service("srv-web".into());
        for status in [200, 204, 301, 404, 502, 503, 504] {
            stats.record(&web, status, Duration::from_millis(3));
        }
        stats.record(&web, 101, Duration::from_millis(3));
        stats.record(&RequestTarget::Unrouted, 404, Duration::from_micros(200));

        let web_counts = counts_of(&stats, &web);
        assert_eq!(
            (web_counts.status_2xx, web_counts.status_3xx, web_counts.status_4xx, web_counts.status_5xx),
            (3, 1, 1, 3)
        );
        assert_eq!(web_counts.latency.count(), 7, "the WebSocket is not in the latency");
        assert_eq!(web_counts.latency.0[latency_bucket(Duration::from_millis(3))], 7);
        let unrouted = counts_of(&stats, &RequestTarget::Unrouted);
        assert_eq!((unrouted.status_4xx, unrouted.latency.0[0]), (1, 1));
        assert_eq!(stats.latency_sum(&web), Duration::from_millis(21), "7 timed requests of 3 ms");
        assert_eq!(stats.latency_sum(&RequestTarget::Unrouted), Duration::from_micros(200));
        assert_eq!(stats.latency_sum(&RequestTarget::Service("srv-never".into())), Duration::ZERO);

        // Clones count into the same counters; a route's counters are the service's.
        stats.clone().record_into(Some(&stats.counters("srv-web")), 200, Duration::ZERO);
        stats.record_into(None, 200, Duration::ZERO);
        assert_eq!(counts_of(&stats, &web).status_2xx, 4);
        assert_eq!(counts_of(&stats, &RequestTarget::Unrouted).status_2xx, 1);

        let targets: Vec<RequestTarget> = stats.snapshot().into_iter().map(|t| t.target).collect();
        assert_eq!(targets, [web, RequestTarget::Unrouted]);
    }

    #[test]
    fn counting_from_many_threads_loses_nothing() {
        let stats = RequestStats::new();
        std::thread::scope(|s| {
            for _ in 0..8 {
                s.spawn(|| {
                    let target = RequestTarget::Service("srv-1".into());
                    for i in 0..1_000u64 {
                        stats.record(&target, if i % 10 == 0 { 500 } else { 200 }, Duration::from_millis(i % 50));
                    }
                });
            }
        });
        let counts = counts_of(&stats, &RequestTarget::Service("srv-1".into()));
        assert_eq!((counts.status_2xx, counts.status_5xx, counts.latency.count()), (7_200, 800, 8_000));
    }
}
