//! Server metrics (`Engine::server_metrics`, DESIGN.md §22): how busy the
//! machine is — CPU, memory and network from `/proc`, the disks from
//! `statvfs` and `/proc/diskstats` — the CPU / memory of every running
//! service instance and datastore, from Docker, and the requests of the
//! proxy's last reading (`requests`).
//!
//! CPU % and the throughputs are rates between two readings of the kernel's
//! counters. [`HostSampler`] keeps the last reading: a call within a minute
//! of it measures since then; any other call reads twice, one second apart
//! (meanwhile Docker samples the containers, which takes as long). Where
//! `/proc` is missing (macOS) those values are `None` and nothing waits.
//!
//! `GET /metrics` (`Engine::metrics_snapshot`) reads nothing: it publishes
//! the counters of the history's last reading ([`LastReading`]) as they are.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Mutex as StdMutex;
use std::time::{Duration, Instant};

use chrono::Utc;
use ferry_core::Result;
use ferry_core::dto::{
    ContainerKind, ContainerMetrics, DiskCounters, DiskMetrics, HostCounters, HostMetrics, MetricsSnapshot,
    NetworkCounters, ServerMetrics,
};
use ferry_core::naming::{LABEL_DATASTORE, LABEL_INSTANCE, LABEL_ROLE, LABEL_SERVICE, ROLE_DATASTORE, ROLE_SERVICE};
use ferry_docker::ContainerInfo;
use futures::future::join_all;
use tracing::debug;

use crate::limits::{DiskSpace, disk_space, local_docker_root};
use crate::requests;
use crate::state::Inner;
use crate::status::{INSPECT_TIMEOUT, STATS_TIMEOUT, cpu_limit, memory_limit};
use crate::util::{instance_id, lock, with_timeout};

/// Where Linux publishes the kernel's counters.
const PROC_DIR: &str = "/proc";
/// Shortest time between the two readings of a rate.
const MIN_GAP: Duration = Duration::from_secs(1);
/// A reading older than this is not used: the rate would be an average over
/// too long a time.
const MAX_AGE: Duration = Duration::from_secs(60);
/// Bound on one reading of the host (`statvfs` can hang on a network
/// filesystem).
const HOST_TIMEOUT: Duration = Duration::from_secs(3);
/// Bound on finding Docker's data directory (the daemon may not answer).
const DOCKER_ROOT_TIMEOUT: Duration = Duration::from_secs(2);
/// Bound on listing the containers: the host's values come back without
/// them when Docker does not answer.
const LIST_TIMEOUT: Duration = Duration::from_secs(3);
/// `/proc/diskstats` counts sectors of 512 bytes, whatever the device's own.
const SECTOR_BYTES: u64 = 512;
/// `/proc/stat` counts the CPU time in ticks of `USER_HZ`, which Linux keeps
/// at 100 per second for programs on every architecture Docker runs on.
const TICKS_PER_SEC: f64 = 100.0;
/// The modes of the `cpu` line of `/proc/stat`, in its order (guest time is
/// counted in user and nice already).
const CPU_MODES: [&str; 8] = ["user", "nice", "system", "idle", "iowait", "irq", "softirq", "steal"];
/// The oldest reading `GET /metrics` publishes: the history reads every
/// 10 s and skips a reading that takes more than 9 s.
const SNAPSHOT_MAX_AGE: Duration = Duration::from_secs(30);

/// `Engine::server_metrics`.
pub(crate) async fn server_metrics(inner: &Inner) -> Result<ServerMetrics> {
    let (host, containers) = tokio::join!(host(inner, &inner.host_sampler), containers(inner, true));
    Ok(ServerMetrics { at: Utc::now(), host, containers: containers?, requests: inner.requests.last() })
}

/// `Engine::metrics_snapshot`: the host and the containers of the history's
/// last reading, when it is recent, and the proxy's counters. Reads neither
/// the machine nor Docker.
pub(crate) async fn snapshot(inner: &Inner) -> Result<MetricsSnapshot> {
    let (host, containers) = inner.last_reading.recent(SNAPSHOT_MAX_AGE);
    let requests = requests::totals(&inner.store, inner.routes.request_stats()).await;
    Ok(MetricsSnapshot { host, containers, requests })
}

/// The host and the containers of the history's last reading (every 10 s),
/// kept for `GET /metrics`, each with the time it was read.
#[derive(Default)]
pub(crate) struct LastReading(StdMutex<Kept>);

#[derive(Default)]
struct Kept {
    host: Option<(Instant, HostCounters)>,
    containers: Option<(Instant, Vec<ContainerMetrics>)>,
}

impl LastReading {
    /// Keep the last reading of `sampler`, and the `containers` read just
    /// now (`None`: Docker did not answer).
    pub(crate) fn keep(&self, sampler: &HostSampler, containers: Option<&[ContainerMetrics]>) {
        let host = lock(&sampler.last).as_ref().map(|r| (r.at, r.totals()));
        *lock(&self.0) = Kept { host, containers: containers.map(|c| (Instant::now(), c.to_vec())) };
    }

    /// The host and the containers, each when read within `max_age`.
    pub(crate) fn recent(&self, max_age: Duration) -> (Option<HostCounters>, Option<Vec<ContainerMetrics>>) {
        let kept = lock(&self.0);
        let recent = |at: &Instant| at.elapsed() <= max_age;
        (
            kept.host.as_ref().filter(|(at, _)| recent(at)).map(|(_, h)| h.clone()),
            kept.containers.as_ref().filter(|(at, _)| recent(at)).map(|(_, c)| c.clone()),
        )
    }
}

/// The host's values, with the rates since the last reading of `sampler`.
pub(crate) async fn host(inner: &Inner, sampler: &HostSampler) -> HostMetrics {
    let mut paths = vec![inner.config.data_dir.clone()];
    if let Ok(Some(root)) =
        tokio::time::timeout(DOCKER_ROOT_TIMEOUT, async { local_docker_root(inner.host_info().await) }).await
    {
        paths.push(root);
    }
    sampler.read(paths).await
}

// ---------------------------------------------------------------------------
// containers

/// What a container runs: a service (its instances) or a datastore.
type Owner = (ContainerKind, String, String);

/// The running containers of services and datastores, the busiest first.
/// `None` when Docker does not answer. Without `with_limits`, the
/// containers are not inspected and their limits are `None`.
pub(crate) async fn containers(inner: &Inner, with_limits: bool) -> Result<Option<Vec<ContainerMetrics>>> {
    let prefix = inner.naming.prefix();
    let listed = with_timeout(
        "listing containers",
        LIST_TIMEOUT,
        inner.docker.list_containers(&[(LABEL_INSTANCE, prefix)], false),
    )
    .await;
    let list = match listed {
        Ok(list) => list,
        Err(e) => {
            debug!("server metrics without the containers: {e}");
            return Ok(None);
        }
    };
    let services: HashMap<String, String> =
        inner.store.list_services().await?.into_iter().map(|s| (s.id, s.name)).collect();
    let datastores: HashMap<String, String> =
        inner.store.list_datastores().await?.into_iter().map(|d| (d.id, d.name)).collect();
    let mut rows = join_all(
        list.iter()
            .filter_map(|c| owner(c, &services, &datastores).map(|o| (c, o)))
            .map(|(c, owner)| container_metrics(inner, c, owner, with_limits)),
    )
    .await;
    sort_busiest_first(&mut rows);
    Ok(Some(rows))
}

/// The service or datastore a container belongs to, when the store knows it
/// (`None` for jobs and builds).
fn owner(c: &ContainerInfo, services: &HashMap<String, String>, datastores: &HashMap<String, String>) -> Option<Owner> {
    let (kind, label, names) = match c.labels.get(LABEL_ROLE).map(String::as_str) {
        Some(ROLE_SERVICE) => (ContainerKind::Service, LABEL_SERVICE, services),
        Some(ROLE_DATASTORE) => (ContainerKind::Datastore, LABEL_DATASTORE, datastores),
        _ => return None,
    };
    let id = c.labels.get(label)?;
    Some((kind, id.clone(), names.get(id)?.clone()))
}

async fn container_metrics(
    inner: &Inner,
    c: &ContainerInfo,
    (kind, id, name): Owner,
    with_limits: bool,
) -> ContainerMetrics {
    // The list API has no limits: inspect (bounded) while Docker samples the
    // CPU (about a second).
    let inspected = async {
        if !with_limits {
            return None;
        }
        match tokio::time::timeout(INSPECT_TIMEOUT, inner.docker.inspect_container(&c.id)).await {
            Ok(Ok(info)) => info,
            _ => None,
        }
    };
    let (inspected, stats) = tokio::join!(inspected, tokio::time::timeout(STATS_TIMEOUT, inner.docker.stats(&c.id)));
    let info = inspected.as_ref().unwrap_or(c);
    let stats = match stats {
        Ok(Ok(s)) => Some(s),
        _ => None,
    };
    ContainerMetrics {
        kind,
        id,
        name,
        instance: (kind == ContainerKind::Service).then(|| instance_id(&c.name)),
        cpu_percent: stats.map(|s| s.cpu_percent),
        memory_bytes: stats.map(|s| s.memory_bytes),
        memory_limit_bytes: memory_limit(info),
        cpu_limit: cpu_limit(info),
    }
}

/// The busiest (CPU) first, then the ones Docker did not measure; by name
/// and instance among equals.
fn sort_busiest_first(rows: &mut [ContainerMetrics]) {
    let cpu = |r: &ContainerMetrics| r.cpu_percent.unwrap_or(f64::NEG_INFINITY);
    rows.sort_by(|a, b| {
        cpu(b).total_cmp(&cpu(a)).then_with(|| a.name.cmp(&b.name)).then_with(|| a.instance.cmp(&b.instance))
    });
}

// ---------------------------------------------------------------------------
// the host

/// Reads the host, and keeps the last reading for the rates of the next one.
pub(crate) struct HostSampler {
    proc_dir: PathBuf,
    last: StdMutex<Option<Reading>>,
}

impl HostSampler {
    pub fn new() -> Self {
        Self::with_proc_dir(PROC_DIR)
    }

    fn with_proc_dir(proc_dir: impl Into<PathBuf>) -> Self {
        HostSampler { proc_dir: proc_dir.into(), last: StdMutex::new(None) }
    }

    /// The host's values; `paths` are the directories whose filesystems to
    /// report (the first one first; one filesystem once).
    pub async fn read(&self, paths: Vec<PathBuf>) -> HostMetrics {
        let previous =
            lock(&self.last).as_ref().filter(|r| r.at.elapsed() <= MAX_AGE).map(|r| (r.at, r.counters.clone()));
        let (since, before) = match previous {
            Some(previous) => previous,
            None => match self.read_now(paths.clone()).await {
                Some(r) => (r.at, r.counters),
                None => return HostMetrics { cpu_count: available_cpus(), ..HostMetrics::default() },
            },
        };
        // Without counters (macOS) there is no rate to wait for.
        if !before.is_empty() {
            tokio::time::sleep(MIN_GAP.saturating_sub(since.elapsed())).await;
        }
        let Some(now) = self.read_now(paths).await else {
            return HostMetrics { cpu_count: available_cpus(), ..HostMetrics::default() };
        };
        {
            let mut last = lock(&self.last);
            if last.as_ref().is_none_or(|r| r.at < now.at) {
                *last = Some(now.clone());
            }
        }
        now.metrics(&before, now.at.saturating_duration_since(since))
    }

    async fn read_now(&self, paths: Vec<PathBuf>) -> Option<Reading> {
        let proc_dir = self.proc_dir.clone();
        let reading = tokio::task::spawn_blocking(move || read_host(&proc_dir, &paths));
        match tokio::time::timeout(HOST_TIMEOUT, reading).await {
            Ok(Ok(reading)) => Some(reading),
            Ok(Err(e)) => {
                debug!("reading the host's metrics failed: {e}");
                None
            }
            Err(_) => {
                debug!("reading the host's metrics timed out after {}s", HOST_TIMEOUT.as_secs());
                None
            }
        }
    }
}

/// The kernel's counters at one moment.
#[derive(Debug, Clone, Default, PartialEq)]
struct Counters {
    cpu: Option<CpuTimes>,
    /// Bytes read and written per block device, by `(major, minor)`.
    disks: HashMap<(u32, u32), DiskIo>,
    /// Bytes received and sent over the counted interfaces, all together.
    net: Option<NetIo>,
    /// The same, per interface (`GET /metrics`: a sum would go down when
    /// an interface goes away, and Prometheus would read a reset).
    interfaces: Vec<(String, NetIo)>,
}

impl Counters {
    fn is_empty(&self) -> bool {
        self.cpu.is_none() && self.disks.is_empty() && self.net.is_none()
    }
}

/// Time (in ticks) all the CPUs spent in each mode of [`CPU_MODES`]: the
/// first `modes` of them, the ones the kernel counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CpuTimes {
    ticks: [u64; CPU_MODES.len()],
    modes: usize,
}

impl CpuTimes {
    fn total(&self) -> u64 {
        self.ticks.iter().sum()
    }

    /// Waiting for I/O counts as idle.
    fn idle(&self) -> u64 {
        self.ticks[3] + self.ticks[4]
    }

    /// The seconds of each mode the kernel counts.
    fn seconds(&self) -> Vec<(String, f64)> {
        CPU_MODES
            .iter()
            .zip(self.ticks)
            .take(self.modes)
            .map(|(m, t)| (m.to_string(), t as f64 / TICKS_PER_SEC))
            .collect()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct DiskIo {
    read: u64,
    written: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
struct NetIo {
    rx: u64,
    tx: u64,
}

/// One filesystem to report.
#[derive(Debug, Clone, PartialEq)]
struct Disk {
    mount_point: PathBuf,
    /// Its block device, `(major, minor)`.
    device: (u32, u32),
    space: DiskSpace,
}

/// One reading of the host.
#[derive(Debug, Clone)]
struct Reading {
    at: Instant,
    counters: Counters,
    cpu_count: Option<u32>,
    memory_used: Option<u64>,
    memory_total: Option<u64>,
    disks: Vec<Disk>,
}

impl Reading {
    /// The values, with the rates since the counters `before`, read
    /// `elapsed` earlier.
    fn metrics(&self, before: &Counters, elapsed: Duration) -> HostMetrics {
        let secs = elapsed.as_secs_f64();
        let now = &self.counters;
        let disks = self
            .disks
            .iter()
            .map(|d| {
                let io = before.disks.get(&d.device).zip(now.disks.get(&d.device));
                DiskMetrics {
                    mount_point: d.mount_point.display().to_string(),
                    used_bytes: d.space.used_bytes(),
                    total_bytes: d.space.total_bytes,
                    read_bytes_per_sec: io.and_then(|(b, a)| rate(b.read, a.read, secs)),
                    write_bytes_per_sec: io.and_then(|(b, a)| rate(b.written, a.written, secs)),
                }
            })
            .collect();
        let net = before.net.zip(now.net);
        HostMetrics {
            cpu_percent: before.cpu.zip(now.cpu).and_then(|(b, a)| cpu_percent(b, a)),
            cpu_count: self.cpu_count.unwrap_or_else(available_cpus),
            memory_used_bytes: self.memory_used,
            memory_total_bytes: self.memory_total,
            disks,
            network_rx_bytes_per_sec: net.and_then(|(b, a)| rate(b.rx, a.rx, secs)),
            network_tx_bytes_per_sec: net.and_then(|(b, a)| rate(b.tx, a.tx, secs)),
        }
    }

    /// The values, with the kernel's counters as they are (`GET /metrics`).
    fn totals(&self) -> HostCounters {
        let now = &self.counters;
        let disks = self
            .disks
            .iter()
            .map(|d| {
                let io = now.disks.get(&d.device);
                DiskCounters {
                    mount_point: d.mount_point.display().to_string(),
                    used_bytes: d.space.used_bytes(),
                    total_bytes: d.space.total_bytes,
                    read_bytes: io.map(|io| io.read),
                    written_bytes: io.map(|io| io.written),
                }
            })
            .collect();
        HostCounters {
            cpu_count: self.cpu_count.unwrap_or_else(available_cpus),
            cpu_seconds: now.cpu.map(|c| c.seconds()).unwrap_or_default(),
            memory_total_bytes: self.memory_total,
            memory_available_bytes: self.memory_total.zip(self.memory_used).map(|(t, u)| t.saturating_sub(u)),
            disks,
            network: now
                .interfaces
                .iter()
                .map(|(device, io)| NetworkCounters {
                    device: device.clone(),
                    receive_bytes: io.rx,
                    transmit_bytes: io.tx,
                })
                .collect(),
        }
    }
}

/// Read the host's counters and sizes (blocking). What can't be read is
/// left out, without an error.
fn read_host(proc_dir: &Path, paths: &[PathBuf]) -> Reading {
    let read = |name: &str| std::fs::read_to_string(proc_dir.join(name)).ok();
    let at = Instant::now();
    let stat = read("stat");
    let (memory_used, memory_total) = read("meminfo").map(|m| parse_meminfo(&m)).unwrap_or_default();
    let interfaces = read("net/dev").as_deref().and_then(parse_net_devices);
    Reading {
        at,
        counters: Counters {
            cpu: stat.as_deref().and_then(parse_cpu_times),
            disks: read("diskstats").map(|d| parse_diskstats(&d)).unwrap_or_default(),
            net: interfaces.as_deref().map(sum_net),
            interfaces: interfaces.unwrap_or_default(),
        },
        cpu_count: stat.as_deref().and_then(parse_cpu_count),
        memory_used,
        memory_total,
        disks: read_disks(paths),
    }
}

/// The filesystem of each path, once each.
fn read_disks(paths: &[PathBuf]) -> Vec<Disk> {
    let mut seen = HashSet::new();
    let mut disks = Vec::new();
    for path in paths {
        let (mount_point, dev) = match mount_point(path) {
            Ok(found) => found,
            Err(e) => {
                debug!(path = %path.display(), "cannot find the filesystem: {e}");
                continue;
            }
        };
        if !seen.insert(dev) {
            continue;
        }
        match disk_space(&mount_point) {
            Ok(space) => disks.push(Disk { mount_point, device: major_minor(dev), space }),
            Err(e) => debug!(path = %mount_point.display(), "cannot read the size of the filesystem: {e}"),
        }
    }
    disks
}

/// Where the filesystem of `path` is mounted (its highest ancestor on the
/// same device), and that device.
#[cfg(unix)]
fn mount_point(path: &Path) -> std::io::Result<(PathBuf, u64)> {
    use std::os::unix::fs::MetadataExt;
    let path = std::fs::canonicalize(path)?;
    let dev = std::fs::metadata(&path)?.dev();
    let mut mount = path.as_path();
    while let Some(parent) = mount.parent() {
        match std::fs::metadata(parent) {
            Ok(m) if m.dev() == dev => mount = parent,
            _ => break,
        }
    }
    Ok((mount.to_path_buf(), dev))
}

#[cfg(not(unix))]
fn mount_point(_path: &Path) -> std::io::Result<(PathBuf, u64)> {
    Err(std::io::Error::new(std::io::ErrorKind::Unsupported, "disks are only read on Unix"))
}

/// `(major, minor)` of a Linux device number (glibc's encoding of `st_dev`).
fn major_minor(dev: u64) -> (u32, u32) {
    let major = ((dev >> 8) & 0xfff) | ((dev >> 32) & 0xffff_f000);
    let minor = (dev & 0xff) | ((dev >> 12) & 0xffff_ff00);
    (major as u32, minor as u32)
}

/// CPUs of this machine when `/proc/stat` can't tell.
fn available_cpus() -> u32 {
    std::thread::available_parallelism().map_or(1, |n| u32::try_from(n.get()).unwrap_or(u32::MAX))
}

/// The `cpu` line of `/proc/stat`: the ticks of [`CPU_MODES`], at least
/// user nice system idle (an old kernel has no more).
fn parse_cpu_times(stat: &str) -> Option<CpuTimes> {
    let line = stat.lines().find(|l| l.starts_with("cpu "))?;
    let fields: Vec<u64> = line.split_whitespace().skip(1).map(|f| f.parse().ok()).collect::<Option<_>>()?;
    if fields.len() < 4 {
        return None;
    }
    let mut ticks = [0; CPU_MODES.len()];
    let modes = fields.len().min(ticks.len());
    ticks[..modes].copy_from_slice(&fields[..modes]);
    Some(CpuTimes { ticks, modes })
}

/// Number of `cpuN` lines of `/proc/stat` (the CPUs online).
fn parse_cpu_count(stat: &str) -> Option<u32> {
    let count = stat
        .lines()
        .filter(|l| l.strip_prefix("cpu").and_then(|rest| rest.chars().next()).is_some_and(|c| c.is_ascii_digit()))
        .count();
    u32::try_from(count).ok().filter(|n| *n > 0)
}

/// Busy time between two readings, in percent of all the CPUs.
fn cpu_percent(before: CpuTimes, after: CpuTimes) -> Option<f64> {
    let total = after.total().checked_sub(before.total())?;
    let idle = after.idle().checked_sub(before.idle())?;
    if total == 0 {
        return None;
    }
    let busy = total.saturating_sub(idle);
    Some((busy as f64 / total as f64 * 100.0).clamp(0.0, 100.0))
}

/// `(used, total)` memory in bytes, from `/proc/meminfo`. Used leaves out
/// the page cache, like `free`: `MemTotal - MemAvailable`, or before Linux
/// 3.14 `MemTotal - MemFree - Buffers - Cached - SReclaimable`.
fn parse_meminfo(meminfo: &str) -> (Option<u64>, Option<u64>) {
    let mut values: HashMap<&str, u64> = HashMap::new();
    for line in meminfo.lines() {
        let Some((key, rest)) = line.split_once(':') else { continue };
        let mut parts = rest.split_whitespace();
        let Some(n) = parts.next().and_then(|v| v.parse::<u64>().ok()) else { continue };
        let bytes = match parts.next() {
            Some("kB") => n.saturating_mul(1024),
            None => n,
            Some(_) => continue,
        };
        values.insert(key.trim(), bytes);
    }
    let Some(total) = values.get("MemTotal").copied().filter(|t| *t > 0) else {
        return (None, None);
    };
    let used = match values.get("MemAvailable") {
        Some(available) => Some(total.saturating_sub(*available)),
        None => values.get("MemFree").map(|free| {
            let cache: u64 = ["Buffers", "Cached", "SReclaimable"].iter().filter_map(|k| values.get(k)).sum();
            total.saturating_sub(free.saturating_add(cache))
        }),
    };
    (used, Some(total))
}

/// Docker's own interfaces and the loopback: what goes through them is
/// counted again on the real ones (or never leaves the machine).
fn counted_interface(name: &str) -> bool {
    !(name == "lo" || name.starts_with("docker") || name.starts_with("br-") || name.starts_with("veth"))
}

/// Bytes received and sent by each counted interface of `/proc/net/dev`,
/// in its order. `None` when it has no interface at all.
fn parse_net_devices(net_dev: &str) -> Option<Vec<(String, NetIo)>> {
    let mut interfaces = Vec::new();
    let mut found = false;
    for line in net_dev.lines() {
        let Some((name, rest)) = line.split_once(':') else { continue };
        let fields: Vec<u64> = rest.split_whitespace().filter_map(|f| f.parse().ok()).collect();
        // Receive: bytes packets errs drop fifo frame compressed multicast,
        // then transmit: bytes ...
        if fields.len() < 9 {
            continue;
        }
        found = true;
        let name = name.trim();
        if counted_interface(name) {
            interfaces.push((name.to_string(), NetIo { rx: fields[0], tx: fields[8] }));
        }
    }
    found.then_some(interfaces)
}

/// The bytes of all the interfaces together.
fn sum_net(interfaces: &[(String, NetIo)]) -> NetIo {
    interfaces.iter().fold(NetIo::default(), |total, (_, io)| NetIo {
        rx: total.rx.saturating_add(io.rx),
        tx: total.tx.saturating_add(io.tx),
    })
}

/// Bytes received and sent over the counted interfaces of `/proc/net/dev`.
#[cfg(test)]
fn parse_net_dev(net_dev: &str) -> Option<NetIo> {
    parse_net_devices(net_dev).as_deref().map(sum_net)
}

/// Bytes read and written per device, from `/proc/diskstats`: major minor
/// name, reads completed, merged, sectors read, time, writes completed,
/// merged, sectors written, ...
fn parse_diskstats(diskstats: &str) -> HashMap<(u32, u32), DiskIo> {
    diskstats
        .lines()
        .filter_map(|line| {
            let f: Vec<&str> = line.split_whitespace().collect();
            if f.len() < 10 {
                return None;
            }
            let num = |i: usize| f[i].parse::<u64>().ok();
            let device = (f[0].parse().ok()?, f[1].parse().ok()?);
            let io =
                DiskIo { read: num(5)?.saturating_mul(SECTOR_BYTES), written: num(9)?.saturating_mul(SECTOR_BYTES) };
            Some((device, io))
        })
        .collect()
}

/// Per second, between two readings of a counter. `None` when it went back
/// (an interface went away, a counter wrapped).
fn rate(before: u64, after: u64, secs: f64) -> Option<u64> {
    if after < before || secs <= 0.0 {
        return None;
    }
    Some(((after - before) as f64 / secs).round() as u64)
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use ferry_docker::ContainerState;

    use super::*;

    // Two readings of a 4-CPU machine, 2 seconds apart.
    const STAT_1: &str = "\
cpu  1000 50 500 8000 200 10 20 30 5 0
cpu0 250 10 125 2000 50 3 5 8 1 0
cpu1 250 10 125 2000 50 2 5 7 1 0
cpu2 250 15 125 2000 50 2 5 7 1 0
cpu3 250 15 125 2000 50 3 5 8 2 0
intr 123456 0 0
ctxt 987654
btime 1760000000
processes 4242
procs_running 2
";
    // +300 user, +100 system, +550 idle, +50 iowait: 1000 ticks, 400 busy.
    const STAT_2: &str = "\
cpu  1300 50 600 8550 250 10 20 30 5 0
cpu0 325 10 150 2137 62 3 5 8 1 0
cpu1 325 10 150 2137 63 2 5 7 1 0
cpu2 325 15 150 2138 62 2 5 7 1 0
cpu3 325 15 150 2138 63 3 5 8 2 0
";

    const MEMINFO: &str = "\
MemTotal:        8000000 kB
MemFree:          500000 kB
MemAvailable:    5000000 kB
Buffers:          200000 kB
Cached:          4000000 kB
SwapCached:            0 kB
SReclaimable:     300000 kB
HugePages_Total:       0
";

    const NET_DEV_1: &str = "\
Inter-|   Receive                                                |  Transmit
 face |bytes    packets errs drop fifo frame compressed multicast|bytes    packets errs drop fifo colls carrier compressed
    lo: 9000000    100    0    0    0     0          0         0  9000000     100    0    0    0     0       0          0
  eth0: 1000000   2000    0    0    0     0          0         0   400000    1500    0    0    0     0       0          0
 wlan0:   50000    100    0    0    0     0          0         0    20000     100    0    0    0     0       0          0
docker0: 7000000   500    0    0    0     0          0         0  7000000     500    0    0    0     0       0          0
br-1a2b3c4d5e6f: 6000000 400 0  0    0     0          0         0  6000000     400    0    0    0     0       0          0
veth12ab34c: 5000000   300    0    0    0     0          0         0  5000000     300    0    0    0     0       0          0
";
    // eth0 +2 MB in / +600 kB out, wlan0 +50 kB / +20 kB; Docker's and lo
    // move a lot too, and must not count.
    const NET_DEV_2: &str = "\
Inter-|   Receive                                                |  Transmit
 face |bytes    packets errs drop fifo frame compressed multicast|bytes    packets errs drop fifo colls carrier compressed
    lo: 99000000   100    0    0    0     0          0         0 99000000     100    0    0    0     0       0          0
  eth0: 3000000   2000    0    0    0     0          0         0  1000000    1500    0    0    0     0       0          0
 wlan0:  100000    100    0    0    0     0          0         0    40000     100    0    0    0     0       0          0
docker0: 97000000  500    0    0    0     0          0         0 97000000     500    0    0    0     0       0          0
br-1a2b3c4d5e6f: 96000000 400 0 0    0     0          0         0 96000000     400    0    0    0     0       0          0
veth12ab34c: 95000000  300    0    0    0     0          0         0 95000000     300    0    0    0     0       0          0
";

    fn diskstats(major: u32, minor: u32, sectors_read: u64, sectors_written: u64) -> String {
        format!(
            "   7       0 loop0 10 0 999999 0 0 0 0 0 0 0 0 0 0 0 0 0 0\n \
             {major} {minor} sda1 500 20 {sectors_read} 300 400 10 {sectors_written} 900 0 1200 1200 0 0 0 0 0 0\n"
        )
    }

    #[test]
    fn cpu_percent_from_two_readings() {
        let before = parse_cpu_times(STAT_1).unwrap();
        let after = parse_cpu_times(STAT_2).unwrap();
        // user nice system idle iowait irq softirq steal; iowait is idle.
        assert_eq!((before.total(), before.idle()), (1000 + 50 + 500 + 8000 + 200 + 10 + 20 + 30, 8200));
        assert_eq!(cpu_percent(before, after), Some(40.0));
        assert_eq!(parse_cpu_count(STAT_1), Some(4));
        // No time passed, or a counter went back (a new boot): no value.
        assert_eq!(cpu_percent(before, before), None);
        assert_eq!(cpu_percent(after, before), None);
        // An old kernel with only 4 columns; garbage.
        let old = parse_cpu_times("cpu 1 2 3 4\n").unwrap();
        assert_eq!((old.total(), old.idle()), (10, 4));
        assert_eq!(parse_cpu_times("cpu 1 2\n"), None);
        assert_eq!(parse_cpu_times("cpu a b c d\n"), None);
        assert_eq!(parse_cpu_times(""), None);
        assert_eq!(parse_cpu_count("cpu 1 2 3 4\n"), None);
    }

    #[test]
    fn cpu_seconds_by_mode() {
        // 100 ticks a second; the 9th and 10th columns (guest) are in user and nice already.
        let seconds = parse_cpu_times(STAT_1).unwrap().seconds();
        let expected = [
            ("user", 10.0),
            ("nice", 0.5),
            ("system", 5.0),
            ("idle", 80.0),
            ("iowait", 2.0),
            ("irq", 0.1),
            ("softirq", 0.2),
            ("steal", 0.3),
        ];
        assert_eq!(seconds, expected.map(|(m, s)| (m.to_string(), s)));
        // An old kernel: only the modes it counts.
        let modes: Vec<String> =
            parse_cpu_times("cpu 1 2 3 4\n").unwrap().seconds().into_iter().map(|(m, _)| m).collect();
        assert_eq!(modes, ["user", "nice", "system", "idle"]);
    }

    #[test]
    fn memory_used_leaves_out_the_cache() {
        // MemTotal - MemAvailable, not MemTotal - MemFree.
        assert_eq!(parse_meminfo(MEMINFO), (Some(3_000_000 * 1024), Some(8_000_000 * 1024)));
        // Before Linux 3.14: no MemAvailable.
        let old = MEMINFO.lines().filter(|l| !l.starts_with("MemAvailable")).collect::<Vec<_>>().join("\n");
        let cached = 500_000 + 200_000 + 4_000_000 + 300_000;
        assert_eq!(parse_meminfo(&old), (Some((8_000_000 - cached) * 1024), Some(8_000_000 * 1024)));
        assert_eq!(parse_meminfo("MemFree: 10 kB\n"), (None, None));
        assert_eq!(parse_meminfo(""), (None, None));
    }

    #[test]
    fn network_leaves_out_the_loopback_and_dockers_interfaces() {
        assert_eq!(parse_net_dev(NET_DEV_1), Some(NetIo { rx: 1_050_000, tx: 420_000 }));
        assert_eq!(parse_net_dev(NET_DEV_2), Some(NetIo { rx: 3_100_000, tx: 1_040_000 }));
        for name in ["lo", "docker0", "docker_gwbridge", "br-1a2b3c4d5e6f", "veth12ab34c"] {
            assert!(!counted_interface(name), "{name}");
        }
        for name in ["eth0", "ens3", "enp0s31f6", "wlan0", "br0", "bond0"] {
            assert!(counted_interface(name), "{name}");
        }
        assert_eq!(parse_net_dev(""), None);
        let interfaces = parse_net_devices(NET_DEV_2).unwrap();
        assert_eq!(
            interfaces,
            [
                ("eth0".to_string(), NetIo { rx: 3_000_000, tx: 1_000_000 }),
                ("wlan0".to_string(), NetIo { rx: 100_000, tx: 40_000 })
            ],
            "each counted interface, for GET /metrics"
        );
    }

    #[test]
    fn disk_rates_from_two_readings() {
        let before = parse_diskstats(&diskstats(8, 1, 1000, 2000));
        let after = parse_diskstats(&diskstats(8, 1, 1000 + 4096, 2000 + 2048));
        assert_eq!(before[&(8, 1)], DiskIo { read: 1000 * 512, written: 2000 * 512 });
        assert_eq!(before[&(7, 0)], DiskIo { read: 999_999 * 512, written: 0 });
        let (b, a) = (before[&(8, 1)], after[&(8, 1)]);
        // 2 seconds: 4096 sectors read = 1 MiB/s, 2048 written = 512 KiB/s.
        assert_eq!(rate(b.read, a.read, 2.0), Some(1 << 20));
        assert_eq!(rate(b.written, a.written, 2.0), Some(512 << 10));
        assert_eq!(rate(a.read, b.read, 2.0), None, "a counter that went back");
        assert_eq!(rate(1, 2, 0.0), None);
        assert!(parse_diskstats("8 1 sda1 1 2\n").is_empty());
    }

    #[test]
    fn device_numbers_like_glibc() {
        let makedev = |major: u64, minor: u64| {
            ((major & 0xfff) << 8) | ((major & 0xffff_f000) << 32) | (minor & 0xff) | ((minor & 0xffff_ff00) << 12)
        };
        for (major, minor) in [(8, 1), (259, 2), (253, 0), (0, 42), (4095, 255), (5000, 70000)] {
            assert_eq!(major_minor(makedev(major, minor)), (major as u32, minor as u32));
        }
        assert_eq!(major_minor(2049), (8, 1));
    }

    /// A `/proc` with the given files.
    fn proc_dir(stat: &str, meminfo: &str, net_dev: &str, disk: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("net")).unwrap();
        std::fs::write(dir.path().join("stat"), stat).unwrap();
        std::fs::write(dir.path().join("meminfo"), meminfo).unwrap();
        std::fs::write(dir.path().join("net/dev"), net_dev).unwrap();
        std::fs::write(dir.path().join("diskstats"), disk).unwrap();
        dir
    }

    #[test]
    fn a_reading_of_proc_gives_every_host_value() {
        let data = tempfile::tempdir().unwrap();
        let (mount, dev) = mount_point(data.path()).unwrap();
        let (major, minor) = major_minor(dev);
        let first = proc_dir(STAT_1, MEMINFO, NET_DEV_1, &diskstats(major, minor, 1000, 2000));
        let second = proc_dir(STAT_2, MEMINFO, NET_DEV_2, &diskstats(major, minor, 1000 + 4096, 2000 + 2048));
        let paths = vec![data.path().to_path_buf()];
        let before = read_host(first.path(), &paths);
        let after = read_host(second.path(), &paths);
        let m = after.metrics(&before.counters, Duration::from_secs(2));
        assert_eq!(m.cpu_percent, Some(40.0));
        assert_eq!(m.cpu_count, 4);
        assert_eq!((m.memory_used_bytes, m.memory_total_bytes), (Some(3_000_000 * 1024), Some(8_000_000 * 1024)));
        assert_eq!((m.network_rx_bytes_per_sec, m.network_tx_bytes_per_sec), (Some(1_025_000), Some(310_000)));
        assert_eq!(m.disks.len(), 1, "{:?}", m.disks);
        let disk = &m.disks[0];
        assert_eq!(disk.mount_point, mount.display().to_string());
        assert!(data.path().canonicalize().unwrap().starts_with(&mount));
        assert!(disk.total_bytes > 0 && disk.used_bytes <= disk.total_bytes, "{disk:?}");
        assert_eq!((disk.read_bytes_per_sec, disk.write_bytes_per_sec), (Some(1 << 20), Some(512 << 10)));

        // The counters as they are, for `GET /metrics`.
        let t = after.totals();
        assert_eq!(t.cpu_count, 4);
        assert_eq!(t.cpu_seconds.iter().find(|(m, _)| m == "user").map(|(_, s)| *s), Some(13.0));
        assert_eq!((t.memory_total_bytes, t.memory_available_bytes), (Some(8_000_000 * 1024), Some(5_000_000 * 1024)));
        let network: Vec<_> =
            t.network.iter().map(|n| (n.device.as_str(), n.receive_bytes, n.transmit_bytes)).collect();
        assert_eq!(network, [("eth0", 3_000_000, 1_000_000), ("wlan0", 100_000, 40_000)], "per interface, never a sum");
        assert_eq!(t.disks.len(), 1);
        let disk = &t.disks[0];
        assert_eq!(
            (disk.mount_point.as_str(), disk.total_bytes),
            (m.disks[0].mount_point.as_str(), m.disks[0].total_bytes)
        );
        assert_eq!((disk.read_bytes, disk.written_bytes), (Some((1000 + 4096) * 512), Some((2000 + 2048) * 512)));
    }

    #[test]
    fn without_proc_only_the_cpu_count_and_the_disks_space_are_known() {
        // macOS: no /proc.
        let data = tempfile::tempdir().unwrap();
        let missing = data.path().join("no-proc");
        let paths = vec![data.path().to_path_buf(), data.path().join("same-filesystem"), data.path().join("nope")];
        std::fs::create_dir(&paths[1]).unwrap();
        let reading = read_host(&missing, &paths);
        assert!(reading.counters.is_empty());
        let m = reading.metrics(&reading.counters, Duration::from_secs(1));
        assert_eq!(m.cpu_percent, None);
        assert_eq!(m.cpu_count, available_cpus());
        assert_eq!((m.memory_used_bytes, m.memory_total_bytes), (None, None));
        assert_eq!((m.network_rx_bytes_per_sec, m.network_tx_bytes_per_sec), (None, None));
        // One filesystem once; a missing directory is left out.
        assert_eq!(m.disks.len(), 1, "{:?}", m.disks);
        assert_eq!((m.disks[0].read_bytes_per_sec, m.disks[0].write_bytes_per_sec), (None, None));
        // Nothing to publish but the CPU count and the disks' space.
        let t = reading.totals();
        assert!(t.cpu_seconds.is_empty());
        assert_eq!((t.memory_total_bytes, t.memory_available_bytes), (None, None));
        assert!(t.network.is_empty());
        assert_eq!(t.disks.len(), 1);
        assert_eq!((t.disks[0].read_bytes, t.disks[0].written_bytes), (None, None));
    }

    #[tokio::test]
    async fn the_sampler_measures_since_its_last_reading() {
        let data = tempfile::tempdir().unwrap();
        let proc = proc_dir(STAT_2, MEMINFO, NET_DEV_2, "");
        let sampler = HostSampler::with_proc_dir(proc.path());
        let before = parse_cpu_times(STAT_1).unwrap();
        let two_secs_ago = Instant::now().checked_sub(Duration::from_secs(2)).unwrap();
        *lock(&sampler.last) = Some(Reading {
            at: two_secs_ago,
            counters: Counters { cpu: Some(before), net: parse_net_dev(NET_DEV_1), ..Counters::default() },
            cpu_count: None,
            memory_used: None,
            memory_total: None,
            disks: Vec::new(),
        });
        let started = Instant::now();
        let m = sampler.read(vec![data.path().to_path_buf()]).await;
        assert!(started.elapsed() < MIN_GAP, "no second reading to wait for");
        assert_eq!(m.cpu_percent, Some(40.0));
        let rx = m.network_rx_bytes_per_sec.unwrap();
        assert!((1_000_000..=1_025_000).contains(&rx), "about 2 MB in 2 s: {rx}");
        // The new reading is kept for the next call.
        let kept = lock(&sampler.last).clone().unwrap();
        assert!(kept.at > two_secs_ago);
        assert_eq!(kept.counters.cpu, parse_cpu_times(STAT_2));
    }

    #[tokio::test]
    async fn get_metrics_publishes_the_last_reading_only_while_it_is_recent() {
        let data = tempfile::tempdir().unwrap();
        let proc = proc_dir(STAT_2, MEMINFO, NET_DEV_2, "");
        let sampler = HostSampler::with_proc_dir(proc.path());
        let last = LastReading::default();
        assert_eq!(last.recent(SNAPSHOT_MAX_AGE).0, None, "before the first reading");

        sampler.read(vec![data.path().to_path_buf()]).await;
        let web = ContainerMetrics {
            kind: ContainerKind::Service,
            id: "srv-1".into(),
            name: "web".into(),
            instance: Some("9f8e7d".into()),
            cpu_percent: Some(12.5),
            memory_bytes: Some(100 << 20),
            memory_limit_bytes: None,
            cpu_limit: None,
        };
        last.keep(&sampler, Some(std::slice::from_ref(&web)));
        let (host, containers) = last.recent(SNAPSHOT_MAX_AGE);
        let host = host.unwrap();
        assert_eq!((host.cpu_count, host.network.len()), (4, 2));
        assert_eq!(containers.unwrap()[0].cpu_percent, Some(12.5));

        // Docker did not answer this time: no containers rather than old ones.
        last.keep(&sampler, None);
        assert!(last.recent(SNAPSHOT_MAX_AGE).1.is_none());
        // A reading older than the bound is not published.
        last.keep(&sampler, Some(std::slice::from_ref(&web)));
        tokio::time::sleep(Duration::from_millis(5)).await;
        let (host, containers) = last.recent(Duration::from_millis(1));
        assert!(host.is_none() && containers.is_none());
    }

    #[tokio::test]
    async fn the_sampler_does_not_wait_without_counters() {
        let data = tempfile::tempdir().unwrap();
        let sampler = HostSampler::with_proc_dir(data.path().join("no-proc"));
        let started = Instant::now();
        let m = sampler.read(vec![data.path().to_path_buf()]).await;
        assert!(started.elapsed() < MIN_GAP);
        assert_eq!(m.cpu_percent, None);
        assert_eq!(m.disks.len(), 1);
    }

    fn container(name: &str, labels: &[(&str, &str)]) -> ContainerInfo {
        ContainerInfo {
            id: format!("id-{name}"),
            name: name.into(),
            image: "i".into(),
            state: ContainerState::Running,
            exit_code: None,
            host_port: None,
            labels: labels.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect::<BTreeMap<_, _>>(),
            started_at: None,
            restart_count: None,
            oom_killed: false,
            memory_limit_bytes: None,
            nano_cpus: None,
        }
    }

    #[test]
    fn containers_belong_to_known_services_and_datastores() {
        let services = HashMap::from([("srv-1".to_string(), "web".to_string())]);
        let datastores = HashMap::from([("ds-1".to_string(), "db".to_string())]);
        let svc = container("ferry-web-1a2b3c4d-9f8e7d", &[(LABEL_ROLE, ROLE_SERVICE), (LABEL_SERVICE, "srv-1")]);
        assert_eq!(owner(&svc, &services, &datastores), Some((ContainerKind::Service, "srv-1".into(), "web".into())));
        let ds = container("ferry-ds-db", &[(LABEL_ROLE, ROLE_DATASTORE), (LABEL_DATASTORE, "ds-1")]);
        assert_eq!(owner(&ds, &services, &datastores), Some((ContainerKind::Datastore, "ds-1".into(), "db".into())));
        // Jobs, unknown owners and unlabeled containers are not listed.
        let job = container("ferry-job-web-1a2b3c4d", &[(LABEL_ROLE, "job"), (LABEL_SERVICE, "srv-1")]);
        let gone = container("ferry-old-1a2b3c4d-000000", &[(LABEL_ROLE, ROLE_SERVICE), (LABEL_SERVICE, "srv-9")]);
        for c in [job, gone, container("x", &[])] {
            assert_eq!(owner(&c, &services, &datastores), None, "{}", c.name);
        }
    }

    #[test]
    fn the_busiest_come_first() {
        let row = |name: &str, instance: Option<&str>, cpu: Option<f64>| ContainerMetrics {
            kind: if instance.is_some() { ContainerKind::Service } else { ContainerKind::Datastore },
            id: name.into(),
            name: name.into(),
            instance: instance.map(Into::into),
            cpu_percent: cpu,
            memory_bytes: None,
            memory_limit_bytes: None,
            cpu_limit: None,
        };
        let mut rows = vec![
            row("web", Some("bbbbbb"), Some(2.0)),
            row("db", None, None),
            row("api", Some("aaaaaa"), Some(150.0)),
            row("web", Some("aaaaaa"), Some(2.0)),
            row("cache", None, Some(0.0)),
        ];
        sort_busiest_first(&mut rows);
        let order: Vec<_> =
            rows.iter().map(|r| format!("{}/{}", r.name, r.instance.as_deref().unwrap_or("-"))).collect();
        assert_eq!(order, ["api/aaaaaa", "web/aaaaaa", "web/bbbbbb", "cache/-", "db/-"]);
    }
}
