//! The requests the proxy answered (DESIGN.md §22). The proxy counts them in
//! memory from the start of ferryd ([`RequestStats`], counters that only go
//! up). Every 10 s, on the wall clock's multiples of 10 s, this task stores
//! what each target got since its previous reading, and keeps that reading
//! for the live metrics; one last reading is stored at shutdown. The history
//! reads them back by step, with their percentiles, up to the last slot
//! stored. `GET /metrics` publishes the counters themselves ([`totals`]).

use std::collections::hash_map::Entry;
use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::{Duration, Instant};

use chrono::{DateTime, TimeDelta, Utc};
use ferry_core::dto::{HostPoint, RequestMetrics, RequestPoint};
use ferry_core::requests::{DASHBOARD, RequestCounts, TargetRequestTotals, TargetRequests, UNROUTED};
use ferry_core::{Result, Store};
use ferry_proxy::{RequestStats, RequestTarget, RequestTotals};
use tracing::warn;

use crate::reconcile::DASHBOARD_ROUTE;
use crate::state::Inner;
use crate::util::lock;

/// Time between two readings, in seconds: they are taken on the wall clock's
/// multiples of it, so each one is the requests of one slot of 10 s.
const SAMPLE_EVERY_SECS: i64 = 10;

/// The last reading, for `Engine::server_metrics` and the history.
#[derive(Default)]
pub(crate) struct State {
    /// The requests of the last reading stored.
    last: StdMutex<Vec<RequestMetrics>>,
    /// The end of the last slot stored (the start of the slot the sampler
    /// started in, before its first reading): the store has the requests
    /// before it.
    stored_until: StdMutex<Option<DateTime<Utc>>>,
}

impl State {
    /// The requests of the last reading stored; empty before the first one.
    pub(crate) fn last(&self) -> Vec<RequestMetrics> {
        lock(&self.last).clone()
    }

    /// Until when the store has the requests, at most `now`: the ones after
    /// are still counted in memory. `now` when the sampler never ran.
    pub(crate) fn stored_until(&self, now: DateTime<Utc>) -> DateTime<Utc> {
        lock(&self.stored_until).map_or(now, |until| until.min(now))
    }
}

/// One reading stored.
pub(crate) struct Reading {
    /// What each target got since the previous reading.
    pub requests: Vec<RequestMetrics>,
    /// The end of the slot it was stored in.
    pub until: DateTime<Utc>,
}

/// Reads the proxy's counters every 10 s, and one last time at shutdown
/// (spawned by `start`). A reading that can't be stored is not lost: the
/// next one counts from the same previous reading.
pub(crate) async fn run_sampler(inner: Arc<Inner>) {
    // The requests of a previous ferryd are all stored; the slot ferryd
    // starts in is stored with the first reading.
    let start = Utc::now().timestamp();
    *lock(&inner.requests.stored_until) =
        DateTime::from_timestamp(start - start.rem_euclid(SAMPLE_EVERY_SECS), 0).or_else(|| Some(Utc::now()));
    let mut sampler = RequestSampler::new();
    loop {
        let stopping = tokio::select! {
            _ = inner.shutdown.cancelled() => true,
            _ = tokio::time::sleep(until_next_slot(Utc::now())) => false,
        };
        read(&inner, &mut sampler).await;
        if stopping {
            return;
        }
    }
}

/// One reading of the proxy's counters, stored and kept for the live metrics.
pub(crate) async fn read(inner: &Inner, sampler: &mut RequestSampler) {
    match sample(&inner.store, inner.routes.request_stats(), sampler, Utc::now()).await {
        Ok(reading) => {
            *lock(&inner.requests.last) = reading.requests;
            *lock(&inner.requests.stored_until) = Some(reading.until);
        }
        Err(e) => warn!("storing the proxy's requests failed (the next reading keeps them): {e}"),
    }
}

/// The time until the wall clock's next multiple of 10 s (never zero).
fn until_next_slot(now: DateTime<Utc>) -> Duration {
    let every = SAMPLE_EVERY_SECS * 1_000;
    Duration::from_millis((every - now.timestamp_millis().rem_euclid(every)) as u64)
}

/// The start of the slot of 10 s a reading taken at `now` closes: the
/// multiple of 10 s nearest to `now`, minus 10 s. A reading a little early
/// or late still lands in its own slot; one in the middle of a slot (at
/// shutdown) adds to the nearest.
fn slot_of(now: DateTime<Utc>) -> DateTime<Utc> {
    let every = SAMPLE_EVERY_SECS * 1_000;
    let nearest = (now.timestamp_millis() + every / 2).div_euclid(every) * every;
    DateTime::from_timestamp_millis(nearest - every).unwrap_or(now)
}

/// Turns the proxy's counters, which only go up, into the requests between
/// two readings.
pub(crate) struct RequestSampler {
    /// The counters at the last reading stored.
    previous: HashMap<RequestTarget, RequestCounts>,
    /// When it was taken (when the sampler was created, before the first).
    previous_at: Instant,
}

impl RequestSampler {
    pub(crate) fn new() -> Self {
        RequestSampler { previous: HashMap::new(), previous_at: Instant::now() }
    }

    /// What each target got since the last reading stored; the targets
    /// without a request are left out.
    fn since_last<'a>(&self, now: &'a [RequestTotals]) -> Vec<(&'a RequestTarget, RequestCounts)> {
        now.iter()
            .filter_map(|t| {
                let since = self.previous.get(&t.target).map_or(t.counts, |previous| t.counts.since(previous));
                (!since.is_empty()).then_some((&t.target, since))
            })
            .collect()
    }

    /// `now`, taken at `at`, was stored: the next reading counts from it.
    fn stored(&mut self, now: Vec<RequestTotals>, at: Instant) {
        self.previous = now.into_iter().map(|t| (t.target, t.counts)).collect();
        self.previous_at = at;
    }
}

/// One reading: store, in the slot of `now`, what each target got since the
/// last reading stored, and return it for the live metrics. When it can't
/// be stored, the sampler keeps its previous reading, so the next one
/// counts these requests too.
pub(crate) async fn sample(
    store: &Store,
    stats: &RequestStats,
    sampler: &mut RequestSampler,
    now: DateTime<Utc>,
) -> Result<Reading> {
    let taken = Instant::now();
    let totals = stats.snapshot();
    let changes = sampler.since_last(&totals);
    let mut rows = Vec::with_capacity(changes.len());
    if !changes.is_empty() {
        let names: HashMap<String, String> =
            store.list_services().await?.into_iter().map(|s| (s.id.clone(), s.qualified_name())).collect();
        for (target, counts) in changes {
            let target = target_id(target);
            // A service deleted since: its id stands for its name.
            let name = names.get(target).cloned().unwrap_or_else(|| target.to_string());
            rows.push(TargetRequests { target: target.to_string(), name, counts });
        }
        store.insert_requests(slot_of(now), &rows).await?;
    }
    let seconds = taken.duration_since(sampler.previous_at).as_secs_f64();
    sampler.stored(totals, taken);
    Ok(Reading {
        requests: busiest_first(rows.iter().map(|r| request_metrics(r, seconds)).collect()),
        until: slot_of(now) + TimeDelta::seconds(SAMPLE_EVERY_SECS),
    })
}

/// Every target's requests since ferryd started, for `GET /metrics`: the
/// proxy's counters as they are, with the time the requests took. The
/// services (by name), then the dashboard, then the unrouted requests. When
/// the store can't give the services' names, their ids stand for them.
pub(crate) async fn totals(store: &Store, stats: &RequestStats) -> Vec<TargetRequestTotals> {
    let snapshot = stats.snapshot();
    let services = snapshot.iter().any(|t| matches!(&t.target, RequestTarget::Service(id) if id != DASHBOARD_ROUTE));
    let names: HashMap<String, String> = match services.then(|| store.list_services()) {
        None => HashMap::new(),
        Some(listed) => match listed.await {
            Ok(services) => services.into_iter().map(|s| (s.id.clone(), s.qualified_name())).collect(),
            Err(e) => {
                warn!("GET /metrics: the services' names can't be read, their ids stand for them: {e}");
                HashMap::new()
            }
        },
    };
    let mut totals: Vec<TargetRequestTotals> = snapshot
        .into_iter()
        .map(|t| {
            let target = target_id(&t.target);
            TargetRequestTotals {
                target: target.to_string(),
                // A service deleted since: its id stands for its name.
                name: names.get(target).cloned().unwrap_or_else(|| target.to_string()),
                counts: t.counts,
                latency_sum: stats.latency_sum(&t.target),
            }
        })
        .collect();
    totals.sort_by(|a, b| {
        rank(&a.target).cmp(&rank(&b.target)).then_with(|| a.name.cmp(&b.name)).then_with(|| a.target.cmp(&b.target))
    });
    totals
}

/// The services first, then the dashboard, then the unrouted requests.
fn rank(target: &str) -> u8 {
    match target {
        DASHBOARD => 1,
        UNROUTED => 2,
        _ => 0,
    }
}

/// The target of the store and the API: the service id, `dashboard` or
/// `unrouted`.
fn target_id(target: &RequestTarget) -> &str {
    match target {
        RequestTarget::Service(id) if id == DASHBOARD_ROUTE => DASHBOARD,
        RequestTarget::Service(id) => id,
        RequestTarget::Unrouted => UNROUTED,
    }
}

fn per_sec(n: u64, seconds: f64) -> f64 {
    if seconds > 0.0 { n as f64 / seconds } else { 0.0 }
}

/// The requests of one target over `seconds`.
fn request_metrics(r: &TargetRequests, seconds: f64) -> RequestMetrics {
    let c = &r.counts;
    RequestMetrics {
        target: r.target.clone(),
        name: r.name.clone(),
        requests_per_sec: per_sec(c.total(), seconds),
        status_2xx: c.status_2xx,
        status_3xx: c.status_3xx,
        status_4xx: c.status_4xx,
        status_5xx: c.status_5xx,
        p50_ms: c.latency.percentile(0.5),
        p95_ms: c.latency.percentile(0.95),
    }
}

/// The services, the busiest first (then by name), then the dashboard,
/// then the unrouted requests.
fn busiest_first(mut targets: Vec<RequestMetrics>) -> Vec<RequestMetrics> {
    targets.sort_by(|a, b| {
        rank(&a.target)
            .cmp(&rank(&b.target))
            .then_with(|| b.requests_per_sec.total_cmp(&a.requests_per_sec))
            .then_with(|| a.name.cmp(&b.name))
    });
    targets
}

/// The seconds of the step starting `at` whose requests are stored: the
/// step, or less for the one `stored_until` falls in (a step after it has
/// no request stored but from a previous ferryd: the whole step).
fn stored_seconds(at: DateTime<Utc>, step: u32, stored_until: DateTime<Utc>) -> u32 {
    match (stored_until - at).num_seconds() {
        left @ 0.. => u32::try_from(left).unwrap_or(step).min(step),
        _ => step,
    }
}

/// The requests of `MetricsHistory` from `from` to `to`, up to
/// `stored_until` (the requests after are not stored yet): the services'
/// together by step — a step of `host` (ferryd ran) without a request is a
/// point with zeros, and a step still filling counts over its stored
/// seconds — and each target over the range, named as in its last step.
pub(crate) async fn history(
    store: &Store,
    from: DateTime<Utc>,
    to: DateTime<Utc>,
    step: u32,
    host: &[HostPoint],
    stored_until: DateTime<Utc>,
) -> Result<(Vec<RequestPoint>, Vec<RequestMetrics>)> {
    // A step with no slot stored yet has no point, not a false zero.
    let mut by_step: BTreeMap<DateTime<Utc>, RequestCounts> =
        host.iter().filter(|p| p.at < stored_until).map(|p| (p.at, RequestCounts::default())).collect();
    let mut by_target: HashMap<String, TargetRequests> = HashMap::new();
    // Oldest first: the last name seen is the target's last one.
    for (at, r) in store.request_history(from, to, step).await? {
        if r.target != DASHBOARD && r.target != UNROUTED {
            by_step.entry(at).or_default().add(&r.counts);
        }
        match by_target.entry(r.target.clone()) {
            Entry::Occupied(mut e) => {
                let total = e.get_mut();
                total.name = r.name;
                total.counts.add(&r.counts);
            }
            Entry::Vacant(e) => {
                e.insert(r);
            }
        }
    }
    let points = by_step
        .into_iter()
        .map(|(at, c)| {
            let seconds = stored_seconds(at, step, stored_until);
            RequestPoint {
                at,
                seconds,
                requests_per_sec: per_sec(c.total(), f64::from(seconds)),
                status_2xx: c.status_2xx,
                status_3xx: c.status_3xx,
                status_4xx: c.status_4xx,
                status_5xx: c.status_5xx,
                p50_ms: c.latency.percentile(0.5),
                p95_ms: c.latency.percentile(0.95),
            }
        })
        .collect();
    let seconds = (stored_until.min(to) - from).num_milliseconds() as f64 / 1_000.0;
    Ok((points, busiest_first(by_target.values().map(|t| request_metrics(t, seconds)).collect())))
}

#[cfg(test)]
mod tests {
    use ferry_core::models::{Service, ServiceType};

    use super::*;

    fn at(secs: i64) -> DateTime<Utc> {
        DateTime::from_timestamp(secs, 0).unwrap()
    }

    fn ms(n: u64) -> Duration {
        Duration::from_millis(n)
    }

    /// Every target over the whole stored history.
    async fn stored(store: &Store) -> HashMap<String, (String, RequestCounts)> {
        let entries = store.request_history(at(0), Utc::now() + chrono::TimeDelta::days(1), 10).await.unwrap();
        let mut stored: HashMap<String, (String, RequestCounts)> = HashMap::new();
        for (_, r) in entries {
            let (name, counts) = stored.entry(r.target).or_default();
            *name = r.name;
            counts.add(&r.counts);
        }
        stored
    }

    #[test]
    fn readings_close_the_slots_of_the_wall_clock() {
        let t = |ms: i64| DateTime::from_timestamp_millis(1_760_000_000_000 + ms).unwrap();
        assert_eq!(until_next_slot(t(0)), Duration::from_secs(10), "never zero");
        assert_eq!(until_next_slot(t(2_500)), Duration::from_millis(7_500));
        assert_eq!(until_next_slot(t(9_999)), Duration::from_millis(1));
        // On time, a little late or a little early: the slot that just ended.
        assert_eq!(slot_of(t(10_000)), t(0));
        assert_eq!(slot_of(t(10_040)), t(0));
        assert_eq!(slot_of(t(9_990)), t(0));
        assert_eq!(slot_of(t(20_000)), t(10_000));
    }

    #[tokio::test]
    async fn the_sampler_stores_the_difference_between_two_readings() {
        let store = Store::open_in_memory().await.unwrap();
        let web = Service::new("web", ServiceType::WebService);
        store.create_service(&web).await.unwrap();
        let stats = RequestStats::new();
        let web_target = RequestTarget::Service(web.id.clone());
        let mut sampler = RequestSampler::new();

        for _ in 0..3 {
            stats.record(&web_target, 200, ms(5));
        }
        let reading = sample(&store, &stats, &mut sampler, at(1_000)).await.unwrap();
        assert_eq!(reading.until, at(1_000), "the slot [990, 1000) closed");
        let first = reading.requests;
        assert_eq!(first.len(), 1);
        assert_eq!(
            (first[0].target.as_str(), first[0].name.as_str(), first[0].status_2xx),
            (web.id.as_str(), "web", 3)
        );
        assert!(first[0].requests_per_sec > 0.0 && first[0].p50_ms.is_some());

        // Nothing new: nothing stored, nothing live.
        let quiet = sample(&store, &stats, &mut sampler, at(1_010)).await.unwrap();
        assert!(quiet.requests.is_empty());
        assert_eq!(quiet.until, at(1_010), "a slot without requests is closed too");

        stats.record(&web_target, 500, ms(80));
        stats.record(&web_target, 101, ms(1));
        stats.record(&RequestTarget::Service(DASHBOARD_ROUTE.into()), 200, ms(1));
        stats.record(&RequestTarget::Unrouted, 404, ms(1));
        stats.record(&RequestTarget::Service("srv-gone".into()), 200, ms(1));
        let third = sample(&store, &stats, &mut sampler, at(1_020)).await.unwrap().requests;
        let live: Vec<_> =
            third.iter().map(|r| (r.target.as_str(), r.name.as_str(), r.status_2xx, r.status_5xx)).collect();
        assert_eq!(
            live,
            [
                (web.id.as_str(), "web", 1, 1),
                ("srv-gone", "srv-gone", 1, 0),
                (DASHBOARD, DASHBOARD, 1, 0),
                (UNROUTED, UNROUTED, 0, 0),
            ],
            "only what came since the previous reading; the services first, then the dashboard and unrouted"
        );

        let stored = stored(&store).await;
        let (_, counts) = &stored[&web.id];
        assert_eq!((counts.status_2xx, counts.status_5xx), (4, 1), "nothing counted twice");
        assert_eq!(counts.latency.count(), 4, "the WebSocket is not in the latency");
        assert_eq!(stored[UNROUTED].1.status_4xx, 1);
    }

    #[tokio::test]
    async fn get_metrics_publishes_the_counters_since_the_start() {
        let store = Store::open_in_memory().await.unwrap();
        let web = Service::new("web", ServiceType::WebService);
        store.create_service(&web).await.unwrap();
        let stats = RequestStats::new();
        let targets = |totals: &[TargetRequestTotals]| totals.iter().map(|t| t.target.clone()).collect::<Vec<_>>();
        assert_eq!(targets(&totals(&store, &stats).await), [UNROUTED], "unrouted is always there");

        let web_target = RequestTarget::Service(web.id.clone());
        stats.record(&web_target, 200, ms(5));
        stats.record(&web_target, 503, ms(15));
        stats.record(&web_target, 101, ms(1_000));
        stats.record(&RequestTarget::Service(DASHBOARD_ROUTE.into()), 200, ms(2));
        stats.record(&RequestTarget::Service("srv-gone".into()), 404, ms(1));
        // A reading of the history resets nothing.
        sample(&store, &stats, &mut RequestSampler::new(), Utc::now()).await.unwrap();
        stats.record(&web_target, 200, ms(30));

        let all = totals(&store, &stats).await;
        let summary: Vec<_> = all.iter().map(|t| (t.target.as_str(), t.name.as_str(), t.counts.total())).collect();
        assert_eq!(
            summary,
            [
                ("srv-gone", "srv-gone", 1),
                (web.id.as_str(), "web", 4),
                (DASHBOARD, DASHBOARD, 1),
                (UNROUTED, UNROUTED, 0)
            ],
            "the services by name (a deleted one by its id), then the dashboard and unrouted"
        );
        let web_totals = &all[1];
        assert_eq!((web_totals.counts.status_2xx, web_totals.counts.status_5xx), (3, 1));
        assert_eq!(web_totals.counts.latency.count(), 3, "the WebSocket is not in the latency");
        assert_eq!(web_totals.latency_sum, ms(50));

        // A store that can't answer: the ids stand for the names, and the counts are all there.
        store.pool().close().await;
        let fallback = totals(&store, &stats).await;
        assert!(fallback.iter().all(|t| t.name == t.target), "{fallback:?}");
        assert_eq!(fallback.iter().map(|t| t.counts.total()).sum::<u64>(), 6);
    }

    #[tokio::test]
    async fn a_reading_that_cannot_be_stored_is_kept_for_the_next_one() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ferry.db");
        let stats = RequestStats::new();
        let target = RequestTarget::Service("srv-1".into());
        let mut sampler = RequestSampler::new();

        let store = Store::open(&path).await.unwrap();
        stats.record(&target, 200, ms(5));
        sample(&store, &stats, &mut sampler, at(1_000)).await.unwrap();
        stats.record(&target, 200, ms(5));
        stats.record(&target, 200, ms(5));
        store.pool().close().await;
        assert!(sample(&store, &stats, &mut sampler, at(1_010)).await.is_err());

        stats.record(&target, 503, ms(5));
        let store = Store::open(&path).await.unwrap();
        let live = sample(&store, &stats, &mut sampler, at(1_020)).await.unwrap().requests;
        assert_eq!((live[0].status_2xx, live[0].status_5xx), (2, 1), "the requests of the failed reading too");
        let counts = stored(&store).await["srv-1"].1;
        assert_eq!((counts.status_2xx, counts.status_5xx), (3, 1), "nothing lost between readings");
    }

    #[tokio::test]
    async fn the_history_has_a_point_for_every_step_ferryd_ran_in() {
        let store = Store::open_in_memory().await.unwrap();
        let requests = |target: &str, name: &str, ok: u64, slow: u64| {
            let mut counts = RequestCounts::default();
            (0..ok).for_each(|_| counts.record(200, Some(ms(10))));
            (0..slow).for_each(|_| counts.record(500, Some(ms(2_000))));
            TargetRequests { target: target.into(), name: name.into(), counts }
        };
        let readings = [
            (600, vec![requests("srv-a", "web", 50, 0), requests(UNROUTED, UNROUTED, 900, 0)]),
            (610, vec![requests("srv-b", "api", 0, 10), requests(DASHBOARD, DASHBOARD, 30, 0)]),
            (620, vec![requests("srv-a", "web", 10, 0)]),
            // Renamed since.
            (850, vec![requests("srv-a", "site", 1, 0)]),
        ];
        for (secs, rows) in readings {
            store.insert_requests(at(secs), &rows).await.unwrap();
        }
        // The host was read in four steps of 60 s.
        let host: Vec<HostPoint> =
            [600, 660, 720, 840].into_iter().map(|s| HostPoint { at: at(s), ..HostPoint::default() }).collect();

        let (points, targets) = history(&store, at(600), at(900), 60, &host, at(900)).await.unwrap();
        let summary: Vec<_> = points.iter().map(|p| (p.at.timestamp(), p.status_2xx, p.status_5xx)).collect();
        assert_eq!(
            summary,
            [(600, 60, 10), (660, 0, 0), (720, 0, 0), (840, 1, 0)],
            "the services only; zeros where none came"
        );
        assert_eq!((points[0].seconds, points[0].requests_per_sec), (60, 70.0 / 60.0));
        assert!(points[0].p50_ms.unwrap() <= 10.0 && points[0].p95_ms.unwrap() > 1_500.0);
        assert_eq!((points[1].p50_ms, points[1].p95_ms), (None, None));

        let summary: Vec<_> =
            targets.iter().map(|t| (t.target.as_str(), t.name.as_str(), t.status_2xx + t.status_5xx)).collect();
        assert_eq!(
            summary,
            [("srv-a", "site", 61), ("srv-b", "api", 10), (DASHBOARD, DASHBOARD, 30), (UNROUTED, UNROUTED, 900)],
            "the services busiest first, with their last name; the dashboard then unrouted last, however busy"
        );
        assert_eq!(targets[0].requests_per_sec, 61.0 / 300.0, "over the range");
        assert_eq!(targets[1].p95_ms.map(|p| p > 1_500.0), Some(true));
    }

    /// The host is read at any time in a step; the requests of a slot are
    /// stored when it ends.
    #[tokio::test]
    async fn the_step_still_filling_counts_over_its_stored_seconds() {
        let store = Store::open_in_memory().await.unwrap();
        let slot = |secs: i64, n: u64| {
            let mut counts = RequestCounts::default();
            (0..n).for_each(|_| counts.record(200, Some(ms(10))));
            (at(secs), vec![TargetRequests { target: "srv-a".into(), name: "web".into(), counts }])
        };
        // 60 requests every 10 s: 6 per second.
        for secs in (600..680).step_by(10) {
            let (at, rows) = slot(secs, 60);
            store.insert_requests(at, &rows).await.unwrap();
        }
        let host = |secs: &[i64]| -> Vec<HostPoint> {
            secs.iter().map(|&s| HostPoint { at: at(s), ..HostPoint::default() }).collect()
        };

        // Steps of 60 s, at 685: [660, 720) has 2 slots stored of 6.
        let (points, targets) = history(&store, at(600), at(685), 60, &host(&[600, 660]), at(680)).await.unwrap();
        let summary: Vec<_> = points.iter().map(|p| (p.at.timestamp(), p.seconds, p.requests_per_sec)).collect();
        assert_eq!(summary, [(600, 60, 6.0), (660, 20, 6.0)], "not a fraction of the real rate");
        assert_eq!(targets[0].requests_per_sec, 480.0 / 80.0, "over the time stored");

        // Steps of 10 s, at 683: the host was read in [680, 690), its requests are not stored yet.
        let (points, _) = history(&store, at(600), at(683), 10, &host(&[660, 670, 680]), at(680)).await.unwrap();
        let summary: Vec<_> = points.iter().map(|p| (p.at.timestamp(), p.status_2xx)).collect();
        assert_eq!(summary.last(), Some(&(670, 60)), "no false zero at the end: {summary:?}");
        assert!(points.iter().all(|p| p.seconds == 10));
    }
}
