//! `GET /metrics`: the server's numbers in the Prometheus text format,
//! version 0.0.4 (DESIGN.md §22), written by hand — a `# HELP` and a
//! `# TYPE` line per family, then one line per sample. A value ferryd can't
//! read is left out, never published as 0 or NaN; so is a family without a
//! sample.

use std::fmt::{Display, Write};

use ferry_core::dto::{ContainerKind, ContainerMetrics, DiskCounters, HostCounters, MetricsSnapshot, NetworkCounters};
use ferry_core::requests::{LATENCY_BOUNDS_MS, StatusClass, TargetRequestTotals};

/// The `Content-Type` of the text format.
pub const CONTENT_TYPE: &str = "text/plain; version=0.0.4; charset=utf-8";

/// The `# TYPE` of a family.
#[derive(Debug, Clone, Copy)]
enum Kind {
    Counter,
    Gauge,
    Histogram,
}

impl Kind {
    fn as_str(self) -> &'static str {
        match self {
            Kind::Counter => "counter",
            Kind::Gauge => "gauge",
            Kind::Histogram => "histogram",
        }
    }
}

/// The labels of a sample: `(name, value)`.
type Labels<'a> = Vec<(&'static str, &'a str)>;

/// The text `GET /metrics` answers with; `version` is ferryd's.
pub fn render(snapshot: &MetricsSnapshot, version: &str) -> String {
    let mut out = Exposition::default();
    out.family(
        "ferry_build_info",
        Kind::Gauge,
        "Always 1: the version of ferryd is in the label.",
        [(vec![("version", version)], 1)],
    );
    if let Some(host) = &snapshot.host {
        host_families(&mut out, host);
    }
    if let Some(containers) = &snapshot.containers {
        container_families(&mut out, containers);
    }
    request_families(&mut out, &snapshot.requests);
    out.text
}

fn host_families(out: &mut Exposition, host: &HostCounters) {
    out.family("ferry_host_cpu_count", Kind::Gauge, "CPUs of the machine.", [(vec![], host.cpu_count)]);
    out.family(
        "ferry_host_cpu_seconds_total",
        Kind::Counter,
        "Seconds all the CPUs spent in each mode since the machine started (/proc/stat).",
        host.cpu_seconds.iter().filter(|(_, s)| s.is_finite()).map(|(mode, s)| (vec![("mode", mode.as_str())], *s)),
    );
    out.family(
        "ferry_host_memory_total_bytes",
        Kind::Gauge,
        "Memory of the machine, in bytes.",
        host.memory_total_bytes.map(|v| (vec![], v)),
    );
    out.family(
        "ferry_host_memory_available_bytes",
        Kind::Gauge,
        "Memory available without swapping (MemAvailable), in bytes: the total minus the memory in use.",
        host.memory_available_bytes.map(|v| (vec![], v)),
    );
    out.family(
        "ferry_host_disk_total_bytes",
        Kind::Gauge,
        "Size of the filesystem of the data directory (and of Docker's data directory), in bytes.",
        host.disks.iter().map(|d| (disk(d), d.total_bytes)),
    );
    out.family(
        "ferry_host_disk_used_bytes",
        Kind::Gauge,
        "Used space of the filesystem, in bytes (like df).",
        host.disks.iter().map(|d| (disk(d), d.used_bytes)),
    );
    out.family(
        "ferry_host_disk_read_bytes_total",
        Kind::Counter,
        "Bytes read from the device of the filesystem since the machine started.",
        host.disks.iter().filter_map(|d| Some((disk(d), d.read_bytes?))),
    );
    out.family(
        "ferry_host_disk_written_bytes_total",
        Kind::Counter,
        "Bytes written to the device of the filesystem since the machine started.",
        host.disks.iter().filter_map(|d| Some((disk(d), d.written_bytes?))),
    );
    out.family(
        "ferry_host_network_receive_bytes_total",
        Kind::Counter,
        "Bytes received by each network interface since it came up, but the loopback and Docker's own.",
        host.network.iter().map(|n| (device(n), n.receive_bytes)),
    );
    out.family(
        "ferry_host_network_transmit_bytes_total",
        Kind::Counter,
        "Bytes sent by each network interface since it came up, but the loopback and Docker's own.",
        host.network.iter().map(|n| (device(n), n.transmit_bytes)),
    );
}

fn disk(d: &DiskCounters) -> Labels<'_> {
    vec![("mount_point", d.mount_point.as_str())]
}

fn device(n: &NetworkCounters) -> Labels<'_> {
    vec![("device", n.device.as_str())]
}

/// `kind`, `id`, `name`, and `instance_id` for a service instance (not
/// `instance`: Prometheus gives that label to every series it collects, the
/// address of the target, and would rename this one `exported_instance`).
fn container_labels(c: &ContainerMetrics) -> Labels<'_> {
    let kind = match c.kind {
        ContainerKind::Service => "service",
        ContainerKind::Datastore => "datastore",
    };
    let mut labels = vec![("kind", kind), ("id", c.id.as_str()), ("name", c.name.as_str())];
    if let Some(instance) = &c.instance {
        labels.push(("instance_id", instance.as_str()));
    }
    labels
}

fn container_families(out: &mut Exposition, containers: &[ContainerMetrics]) {
    out.family(
        "ferry_container_cpu_percent",
        Kind::Gauge,
        "CPU use of each running service instance and datastore, in percent of one core (100 = one core).",
        containers.iter().filter_map(|c| Some((container_labels(c), c.cpu_percent.filter(|v| v.is_finite())?))),
    );
    out.family(
        "ferry_container_memory_bytes",
        Kind::Gauge,
        "Memory use of each running service instance and datastore, without the page cache, in bytes.",
        containers.iter().filter_map(|c| Some((container_labels(c), c.memory_bytes?))),
    );
}

fn status_class(class: StatusClass) -> &'static str {
    match class {
        StatusClass::Success => "2xx",
        StatusClass::Redirect => "3xx",
        StatusClass::ClientError => "4xx",
        StatusClass::ServerError => "5xx",
    }
}

fn target_labels(r: &TargetRequestTotals) -> Labels<'_> {
    vec![("target", r.target.as_str()), ("name", r.name.as_str())]
}

fn request_families(out: &mut Exposition, requests: &[TargetRequestTotals]) {
    out.family(
        "ferry_http_requests_total",
        Kind::Counter,
        "Requests the proxy answered since ferryd started, by target (a service id, dashboard or unrouted) and \
         status class. A WebSocket that opened is a 2xx; a request whose client went away first is a 4xx (499).",
        requests.iter().flat_map(|r| {
            StatusClass::ALL.map(|class| {
                let mut labels = target_labels(r);
                labels.push(("status_class", status_class(class)));
                (labels, r.counts.status(class))
            })
        }),
    );
    let name = "ferry_http_request_duration_seconds";
    if requests.is_empty() {
        return;
    }
    out.header(
        name,
        Kind::Histogram,
        "Time until the response headers went out, in seconds, since ferryd started (WebSockets left out).",
    );
    for r in requests {
        let labels = target_labels(r);
        let mut below = 0u64;
        for (i, n) in r.counts.latency.0.iter().enumerate() {
            below = below.saturating_add(*n);
            let le = LATENCY_BOUNDS_MS
                .get(i)
                .map_or_else(|| "+Inf".to_string(), |ms| (f64::from(*ms) / 1_000.0).to_string());
            let mut bucket = labels.clone();
            bucket.push(("le", &le));
            out.sample(&format!("{name}_bucket"), &bucket, below);
        }
        out.sample(&format!("{name}_sum"), &labels, r.latency_sum.as_secs_f64());
        out.sample(&format!("{name}_count"), &labels, below);
    }
}

/// The text being written.
#[derive(Default)]
struct Exposition {
    text: String,
}

impl Exposition {
    /// A family and its samples; nothing without a sample.
    fn family<'a, V: Display>(
        &mut self,
        name: &str,
        kind: Kind,
        help: &str,
        samples: impl IntoIterator<Item = (Labels<'a>, V)>,
    ) {
        let mut samples = samples.into_iter().peekable();
        if samples.peek().is_none() {
            return;
        }
        self.header(name, kind, help);
        for (labels, value) in samples {
            self.sample(name, &labels, value);
        }
    }

    fn header(&mut self, name: &str, kind: Kind, help: &str) {
        let help = help.replace('\\', "\\\\").replace('\n', "\\n");
        let _ = writeln!(self.text, "# HELP {name} {help}");
        let _ = writeln!(self.text, "# TYPE {name} {}", kind.as_str());
    }

    fn sample(&mut self, name: &str, labels: &[(&str, &str)], value: impl Display) {
        self.text.push_str(name);
        if !labels.is_empty() {
            self.text.push('{');
            for (i, (label, value)) in labels.iter().enumerate() {
                if i > 0 {
                    self.text.push(',');
                }
                let value = value.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n");
                let _ = write!(self.text, "{label}=\"{value}\"");
            }
            self.text.push('}');
        }
        let _ = writeln!(self.text, " {value}");
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};
    use std::time::Duration;

    use ferry_core::requests::{LATENCY_BUCKETS, LatencyHistogram, RequestCounts};

    use super::*;

    /// One sample of the text.
    #[derive(Debug, Clone, PartialEq)]
    struct Sample {
        name: String,
        labels: BTreeMap<String, String>,
        value: f64,
    }

    /// One family: its `# TYPE`, its `# HELP` and its samples, in order.
    #[derive(Debug, Default)]
    struct Family {
        kind: String,
        help: String,
        samples: Vec<Sample>,
    }

    fn valid_name(name: &str, colon: bool) -> bool {
        let mut chars = name.chars();
        let ok = |c: char| c.is_ascii_alphanumeric() || c == '_' || (colon && c == ':');
        chars.next().is_some_and(|c| !c.is_ascii_digit() && ok(c)) && chars.all(ok)
    }

    /// Undo the escapes of `text`; `quote`: `\"` is allowed (label values).
    fn unescape(text: &str, quote: bool) -> String {
        let mut out = String::new();
        let mut chars = text.chars();
        while let Some(c) = chars.next() {
            if c != '\\' {
                assert!(!(quote && c == '"'), "an unescaped quote in {text:?}");
                out.push(c);
                continue;
            }
            match chars.next() {
                Some('\\') => out.push('\\'),
                Some('n') => out.push('\n'),
                Some('"') if quote => out.push('"'),
                other => panic!("bad escape \\{other:?} in {text:?}"),
            }
        }
        out
    }

    /// `name{a="b",c="d"} value`, by the rules of the text format 0.0.4.
    fn parse_sample(line: &str) -> Sample {
        let (series, value) = line.rsplit_once(' ').unwrap_or_else(|| panic!("no value: {line:?}"));
        let value = match value {
            "+Inf" => f64::INFINITY,
            "-Inf" => f64::NEG_INFINITY,
            v => v.parse::<f64>().unwrap_or_else(|_| panic!("not a number: {line:?}")),
        };
        assert!(!value.is_nan(), "NaN is never published: {line:?}");
        let (name, labels) = match series.split_once('{') {
            None => (series, BTreeMap::new()),
            Some((name, rest)) => {
                let mut rest = rest.strip_suffix('}').unwrap_or_else(|| panic!("no closing brace: {line:?}"));
                let mut labels = BTreeMap::new();
                while !rest.is_empty() {
                    let (label, after) = rest.split_once("=\"").unwrap_or_else(|| panic!("bad label: {line:?}"));
                    assert!(valid_name(label, false) && !label.starts_with("__"), "label name {label:?}: {line:?}");
                    // The value ends at the first quote that is not escaped.
                    let bytes = after.as_bytes();
                    let mut end = 0;
                    while bytes[end] != b'"' {
                        end += if bytes[end] == b'\\' { 2 } else { 1 };
                    }
                    let value = unescape(&after[..end], true);
                    assert!(labels.insert(label.to_string(), value).is_none(), "label {label} twice: {line:?}");
                    rest = &after[end + 1..];
                    rest = rest.strip_prefix(',').unwrap_or(rest);
                }
                (name, labels)
            }
        };
        assert!(valid_name(name, true), "metric name {name:?}");
        Sample { name: name.to_string(), labels, value }
    }

    /// Parse `text`, failing on anything the format does not allow: each
    /// family has one `# HELP` and one `# TYPE` before its samples, which
    /// follow each other; no series twice; a histogram's series have
    /// `_bucket`s with `le` going up to `+Inf`, cumulative, a `_sum` and a
    /// `_count` equal to the `+Inf` bucket; counters end in `_total`.
    fn parse(text: &str) -> BTreeMap<String, Family> {
        assert!(text.ends_with('\n'), "the text ends with a line feed");
        let mut families: BTreeMap<String, Family> = BTreeMap::new();
        let mut current: Option<String> = None;
        let mut seen = BTreeSet::new();
        for line in text.lines() {
            if let Some(rest) = line.strip_prefix("# HELP ") {
                let (name, help) = rest.split_once(' ').unwrap();
                assert!(valid_name(name, true), "{line}");
                assert!(!families.contains_key(name), "{name} declared twice");
                families.insert(name.to_string(), Family { help: unescape(help, false), ..Family::default() });
                current = Some(name.to_string());
            } else if let Some(rest) = line.strip_prefix("# TYPE ") {
                let (name, kind) = rest.split_once(' ').unwrap();
                assert_eq!(current.as_deref(), Some(name), "# TYPE right after its # HELP: {line}");
                let family = families.get_mut(name).unwrap();
                assert!(family.kind.is_empty() && family.samples.is_empty(), "{line}");
                assert!(["counter", "gauge", "histogram"].contains(&kind), "{line}");
                if kind == "counter" {
                    assert!(name.ends_with("_total"), "a counter ends in _total: {name}");
                }
                family.kind = kind.to_string();
            } else {
                assert!(!line.is_empty() && !line.starts_with('#'), "unexpected line {line:?}");
                let sample = parse_sample(line);
                let name = current.clone().unwrap_or_else(|| panic!("a sample before any # TYPE: {line}"));
                let family = families.get_mut(&name).unwrap();
                let allowed = if family.kind == "histogram" {
                    ["_bucket", "_sum", "_count"].iter().any(|s| sample.name == format!("{name}{s}"))
                } else {
                    sample.name == name
                };
                assert!(allowed, "{} is not a sample of {name}, the family above it", sample.name);
                assert!(seen.insert((sample.name.clone(), sample.labels.clone())), "series twice: {line}");
                family.samples.push(sample);
            }
        }
        for (name, family) in &families {
            assert!(!family.kind.is_empty() && !family.samples.is_empty(), "{name}: no # TYPE or no sample");
            if family.kind == "histogram" {
                check_histogram(name, family);
            }
        }
        families
    }

    fn check_histogram(name: &str, family: &Family) {
        let mut series: BTreeMap<BTreeMap<String, String>, Vec<&Sample>> = BTreeMap::new();
        for s in &family.samples {
            let mut labels = s.labels.clone();
            labels.remove("le");
            series.entry(labels).or_default().push(s);
        }
        for (labels, samples) in series {
            let buckets: Vec<(f64, f64)> = samples
                .iter()
                .filter(|s| s.name.ends_with("_bucket"))
                .map(|s| {
                    let le = &s.labels["le"];
                    (if le == "+Inf" { f64::INFINITY } else { le.parse().unwrap() }, s.value)
                })
                .collect();
            assert!(buckets.windows(2).all(|w| w[0].0 < w[1].0), "{name}{labels:?}: le goes up");
            assert!(buckets.windows(2).all(|w| w[0].1 <= w[1].1), "{name}{labels:?}: cumulative");
            assert_eq!(buckets.last().map(|b| b.0), Some(f64::INFINITY), "{name}{labels:?}: ends with +Inf");
            let one = |suffix: &str| {
                let found: Vec<_> = samples.iter().filter(|s| s.name == format!("{name}{suffix}")).collect();
                assert_eq!(found.len(), 1, "{name}{suffix}{labels:?}");
                found[0].value
            };
            assert_eq!(one("_count"), buckets.last().unwrap().1, "{name}{labels:?}: _count is the +Inf bucket");
            assert!(one("_sum") >= 0.0);
        }
    }

    fn value(families: &BTreeMap<String, Family>, family: &str, name: &str, labels: &[(&str, &str)]) -> Option<f64> {
        let labels: BTreeMap<String, String> = labels.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        families.get(family)?.samples.iter().find(|s| s.name == name && s.labels == labels).map(|s| s.value)
    }

    fn counts(latency: &[(usize, u64)], status: [u64; 4]) -> RequestCounts {
        let mut histogram = LatencyHistogram::default();
        for &(bucket, n) in latency {
            histogram.0[bucket] = n;
        }
        RequestCounts {
            status_2xx: status[0],
            status_3xx: status[1],
            status_4xx: status[2],
            status_5xx: status[3],
            latency: histogram,
        }
    }

    fn requests(target: &str, name: &str, counts: RequestCounts, latency_sum: Duration) -> TargetRequestTotals {
        TargetRequestTotals { target: target.into(), name: name.into(), counts, latency_sum }
    }

    fn container(kind: ContainerKind, name: &str, instance: Option<&str>, cpu: Option<f64>) -> ContainerMetrics {
        ContainerMetrics {
            kind,
            id: format!("id-{name}"),
            name: name.into(),
            instance: instance.map(Into::into),
            cpu_percent: cpu,
            memory_bytes: cpu.map(|_| 300 << 20),
            memory_limit_bytes: None,
            cpu_limit: None,
        }
    }

    /// A Linux server with every value, two containers, and the requests
    /// of a service, the dashboard and unrouted ones.
    fn full() -> MetricsSnapshot {
        MetricsSnapshot {
            host: Some(HostCounters {
                cpu_count: 4,
                cpu_seconds: vec![("user".into(), 13.0), ("idle".into(), 85.5)],
                memory_total_bytes: Some(8 << 30),
                memory_available_bytes: Some(5 << 30),
                disks: vec![DiskCounters {
                    mount_point: "/".into(),
                    used_bytes: 40 << 30,
                    total_bytes: 100 << 30,
                    read_bytes: Some(1 << 30),
                    written_bytes: Some(2 << 30),
                }],
                network: vec![
                    NetworkCounters { device: "eth0".into(), receive_bytes: 3_000_000, transmit_bytes: 1_000_000 },
                    NetworkCounters { device: "wg0".into(), receive_bytes: 100_000, transmit_bytes: 40_000 },
                ],
            }),
            containers: Some(vec![
                container(ContainerKind::Service, "web", Some("9f8e7d"), Some(120.5)),
                container(ContainerKind::Datastore, "db", None, Some(3.0)),
            ]),
            requests: vec![
                // 2 requests within 1 ms, 1 within 10 ms, 1 longer than 30 s, and a WebSocket.
                requests(
                    "srv-1",
                    "web",
                    counts(&[(0, 2), (3, 1), (LATENCY_BUCKETS - 1, 1)], [4, 0, 0, 1]),
                    Duration::from_millis(31_008),
                ),
                requests("dashboard", "dashboard", counts(&[(1, 1)], [1, 0, 0, 0]), Duration::from_micros(1_500)),
                requests("unrouted", "unrouted", RequestCounts::default(), Duration::ZERO),
            ],
        }
    }

    #[test]
    fn the_text_follows_the_prometheus_format() {
        let text = render(&full(), "1.2.3");
        let f = parse(&text);
        let kinds: Vec<(&str, &str)> = f.iter().map(|(n, fam)| (n.as_str(), fam.kind.as_str())).collect();
        assert_eq!(
            kinds,
            [
                ("ferry_build_info", "gauge"),
                ("ferry_container_cpu_percent", "gauge"),
                ("ferry_container_memory_bytes", "gauge"),
                ("ferry_host_cpu_count", "gauge"),
                ("ferry_host_cpu_seconds_total", "counter"),
                ("ferry_host_disk_read_bytes_total", "counter"),
                ("ferry_host_disk_total_bytes", "gauge"),
                ("ferry_host_disk_used_bytes", "gauge"),
                ("ferry_host_disk_written_bytes_total", "counter"),
                ("ferry_host_memory_available_bytes", "gauge"),
                ("ferry_host_memory_total_bytes", "gauge"),
                ("ferry_host_network_receive_bytes_total", "counter"),
                ("ferry_host_network_transmit_bytes_total", "counter"),
                ("ferry_http_request_duration_seconds", "histogram"),
                ("ferry_http_requests_total", "counter"),
            ]
        );
        assert!(f.values().all(|fam| !fam.help.is_empty()));
        assert_eq!(value(&f, "ferry_build_info", "ferry_build_info", &[("version", "1.2.3")]), Some(1.0));

        // The host's values as they are.
        let host = |family: &str, labels: &[(&str, &str)]| value(&f, family, family, labels);
        assert_eq!(host("ferry_host_cpu_count", &[]), Some(4.0));
        assert_eq!(host("ferry_host_cpu_seconds_total", &[("mode", "idle")]), Some(85.5));
        assert_eq!(host("ferry_host_memory_available_bytes", &[]), Some((5u64 << 30) as f64));
        assert_eq!(host("ferry_host_disk_written_bytes_total", &[("mount_point", "/")]), Some((2u64 << 30) as f64));
        assert_eq!(host("ferry_host_network_transmit_bytes_total", &[("device", "eth0")]), Some(1_000_000.0));
        assert_eq!(host("ferry_host_network_receive_bytes_total", &[("device", "wg0")]), Some(100_000.0));
        // Integers stay integers.
        assert!(text.contains("\nferry_host_memory_total_bytes 8589934592\n"), "{text}");

        // A service instance has an `instance_id`, a datastore does not.
        let web = [("kind", "service"), ("id", "id-web"), ("name", "web"), ("instance_id", "9f8e7d")];
        assert_eq!(value(&f, "ferry_container_cpu_percent", "ferry_container_cpu_percent", &web), Some(120.5));
        let db = [("kind", "datastore"), ("id", "id-db"), ("name", "db")];
        assert_eq!(
            value(&f, "ferry_container_memory_bytes", "ferry_container_memory_bytes", &db),
            Some((300u64 << 20) as f64)
        );

        // Every target has every status class, zeros included.
        let total = |target: &str, name: &str, class: &str| {
            value(
                &f,
                "ferry_http_requests_total",
                "ferry_http_requests_total",
                &[("target", target), ("name", name), ("status_class", class)],
            )
        };
        assert_eq!(total("srv-1", "web", "2xx"), Some(4.0));
        assert_eq!(total("srv-1", "web", "5xx"), Some(1.0));
        assert_eq!(total("unrouted", "unrouted", "3xx"), Some(0.0));
        assert_eq!(f["ferry_http_requests_total"].samples.len(), 3 * 4);

        // The histogram: the buckets of LATENCY_BOUNDS_MS in seconds, cumulative.
        let h = "ferry_http_request_duration_seconds";
        let bucket =
            |le: &str| value(&f, h, &format!("{h}_bucket"), &[("target", "srv-1"), ("name", "web"), ("le", le)]);
        assert_eq!(bucket("0.001"), Some(2.0));
        assert_eq!(bucket("0.005"), Some(2.0));
        assert_eq!(bucket("0.01"), Some(3.0));
        assert_eq!(bucket("0.075"), Some(3.0));
        assert_eq!(bucket("30"), Some(3.0));
        assert_eq!(bucket("+Inf"), Some(4.0), "the WebSocket is not in the latency");
        let web_labels = [("target", "srv-1"), ("name", "web")];
        assert_eq!(value(&f, h, &format!("{h}_count"), &web_labels), Some(4.0));
        assert_eq!(value(&f, h, &format!("{h}_sum"), &web_labels), Some(31.008));
        let les = f[h].samples.iter().filter(|s| s.name.ends_with("_bucket") && s.labels["target"] == "srv-1").count();
        assert_eq!(les, LATENCY_BUCKETS, "one bucket per bound, and +Inf");
    }

    #[test]
    fn label_values_and_help_are_escaped() {
        let odd = "/mnt/a \"quoted\" back\\slash\nnew line";
        let snapshot = MetricsSnapshot {
            host: Some(HostCounters {
                cpu_count: 1,
                disks: vec![DiskCounters {
                    mount_point: odd.into(),
                    used_bytes: 1,
                    total_bytes: 2,
                    read_bytes: None,
                    written_bytes: None,
                }],
                ..HostCounters::default()
            }),
            ..MetricsSnapshot::default()
        };
        let text = render(&snapshot, "0.1.0");
        assert!(
            text.contains(r#"ferry_host_disk_used_bytes{mount_point="/mnt/a \"quoted\" back\\slash\nnew line"} 1"#),
            "{text}"
        );
        let f = parse(&text);
        assert_eq!(f["ferry_host_disk_used_bytes"].samples[0].labels["mount_point"], odd, "the value comes back whole");

        let mut out = Exposition::default();
        out.header("x", Kind::Gauge, "a \\ b\nc");
        assert_eq!(out.text, "# HELP x a \\\\ b\\nc\n# TYPE x gauge\n");
    }

    #[test]
    fn what_cannot_be_read_is_left_out() {
        // Before the first reading, Docker not answering, no request yet.
        let text = render(
            &MetricsSnapshot {
                requests: vec![requests("unrouted", "unrouted", RequestCounts::default(), Duration::ZERO)],
                ..MetricsSnapshot::default()
            },
            "0.1.0",
        );
        let f = parse(&text);
        let names: Vec<&str> = f.keys().map(String::as_str).collect();
        assert_eq!(names, ["ferry_build_info", "ferry_http_request_duration_seconds", "ferry_http_requests_total"]);
        assert_eq!(render(&MetricsSnapshot::default(), "0.1.0").lines().count(), 3, "build_info alone");

        // macOS: the CPU count and the disks' space only; a container Docker did not measure.
        let snapshot = MetricsSnapshot {
            host: Some(HostCounters {
                cpu_count: 8,
                disks: vec![DiskCounters {
                    mount_point: "/".into(),
                    used_bytes: 1,
                    total_bytes: 2,
                    read_bytes: None,
                    written_bytes: None,
                }],
                ..HostCounters::default()
            }),
            containers: Some(vec![container(ContainerKind::Service, "web", Some("9f8e7d"), None)]),
            requests: Vec::new(),
        };
        let text = render(&snapshot, "0.1.0");
        let f = parse(&text);
        let names: Vec<&str> = f.keys().map(String::as_str).collect();
        assert_eq!(
            names,
            ["ferry_build_info", "ferry_host_cpu_count", "ferry_host_disk_total_bytes", "ferry_host_disk_used_bytes"]
        );
        assert!(!text.contains("NaN") && !text.contains("container"), "{text}");
    }
}
