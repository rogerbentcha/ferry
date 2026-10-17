//! The requests the proxy answers (DESIGN.md §22): how many each target got,
//! by status class, and how long they took, in a histogram of fixed buckets.
//!
//! The proxy counts them from the start of ferryd and never resets its
//! counters; every 10 s the engine stores what changed since its previous
//! reading ([`RequestCounts::since`]). Percentiles come from the histogram:
//! exact to the bucket, interpolated inside it.

use std::time::Duration;

/// The target of the requests to a host no service has (unknown names,
/// bad requests, ACME challenges and domain checks the proxy answers itself).
pub const UNROUTED: &str = "unrouted";
/// The target of the requests to the dashboard's host (`--dashboard-host`).
pub const DASHBOARD: &str = "dashboard";

/// The upper bounds of the latency buckets, in milliseconds. A request that
/// took `d` counts in the first bucket whose bound is at least `d`; one that
/// took longer than the last bound counts in a last bucket of its own.
pub const LATENCY_BOUNDS_MS: [u32; 21] =
    [1, 2, 5, 10, 20, 30, 50, 75, 100, 150, 200, 300, 500, 750, 1_000, 1_500, 2_000, 3_000, 5_000, 10_000, 30_000];

/// The number of latency buckets: one per bound, and one for the longer requests.
pub const LATENCY_BUCKETS: usize = LATENCY_BOUNDS_MS.len() + 1;

/// The bucket a request that took `duration` counts in.
pub fn latency_bucket(duration: Duration) -> usize {
    let micros = duration.as_micros();
    LATENCY_BOUNDS_MS.iter().position(|&ms| micros <= u128::from(ms) * 1_000).unwrap_or(LATENCY_BOUNDS_MS.len())
}

/// The class of a status code. A `101 Switching Protocols` (a WebSocket
/// that opened) is a success.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StatusClass {
    /// 2xx, and the WebSockets that opened.
    Success,
    /// 3xx.
    Redirect,
    /// 4xx.
    ClientError,
    /// 5xx.
    ServerError,
}

impl StatusClass {
    /// Every class, in the order of [`StatusClass::index`].
    pub const ALL: [StatusClass; 4] =
        [StatusClass::Success, StatusClass::Redirect, StatusClass::ClientError, StatusClass::ServerError];

    pub fn of(status: u16) -> StatusClass {
        match status {
            300..=399 => StatusClass::Redirect,
            400..=499 => StatusClass::ClientError,
            500.. => StatusClass::ServerError,
            _ => StatusClass::Success,
        }
    }

    /// 0 to 3, from 2xx to 5xx.
    pub fn index(self) -> usize {
        self as usize
    }
}

/// How many requests fell in each latency bucket ([`LATENCY_BOUNDS_MS`]).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LatencyHistogram(pub [u64; LATENCY_BUCKETS]);

impl LatencyHistogram {
    /// Count one request that took `duration`.
    pub fn record(&mut self, duration: Duration) {
        self.0[latency_bucket(duration)] += 1;
    }

    /// The requests counted.
    pub fn count(&self) -> u64 {
        self.0.iter().sum()
    }

    /// Both histograms together.
    pub fn add(&mut self, other: &LatencyHistogram) {
        for (n, m) in self.0.iter_mut().zip(other.0) {
            *n = n.saturating_add(m);
        }
    }

    /// What was counted after `earlier`, a reading of the same counters.
    pub fn since(&self, earlier: &LatencyHistogram) -> LatencyHistogram {
        let mut out = *self;
        for (n, m) in out.0.iter_mut().zip(earlier.0) {
            *n = n.saturating_sub(m);
        }
        out
    }

    /// The time, in milliseconds, that a share `q` (0–1) of the requests
    /// took at most: `0.95` → 95 % of the requests were this fast or faster.
    /// Exact to the bucket, interpolated linearly inside it; at least 30 s
    /// (the last bound) for one that falls in the last bucket. `None`
    /// without a request.
    pub fn percentile(&self, q: f64) -> Option<f64> {
        let total = self.count();
        if total == 0 {
            return None;
        }
        let rank = q.clamp(0.0, 1.0) * total as f64;
        let mut below = 0u64;
        for (i, &n) in self.0.iter().enumerate() {
            if n == 0 {
                continue;
            }
            let lower = if i == 0 { 0.0 } else { f64::from(LATENCY_BOUNDS_MS[i - 1]) };
            if (below + n) as f64 >= rank {
                let Some(&upper) = LATENCY_BOUNDS_MS.get(i) else { return Some(lower) };
                let inside = ((rank - below as f64) / n as f64).clamp(0.0, 1.0);
                return Some(lower + (f64::from(upper) - lower) * inside);
            }
            below += n;
        }
        LATENCY_BOUNDS_MS.last().map(|&ms| f64::from(ms))
    }
}

/// The requests of one target: by status class, and how long they took
/// until the response headers went out (WebSockets left out).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RequestCounts {
    pub status_2xx: u64,
    pub status_3xx: u64,
    pub status_4xx: u64,
    pub status_5xx: u64,
    pub latency: LatencyHistogram,
}

impl RequestCounts {
    /// The requests of `class`.
    pub fn status(&self, class: StatusClass) -> u64 {
        match class {
            StatusClass::Success => self.status_2xx,
            StatusClass::Redirect => self.status_3xx,
            StatusClass::ClientError => self.status_4xx,
            StatusClass::ServerError => self.status_5xx,
        }
    }

    fn status_mut(&mut self, class: StatusClass) -> &mut u64 {
        match class {
            StatusClass::Success => &mut self.status_2xx,
            StatusClass::Redirect => &mut self.status_3xx,
            StatusClass::ClientError => &mut self.status_4xx,
            StatusClass::ServerError => &mut self.status_5xx,
        }
    }

    /// Count one request; `duration` is `None` for a WebSocket.
    pub fn record(&mut self, status: u16, duration: Option<Duration>) {
        *self.status_mut(StatusClass::of(status)) += 1;
        if let Some(duration) = duration {
            self.latency.record(duration);
        }
    }

    /// Every request, whatever its status.
    pub fn total(&self) -> u64 {
        StatusClass::ALL.iter().map(|&c| self.status(c)).fold(0, u64::saturating_add)
    }

    pub fn is_empty(&self) -> bool {
        self.total() == 0 && self.latency.count() == 0
    }

    /// Both counts together.
    pub fn add(&mut self, other: &RequestCounts) {
        for class in StatusClass::ALL {
            let n = self.status_mut(class);
            *n = n.saturating_add(other.status(class));
        }
        self.latency.add(&other.latency);
    }

    /// What was counted after `earlier`, a reading of the same counters
    /// (they only go up: a value below the earlier one counts as nothing).
    pub fn since(&self, earlier: &RequestCounts) -> RequestCounts {
        let mut out = *self;
        for class in StatusClass::ALL {
            *out.status_mut(class) = self.status(class).saturating_sub(earlier.status(class));
        }
        out.latency = self.latency.since(&earlier.latency);
        out
    }
}

/// The requests of one target, as the store keeps them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetRequests {
    /// A service id, [`DASHBOARD`] or [`UNROUTED`].
    pub target: String,
    /// The service's name (the target itself for the others).
    pub name: String,
    pub counts: RequestCounts,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: f64) -> Duration {
        Duration::from_secs_f64(n / 1_000.0)
    }

    #[test]
    fn requests_fall_in_the_first_bucket_that_holds_them() {
        assert_eq!(latency_bucket(Duration::ZERO), 0);
        assert_eq!(latency_bucket(ms(1.0)), 0);
        assert_eq!(latency_bucket(ms(1.001)), 1);
        assert_eq!(latency_bucket(ms(100.0)), 8);
        assert_eq!(latency_bucket(ms(30_000.0)), LATENCY_BOUNDS_MS.len() - 1);
        assert_eq!(latency_bucket(ms(30_001.0)), LATENCY_BUCKETS - 1);
        assert_eq!(latency_bucket(Duration::from_secs(86_400)), LATENCY_BUCKETS - 1);
        assert!(LATENCY_BOUNDS_MS.windows(2).all(|w| w[0] < w[1]), "the bounds go up");
    }

    #[test]
    fn status_classes() {
        assert_eq!(StatusClass::of(200), StatusClass::Success);
        assert_eq!(StatusClass::of(101), StatusClass::Success, "a WebSocket that opened");
        assert_eq!(StatusClass::of(308), StatusClass::Redirect);
        assert_eq!(StatusClass::of(404), StatusClass::ClientError);
        for status in [500, 502, 503, 504, 599] {
            assert_eq!(StatusClass::of(status), StatusClass::ServerError);
        }
        let indexes: Vec<usize> = StatusClass::ALL.iter().map(|c| c.index()).collect();
        assert_eq!(indexes, [0, 1, 2, 3]);
    }

    /// The true percentile of `durations` (sorted, in ms) and the bucket
    /// that holds it.
    fn exact(durations: &[f64], q: f64) -> f64 {
        let rank = ((q * durations.len() as f64).ceil() as usize).clamp(1, durations.len());
        durations[rank - 1]
    }

    fn bucket_width(value_ms: f64) -> f64 {
        let i = latency_bucket(ms(value_ms));
        let lower = if i == 0 { 0.0 } else { f64::from(LATENCY_BOUNDS_MS[i - 1]) };
        f64::from(LATENCY_BOUNDS_MS[i]) - lower
    }

    #[test]
    fn percentiles_are_within_one_bucket() {
        let cases: Vec<Vec<f64>> = vec![
            // Most answers in ~2 s (the "slow pages" of the issue).
            (0..1_000).map(|i| 1_800.0 + f64::from(i % 400)).collect(),
            // Fast, with a slow tail.
            (0..1_000).map(|i| if i < 900 { 3.0 + f64::from(i % 20) } else { 400.0 + f64::from(i) }).collect(),
            // Spread over every bucket.
            (1..=3_000).map(|i| f64::from(i) * 9.7).collect(),
            vec![42.0],
        ];
        for mut durations in cases {
            durations.sort_by(f64::total_cmp);
            let mut histogram = LatencyHistogram::default();
            for &d in &durations {
                histogram.record(ms(d));
            }
            assert_eq!(histogram.count(), durations.len() as u64);
            for q in [0.5, 0.95] {
                let want = exact(&durations, q);
                let got = histogram.percentile(q).unwrap();
                assert!(
                    (got - want).abs() <= bucket_width(want),
                    "p{} of {} requests: {got} ms, the exact one is {want} ms",
                    q * 100.0,
                    durations.len()
                );
            }
        }
    }

    #[test]
    fn percentiles_of_the_edges() {
        assert_eq!(LatencyHistogram::default().percentile(0.5), None);
        let mut h = LatencyHistogram::default();
        h.record(Duration::from_secs(120));
        assert_eq!(h.percentile(0.95), Some(30_000.0), "longer than the last bound");
        let mut h = LatencyHistogram::default();
        for _ in 0..4 {
            h.record(ms(15.0));
        }
        // 4 requests in (10, 20]: the median is half-way through the bucket.
        assert_eq!(h.percentile(0.5), Some(15.0));
        assert_eq!(h.percentile(1.0), Some(20.0));
        assert_eq!(h.percentile(0.0), Some(10.0));
    }

    #[test]
    fn counts_only_go_up_so_a_reading_minus_the_previous_one_is_what_happened_between() {
        let mut counter = RequestCounts::default();
        let start = counter;
        counter.record(200, Some(ms(5.0)));
        counter.record(101, None);
        counter.record(503, Some(ms(0.5)));
        let first = counter;
        counter.record(404, Some(ms(40.0)));
        counter.record(301, Some(ms(2.0)));
        let second = counter;

        let a = first.since(&start);
        let b = second.since(&first);
        assert_eq!((a.status_2xx, a.status_3xx, a.status_4xx, a.status_5xx), (2, 0, 0, 1));
        assert_eq!(a.latency.count(), 2, "the WebSocket is not in the latency");
        assert_eq!((b.status_2xx, b.status_3xx, b.status_4xx, b.status_5xx), (0, 1, 1, 0));
        let mut both = a;
        both.add(&b);
        assert_eq!(both, second.since(&start), "nothing counted twice or lost");
        assert_eq!(both.total(), 5);
        assert!(second.since(&second).is_empty());
        // A reading taken before the previous one (it can't happen) counts nothing.
        assert!(first.since(&second).is_empty());
    }
}
