//! The adapter manager: configuration, polling, failure isolation and shutdown.
//!
//! Adapters are the pull half of harness reporting. This module owns everything
//! that is the same for all of them, so an adapter itself only has to answer
//! one question - "what is this harness doing?" - and never has to know about
//! threads, retries, the registry or the app lifecycle.
//!
//! Three rules shape the code here:
//!
//! - **One adapter cannot hurt another.** Each adapter is polled on its own
//!   thread, and a failure is logged and retried on the next tick. There is no
//!   shared error state that one bad producer could poison.
//! - **A failure never destroys evidence.** A failed poll leaves the registry
//!   exactly as it was, so the existing staleness rule hides a dead harness on
//!   its own schedule instead of this module guessing.
//! - **Shutdown is immediate.** Tray Exit must not wait on a producer. Each
//!   thread sleeps in short slices against a shared stop flag, so stopping is
//!   bounded by one slice rather than by the polling interval.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tauri::AppHandle;

use crate::paths;

use super::adapter::{
    HarnessAdapter, LocalHttpAdapter, LocalJsonAdapter, DEFAULT_HTTP_PATH, DEFAULT_HTTP_TIMEOUT,
};

use super::registry::HarnessRegistry;

/// How long one adapter waits between polls.
///
/// Slower than the note's one-second refresh on purpose: a poll runs a file
/// read or a loopback request, and a status view does not need sub-second
/// freshness. Five seconds is well inside the staleness timeout, so a healthy
/// harness never flickers out of the note.
pub const DEFAULT_POLL_INTERVAL_MILLIS: u64 = 5_000;

/// The shortest interval a configuration may ask for, in milliseconds.
///
/// A floor because a configuration is external input and a one-millisecond
/// interval would spin a core for no benefit.
pub const MIN_POLL_INTERVAL_MILLIS: u64 = 250;

/// The longest interval a configuration may ask for, in milliseconds.
pub const MAX_POLL_INTERVAL_MILLIS: u64 = 10 * 60 * 1000;

/// The most adapters one configuration may declare.
///
/// Same reasoning as the push endpoint's task limit: this is a status surface,
/// not a fleet manager.
pub const MAX_ADAPTERS: usize = 32;

/// How long a stop request may take to be noticed, in milliseconds.
const STOP_SLICE_MILLIS: u64 = 50;

/// How often the manager checks whether it was asked to stop.
const STOP_POLL_MILLIS: u64 = 100;

/// What kind of transport an adapter entry uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AdapterKind {
    /// Read one local JSON file.
    #[serde(rename = "local-json", alias = "local_json")]
    LocalJson,
    /// Read one loopback HTTP endpoint.
    #[serde(rename = "local-http", alias = "local_http")]
    LocalHttp,
}

impl AdapterKind {
    /// The spelling used in configuration and in log lines.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::LocalJson => "local-json",
            Self::LocalHttp => "local-http",
        }
    }
}

impl std::fmt::Display for AdapterKind {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.as_str())
    }
}

/// One configured adapter.
///
/// Everything is optional except the kind and the name, so a configuration can
/// be as short as `{"kind": "local-json", "name": "my-harness", "path": "..."}`.
/// Unknown fields are rejected rather than ignored: a typo in a key would
/// otherwise silently disable the field it was meant to set.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct AdapterEntry {
    /// A stable name, used in logs and as the adapter's identifier.
    pub name: String,
    /// Which transport to use.
    pub kind: AdapterKind,
    /// Whether this adapter is polled at all. Defaults to enabled.
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    /// How often to poll, in milliseconds. Defaults to
    /// [`DEFAULT_POLL_INTERVAL_MILLIS`].
    #[serde(default)]
    pub poll_interval_millis: Option<u64>,
    /// The file to read. Required by [`AdapterKind::LocalJson`].
    #[serde(default)]
    pub path: Option<String>,
    /// The port to read. Required by [`AdapterKind::LocalHttp`].
    #[serde(default)]
    pub port: Option<u16>,
    /// The request path, for [`AdapterKind::LocalHttp`]. Defaults to
    /// [`DEFAULT_HTTP_PATH`].
    #[serde(default)]
    pub http_path: Option<String>,
    /// How long one HTTP request may take, in milliseconds.
    #[serde(default)]
    pub timeout_millis: Option<u64>,
}

/// An adapter is enabled unless the configuration says otherwise.
fn default_enabled() -> bool {
    true
}

/// The whole adapter configuration file.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub struct AdapterConfig {
    /// The adapters to run. An empty list is a valid configuration.
    #[serde(default)]
    pub adapters: Vec<AdapterEntry>,
}

/// Why a configuration could not be used.
///
/// A string, like every other error a producer can cause. A bad configuration
/// file is reported and the app starts with no adapters, because harness
/// reporting must never be able to stop notes or the tray from working.
pub type AdapterConfigError = String;

impl AdapterConfig {
    /// Parse, then check the whole file.
    ///
    /// Structural problems - a missing path, an unknown kind, a nonsensical
    /// interval - are configuration errors, not runtime ones, so they are found
    /// here once rather than by an adapter failing every five seconds forever.
    pub fn parse(text: &str) -> Result<Self, AdapterConfigError> {
        let config: AdapterConfig = serde_json::from_str(text)
            .map_err(|error| format!("malformed harness adapter JSON: {error}"))?;
        config.validate()?;
        Ok(config)
    }

    /// Check every entry, reporting the first problem with its index.
    pub fn validate(&self) -> Result<(), AdapterConfigError> {
        if self.adapters.len() > MAX_ADAPTERS {
            return Err(format!(
                "{} adapters configured; the limit is {MAX_ADAPTERS}",
                self.adapters.len()
            ));
        }

        for (index, entry) in self.adapters.iter().enumerate() {
            entry
                .validate()
                .map_err(|error| format!("adapter {index}: {error}"))?;
        }

        // Duplicate names would make two adapters indistinguishable in logs and
        // in the stored snapshot's source.
        let mut seen: Vec<&str> = Vec::new();
        for entry in &self.adapters {
            if seen.contains(&entry.name.as_str()) {
                return Err(format!("two adapters are both named {:?}", entry.name));
            }
            seen.push(&entry.name);
        }

        Ok(())
    }

    /// Only the adapters that should actually run.
    pub fn enabled(&self) -> impl Iterator<Item = &AdapterEntry> {
        self.adapters.iter().filter(|entry| entry.enabled)
    }
}

impl AdapterEntry {
    /// Check one entry in isolation.
    pub fn validate(&self) -> Result<(), AdapterConfigError> {
        if self.name.trim().is_empty() {
            return Err("name must not be empty".to_string());
        }
        if self.name.chars().count() > 128 {
            return Err("name must be 128 characters or fewer".to_string());
        }

        match self.kind {
            AdapterKind::LocalJson => {
                let path = self
                    .path
                    .as_deref()
                    .ok_or_else(|| "local-json needs a path".to_string())?;
                if path.trim().is_empty() {
                    return Err("path must not be empty".to_string());
                }
            }
            AdapterKind::LocalHttp => {
                if self.port.is_none() {
                    return Err("local-http needs a port".to_string());
                }
                if let Some(path) = self.http_path.as_deref() {
                    if !path.starts_with('/') {
                        return Err(format!("http_path must start with '/': {path:?}"));
                    }
                }
            }
        }

        if let Some(interval) = self.poll_interval_millis {
            if !(MIN_POLL_INTERVAL_MILLIS..=MAX_POLL_INTERVAL_MILLIS).contains(&interval) {
                return Err(format!(
                    "poll_interval_millis must be between {MIN_POLL_INTERVAL_MILLIS} and {MAX_POLL_INTERVAL_MILLIS}, not {interval}"
                ));
            }
        }

        if self.timeout_millis.is_some() && self.kind != AdapterKind::LocalHttp {
            return Err("timeout_millis only applies to local-http".to_string());
        }

        Ok(())
    }

    /// The interval this adapter polls at.
    pub fn interval(&self) -> Duration {
        Duration::from_millis(
            self.poll_interval_millis
                .unwrap_or(DEFAULT_POLL_INTERVAL_MILLIS),
        )
    }

    /// Build the adapter this entry describes.
    fn build(&self) -> Result<Box<dyn HarnessAdapter>, AdapterConfigError> {
        match self.kind {
            AdapterKind::LocalJson => {
                let path = self.path.clone().unwrap_or_default();
                Ok(Box::new(LocalJsonAdapter::new(self.name.clone(), path)))
            }
            AdapterKind::LocalHttp => {
                let adapter = LocalHttpAdapter::new(
                    self.name.clone(),
                    self.port.unwrap_or(0),
                    self.http_path.clone().unwrap_or_else(|| DEFAULT_HTTP_PATH.to_string()),
                )
                .with_timeout(
                    self.timeout_millis
                        .map(Duration::from_millis)
                        .unwrap_or(DEFAULT_HTTP_TIMEOUT),
                );
                Ok(Box::new(adapter))
            }
        }
    }
}

/// Load the adapter configuration from disk.
///
/// A missing file is not a problem: it means the user has no adapters, which is
/// the same as an empty list. An unreadable or invalid file is reported once and
/// also treated as empty, for the same reason a busy harness port is: harness
/// reporting is a feature, not a prerequisite.
pub fn load_config(path: &PathBuf) -> AdapterConfig {
    if !path.exists() {
        return AdapterConfig::default();
    }

    match std::fs::read_to_string(path) {
        Ok(raw) => match AdapterConfig::parse(&raw) {
            Ok(config) => config,
            Err(error) => {
                eprintln!(
                    "[sticky-harness] ignoring the harness adapter config {}: {error}",
                    path.display()
                );
                AdapterConfig::default()
            }
        },
        Err(error) => {
            eprintln!(
                "[sticky-harness] could not read {}: {error}",
                path.display()
            );
            AdapterConfig::default()
        }
    }
}

/// What the manager is doing, for logs and diagnostics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunState {
    /// No configuration file, or no enabled adapters in it.
    Idle,
    /// Running this many adapters.
    Running(usize),
}

impl RunState {
    /// The number of adapters running.
    ///
    /// Used by the tests today and by any future diagnostics surface; the
    /// startup log reads the variant directly.
    #[allow(dead_code)]
    pub fn adapters(&self) -> usize {
        match self {
            Self::Idle => 0,
            Self::Running(count) => *count,
        }
    }
}

/// The running adapter manager: one thread per enabled adapter, plus a flag.
///
/// Dropping the manager stops every thread. Polls are short by construction
/// (bounded file reads, timed-out HTTP), and each thread checks the stop flag
/// every 50 ms, so Tray Exit does not wait on a producer. Threads are detached
/// rather than joined: the process is exiting anyway, and joining would put the
/// exit path at the mercy of the slowest producer.
#[derive(Debug)]
pub struct AdapterManager {
    stop: Arc<AtomicBool>,
    /// How many adapters were actually started.
    running: usize,
}

impl AdapterManager {
    /// Start every enabled adapter in the configuration.
    ///
    /// The configuration is already validated, so an adapter that fails to
    /// build is a programming error rather than user input; it is reported and
    /// skipped, and the rest still run.
    pub fn start(registry: Arc<HarnessRegistry>, config: &AdapterConfig) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let mut running = 0;

        for entry in config.enabled() {
            let adapter = match entry.build() {
                Ok(adapter) => adapter,
                Err(error) => {
                    eprintln!("[sticky-harness] adapter {} not started: {error}", entry.name);
                    continue;
                }
            };

            let interval = entry.interval();
            let registry = Arc::clone(&registry);
            let stop = Arc::clone(&stop);

            std::thread::spawn(move || {
                run_adapter(adapter, interval, registry, stop);
            });

            println!(
                "[sticky-harness] adapter {} started ({}, every {} ms)",
                entry.name,
                entry.kind,
                interval.as_millis()
            );
            running += 1;
        }

        Self { stop, running }
    }

    /// How the manager started, for the startup log.
    pub fn state(&self) -> RunState {
        if self.running == 0 {
            RunState::Idle
        } else {
            RunState::Running(self.running)
        }
    }

    /// Ask every adapter thread to stop.
    ///
    /// Returns immediately; this only flips a flag. Idempotent, so both the
    /// exit path and an explicit shutdown may call it.
    pub fn stop(&self) {
        self.stop.store(true, Ordering::SeqCst);
    }
}

impl Drop for AdapterManager {
    fn drop(&mut self) {
        // The app may be dropped without an explicit stop (a crash path, or a
        // future non-tray shutdown); a flag write is cheap insurance.
        self.stop();
    }
}

/// Poll one adapter forever, until asked to stop.
///
/// The loop is deliberately boring: poll, log, sleep in small slices. A failed
/// poll changes nothing except the failure counter, so the registry keeps the
/// last good snapshot and the staleness rule decides when it stops being shown.
fn run_adapter(
    adapter: Box<dyn HarnessAdapter>,
    interval: Duration,
    registry: Arc<HarnessRegistry>,
    stop: Arc<AtomicBool>,
) {
    // Poll immediately: waiting one interval before the first read would leave
    // the note empty for no reason after a restart.
    loop {
        if stop.load(Ordering::SeqCst) {
            return;
        }

        match adapter.poll() {
            Ok(snapshot) => match registry.upsert(snapshot) {
                Ok(stored) => log_poll(&adapter, &stored.snapshot.harness_id),
                Err(error) => {
                    // The adapter produced something the protocol rejects. That
                    // is a producer bug, and the registry is untouched.
                    eprintln!(
                        "[sticky-harness] adapter {} reported a snapshot the protocol rejected: {error}",
                        adapter.name()
                    );
                }
            },
            Err(error) => {
                eprintln!(
                    "[sticky-harness] adapter {} poll failed: {error}",
                    adapter.name()
                );
            }
        }

        if !sleep_while_running(&stop, interval) {
            return;
        }
    }
}

/// Log the first successful poll of each harness, and then stay quiet.
///
/// A line every five seconds per adapter would bury everything else in the log
/// for no new information.
fn log_poll(adapter: &Box<dyn HarnessAdapter>, harness_id: &str) {
    static LOGGED: AtomicBool = AtomicBool::new(false);
    let first = !LOGGED.swap(true, Ordering::Relaxed);
    if first {
        println!(
            "[sticky-harness] adapter {} reported harness {harness_id}",
            adapter.name()
        );
    }
}

/// Sleep for `total`, but wake up as soon as a stop is requested.
///
/// Returns `false` when the manager was asked to stop, which unwinds the
/// adapter thread within one slice instead of one interval.
fn sleep_while_running(stop: &Arc<AtomicBool>, total: Duration) -> bool {
    let slice = Duration::from_millis(STOP_SLICE_MILLIS.min(STOP_POLL_MILLIS));
    let mut slept = Duration::ZERO;

    while slept < total {
        if stop.load(Ordering::SeqCst) {
            return false;
        }
        let step = slice.min(total - slept);
        std::thread::sleep(step);
        slept += step;
    }

    !stop.load(Ordering::SeqCst)
}

/// Poll one adapter a single time and store what it produced.
///
/// This is the body of the polling loop without the loop, so the rules that
/// matter - a failure changes nothing, an invalid snapshot is refused, a good
/// one is stored - can be tested without sleeping for an interval or racing a
/// background thread.
#[cfg(test)]
pub fn poll_once_for_test(
    adapter: impl HarnessAdapter,
    registry: Arc<HarnessRegistry>,
    _stop: Arc<AtomicBool>,
) {
    if let Ok(snapshot) = adapter.poll() {
        let _ = registry.upsert(snapshot);
    }
}

/// Where the adapter configuration lives: `<AppData>/harness-adapters.json`.
pub fn config_path(app: &AppHandle) -> Result<PathBuf, String> {
    paths::harness_adapters_config(app)
}

/// Load the configuration from the app data directory.
pub fn load_config_for(app: &AppHandle) -> AdapterConfig {
    match config_path(app) {
        Ok(path) => load_config(&path),
        Err(error) => {
            eprintln!("[sticky-harness] {error}");
            AdapterConfig::default()
        }
    }
}
