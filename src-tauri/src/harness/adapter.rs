//! The pull side: adapters that ask a harness what it is doing.
//!
//! Push (a harness POSTs to us) and pull (we read a harness's state) end in the
//! same place: a validated [`HarnessSnapshot`] handed to the registry. This
//! module defines that seam and the two transports that a real harness can be
//! reached through today:
//!
//! - [`LocalJsonAdapter`] reads one JSON file.
//! - [`LocalHttpAdapter`] reads one loopback HTTP endpoint.
//!
//! There is deliberately no `CodexAdapter` or `DeepSeekAdapter` here. Neither
//! product exposes a documented "what am I running right now" endpoint on this
//! machine, and inventing one from an internal cache would report guesses as
//! facts. Those harnesses are supported through the two paths above - plus the
//! push endpoint - and an adapter for them can be added later without touching
//! the registry, the protocol or the note.

use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

use super::protocol::{HarnessSnapshot, HarnessSource, ProtocolError, MAX_SNAPSHOT_BYTES};

/// A source that can be asked for the current state of one harness.
///
/// The contract is small on purpose:
///
/// - `poll` returns a snapshot or a readable error, and never panics on
///   external data, because everything it reads comes from outside the app.
/// - The snapshot it returns is validated before it is stored, so an adapter
///   may return a permissive draft rather than duplicating the checks.
/// - `poll` may block; the adapter manager runs it off the UI thread.
pub trait HarnessAdapter: Send + Sync {
    /// A stable name for logs and errors, such as `"local-json"`.
    fn name(&self) -> &str;

    /// Read the harness's current state. Taking `&self` keeps adapters cheap to
    /// call repeatedly from a background thread.
    fn poll(&self) -> Result<HarnessSnapshot, ProtocolError>;
}

/// The most a local file or HTTP response may be before it is refused.
///
/// The same limit as the push endpoint: a snapshot is a summary of what is
/// running, so anything larger is a mistake or an attack rather than data.
pub const MAX_ADAPTER_BYTES: usize = MAX_SNAPSHOT_BYTES;

/// A reference adapter that reads one snapshot from a local JSON file.
///
/// Read-only and text-only: the file is opened and parsed, never executed, and
/// the read is bounded so a runaway file cannot be slurped into memory. The
/// file is read fresh on every call, so an updated file is picked up with no
/// restart and no filesystem notification.
pub struct LocalJsonAdapter {
    name: String,
    path: PathBuf,
}

impl LocalJsonAdapter {
    /// Point an adapter at one file. The file is not read until `poll`.
    pub fn new(name: impl Into<String>, path: impl Into<PathBuf>) -> Self {
        Self {
            name: name.into(),
            path: path.into(),
        }
    }

    /// The file this adapter reads.
    ///
    /// Read by tests and by the configuration echo a future adapter list would
    /// show; nothing in the polling path needs it.
    #[allow(dead_code)]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl HarnessAdapter for LocalJsonAdapter {
    fn name(&self) -> &str {
        &self.name
    }

    fn poll(&self) -> Result<HarnessSnapshot, ProtocolError> {
        let bytes = read_bounded(&self.path).map_err(|error| {
            format!(
                "{} could not read {}: {error}",
                self.name,
                self.path.display()
            )
        })?;

        let text = String::from_utf8(bytes)
            .map_err(|_| format!("{} read a file that is not UTF-8 text", self.name))?;

        parse_local_snapshot(&text)
            .map(|snapshot| with_adapter_source(snapshot, &self.name))
    }
}

/// A reference adapter that reads one snapshot from a loopback HTTP endpoint.
///
/// Loopback only: a URL pointing anywhere else is refused before a socket is
/// opened, so an adapter cannot be quietly pointed at a remote host. Requests
/// carry a timeout and the response body is capped, so a server that hangs or
/// floods costs one poll rather than the app.
pub struct LocalHttpAdapter {
    name: String,
    port: u16,
    path: String,
    timeout: Duration,
}

impl LocalHttpAdapter {
    /// Point an adapter at `127.0.0.1:<port><path>`.
    ///
    /// The path defaults to the snapshot route a harness bridge would expose;
    /// an empty path is replaced rather than sent as a request for `/`.
    pub fn new(name: impl Into<String>, port: u16, path: impl Into<String>) -> Self {
        let path = path.into();
        Self {
            name: name.into(),
            port,
            path: if path.trim().is_empty() {
                DEFAULT_HTTP_PATH.to_string()
            } else {
                path
            },
            timeout: DEFAULT_HTTP_TIMEOUT,
        }
    }

    /// How long one request may take before it is abandoned.
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// The address this adapter reads. Always loopback.
    pub fn address(&self) -> SocketAddr {
        SocketAddr::from((Ipv4Addr::LOCALHOST, self.port))
    }

    /// The request path this adapter reads.
    #[allow(dead_code)]
    pub fn path(&self) -> &str {
        &self.path
    }
}

/// How long a local HTTP poll may take before it is abandoned.
///
/// A local endpoint answers in milliseconds, so a second is already generous;
/// the point is that a hung or missing producer costs one second, never a
/// wedged adapter thread.
pub const DEFAULT_HTTP_TIMEOUT: Duration = Duration::from_secs(1);

/// The route a local HTTP adapter reads by default.
pub const DEFAULT_HTTP_PATH: &str = "/api/harness/snapshot";

impl HarnessAdapter for LocalHttpAdapter {
    fn name(&self) -> &str {
        &self.name
    }

    fn poll(&self) -> Result<HarnessSnapshot, ProtocolError> {
        let body = self.get().map_err(|error| format!("{} {error}", self.name))?;
        let text = String::from_utf8(body)
            .map_err(|_| format!("{} read a response that is not UTF-8 text", self.name))?;

        parse_local_snapshot(&text).map(|snapshot| with_adapter_source(snapshot, &self.name))
    }
}

impl LocalHttpAdapter {
    /// One `GET`, with the response capped and the whole exchange on a timeout.
    ///
    /// Hand-written for the same reason the push endpoint is: one request to
    /// loopback does not justify an HTTP client dependency, and the protocol
    /// layer holds the real logic.
    fn get(&self) -> Result<Vec<u8>, String> {
        let address = self.address();
        let stream = TcpStream::connect_timeout(&address, self.timeout)
            .map_err(|error| format!("could not reach {address}: {error}"))?;

        stream
            .set_read_timeout(Some(self.timeout))
            .map_err(|error| format!("could not set a read timeout: {error}"))?;
        stream
            .set_write_timeout(Some(self.timeout))
            .map_err(|error| format!("could not set a write timeout: {error}"))?;

        let mut stream = stream;
        let request = format!(
            "GET {} HTTP/1.1\r\nHost: {address}\r\nAccept: application/json\r\nConnection: close\r\n\r\n",
            self.path
        );
        stream
            .write_all(request.as_bytes())
            .map_err(|error| format!("could not send the request: {error}"))?;

        // Headers are bounded separately and read first, so a server that
        // declares an enormous body is refused on its own `Content-Length`
        // rather than after the fact - and, more importantly, without being
        // buffered. The cap is the header block plus the body.
        let head = read_head(&mut stream)?;
        let declared = declared_body_length(&head);
        if let Some(length) = declared {
            if length > MAX_ADAPTER_BYTES {
                return Err(format!(
                    "the response declares {length} bytes of body; the limit is {MAX_ADAPTER_BYTES}"
                ));
            }
        }

        let mut rest = Vec::new();
        let budget = (MAX_ADAPTER_BYTES + 1) as u64;
        stream
            .take(budget)
            .read_to_end(&mut rest)
            .map_err(|error| format!("could not read the response: {error}"))?;

        if declared.is_none() && rest.len() as u64 >= budget {
            return Err(format!(
                "the response body is larger than the {MAX_ADAPTER_BYTES} byte limit"
            ));
        }

        let mut response = head;
        response.extend_from_slice(&rest);
        split_http_response(&response)
    }
}

/// Split a raw HTTP response into status, headers and body.
///
/// Deliberately minimal: one status line, headers we mostly ignore, and a body.
/// Both `Content-Length` and `chunked` are handled, because a producer that
/// streams its answer is still a producer. Everything else about HTTP - cookies,
/// redirects, compression - is irrelevant to reading one local status document.
fn split_http_response(response: &[u8]) -> Result<Vec<u8>, String> {
    let header_end = find_header_end(response)
        .ok_or_else(|| "the response had no complete header block".to_string())?;

    let head = String::from_utf8_lossy(&response[..header_end]).to_string();
    let mut lines = head.split("\r\n");
    let status_line = lines.next().unwrap_or_default();
    let status: u16 = status_line
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse().ok())
        .ok_or_else(|| format!("unreadable status line: {status_line:?}"))?;

    if !(200..300).contains(&status) {
        return Err(format!("the endpoint answered HTTP {status}"));
    }

    let mut chunked = false;
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            if name.trim().eq_ignore_ascii_case("transfer-encoding")
                && value.to_ascii_lowercase().contains("chunked")
            {
                chunked = true;
            }
        }
    }

    let body = response[header_end..].to_vec();
    if chunked {
        return dechunk(&body);
    }

    Ok(body)
}

/// Read the status line and headers, up to and including the blank line.
///
/// Bounded so a server that never sends a terminator cannot make the adapter
/// allocate without limit; the read timeout is what ends that case.
fn read_head(stream: &mut TcpStream) -> Result<Vec<u8>, String> {
    const MAX_HEAD_BYTES: usize = 8 * 1024;

    let mut head = Vec::new();
    let mut byte = [0_u8; 1];
    while head.len() < MAX_HEAD_BYTES {
        match stream.read(&mut byte) {
            Ok(0) => break,
            Ok(_) => head.push(byte[0]),
            Err(error) => return Err(format!("could not read the response: {error}")),
        }
        if head.ends_with(b"\r\n\r\n") || head.ends_with(b"\n\n") {
            return Ok(head);
        }
    }

    Err("the response headers never ended".to_string())
}

/// The body length a response declares, if it declares one.
fn declared_body_length(head: &[u8]) -> Option<usize> {
    let text = String::from_utf8_lossy(head).to_ascii_lowercase();
    for line in text.split("\r\n").skip(1) {
        if let Some((name, value)) = line.split_once(':') {
            if name.trim() == "content-length" {
                return value.trim().parse().ok();
            }
        }
    }
    None
}

/// Where the header block ends, i.e. just past the blank line.
fn find_header_end(response: &[u8]) -> Option<usize> {
    response
        .windows(4)
        .position(|window| window == b"\r\n\r\n")
        .map(|index| index + 4)
        .or_else(|| {
            response
                .windows(2)
                .position(|window| window == b"\n\n")
                .map(|index| index + 2)
        })
}

/// Decode a chunked body, refusing anything that does not add up.
///
/// A malformed chunk is an error rather than a truncated snapshot: a partial
/// snapshot would be a lie about what is running, and the registry would take
/// it at face value.
fn dechunk(body: &[u8]) -> Result<Vec<u8>, String> {
    let mut remaining = body;
    let mut out = Vec::new();

    loop {
        let line_end = match find_crlf(remaining) {
            Some(index) => index,
            None => return Err("the chunked body ended mid-size".to_string()),
        };
        let size_text = String::from_utf8_lossy(&remaining[..line_end]).to_string();
        let size_text = size_text.trim();
        let size = usize::from_str_radix(size_text.split(';').next().unwrap_or("").trim(), 16)
            .map_err(|_| format!("unreadable chunk size: {size_text:?}"))?;

        remaining = &remaining[line_end + 2..];
        if size == 0 {
            return Ok(out);
        }
        if remaining.len() < size {
            return Err("the chunked body ended early".to_string());
        }

        out.extend_from_slice(&remaining[..size]);
        if out.len() > MAX_ADAPTER_BYTES {
            return Err(format!(
                "the response body is larger than the {MAX_ADAPTER_BYTES} byte limit"
            ));
        }

        // Skip the chunk data and the CRLF that follows it.
        remaining = match remaining.get(size + 2..) {
            Some(rest) => rest,
            None => return Err("the chunked body ended without its terminator".to_string()),
        };
    }
}

/// The offset of the next CRLF, if there is one.
fn find_crlf(bytes: &[u8]) -> Option<usize> {
    bytes.windows(2).position(|window| window == b"\r\n")
}

/// Read a file with a hard size limit.
///
/// The check happens on the opened handle before the read, so an enormous file
/// is refused rather than loaded and then rejected.
fn read_bounded(path: &Path) -> Result<Vec<u8>, String> {
    let file = std::fs::File::open(path).map_err(|error| error.to_string())?;

    let length = file
        .metadata()
        .map_err(|error| error.to_string())?
        .len();
    if length > MAX_ADAPTER_BYTES as u64 {
        return Err(format!(
            "the file is {length} bytes; the limit is {MAX_ADAPTER_BYTES}"
        ));
    }

    let mut bytes = Vec::with_capacity(length as usize);
    file.take(MAX_ADAPTER_BYTES as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| error.to_string())?;

    Ok(bytes)
}

/// Stamp a snapshot with where it came from.
///
/// A producer should not have to describe our transport, and a snapshot that
/// came over the wire still says `push` in its own terms. The registry is what
/// shows which adapter reported, and this keeps one harness from two adapters
/// looking identical in the stored state.
fn with_adapter_source(mut snapshot: HarnessSnapshot, adapter: &str) -> HarnessSnapshot {
    snapshot.source = HarnessSource {
        source_type: "adapter".to_string(),
        name: Some(adapter.to_string()),
    };
    snapshot
}

/// Parse and validate a snapshot from JSON text.
///
/// Split out from the adapters so the protocol can be tested without touching
/// the filesystem or a socket, which is the part that actually matters: the
/// transport is incidental, the shape and the checks are not.
///
/// Both failure modes return a message rather than panicking - malformed JSON
/// from outside the app must never be able to bring the app down.
pub fn parse_local_snapshot(text: &str) -> Result<HarnessSnapshot, ProtocolError> {
    let snapshot: HarnessSnapshot = serde_json::from_str(text)
        .map_err(|error| format!("malformed harness snapshot JSON: {error}"))?;
    snapshot.validate()
}
