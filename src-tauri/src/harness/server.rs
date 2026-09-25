//! The local push endpoint: a very small HTTP server on `127.0.0.1`.
//!
//! Hand-written rather than framework-based. Three routes and a bounded body
//! do not justify a web framework's dependency tree, and the protocol layer is
//! where the real logic lives, so this file stays a thin translation between
//! HTTP and [`HarnessRegistry`].
//!
//! Security boundary: this binds the loopback interface only, never `0.0.0.0`,
//! so nothing on the local network can reach it. It is local harness IPC, not a
//! service. There is no authentication and no CORS header, because a browser
//! page has no business posting harness state; leaving CORS off means a page
//! cannot even read a response cross-origin.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::time::Duration;

use crate::harness::protocol::{
    HarnessSnapshot, HarnessSource, HARNESS_API_PORT, MAX_SNAPSHOT_BYTES,
};
use crate::harness::registry::HarnessRegistry;

/// How long a single request may take to arrive before the connection is dropped.
///
/// Without this, one client that opens a socket and says nothing would hold a
/// worker thread forever.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);

/// The response body is small on every route, so this is generous.
const MAX_HEADER_LINES: usize = 100;

/// Why the server is not running, if it is not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ServerState {
    /// Listening on this address.
    Listening(SocketAddr),
    /// Could not bind; the app is otherwise fully functional.
    Unavailable(String),
}

/// The address the push endpoint listens on.
pub fn local_address(port: u16) -> SocketAddr {
    SocketAddr::from((Ipv4Addr::LOCALHOST, port))
}

/// Start the push endpoint on a background thread.
///
/// Returns the state rather than a `Result`, because a failure here must never
/// stop the app: the tray and the notes have nothing to do with harness
/// reporting, so a busy port costs the user the harness feature and nothing
/// else.
pub fn start(registry: Arc<HarnessRegistry>, port: u16) -> ServerState {
    let address = local_address(port);

    let listener = match TcpListener::bind(address) {
        Ok(listener) => listener,
        Err(error) => {
            let reason = format!("could not bind {address}: {error}");
            eprintln!("[sticky-harness] harness API unavailable: {reason}");
            return ServerState::Unavailable(reason);
        }
    };

    std::thread::spawn(move || {
        for stream in listener.incoming() {
            match stream {
                Ok(stream) => {
                    // One thread per connection: this endpoint sees a handful of
                    // requests from a local harness, not public traffic.
                    let registry = Arc::clone(&registry);
                    std::thread::spawn(move || {
                        if let Err(error) = handle_connection(stream, &registry) {
                            eprintln!("[sticky-harness] harness request failed: {error}");
                        }
                    });
                }
                Err(error) => eprintln!("[sticky-harness] harness accept failed: {error}"),
            }
        }
    });

    println!("[sticky-harness] harness API listening on http://{address}");
    ServerState::Listening(address)
}

/// One HTTP request, already parsed.
struct Request {
    method: String,
    path: String,
    body: String,
}

/// One HTTP response.
struct Response {
    status: u16,
    reason: &'static str,
    body: String,
}

impl Response {
    fn json(status: u16, body: String) -> Self {
        let reason = match status {
            200 => "OK",
            400 => "Bad Request",
            404 => "Not Found",
            405 => "Method Not Allowed",
            413 => "Payload Too Large",
            500 => "Internal Server Error",
            _ => "OK",
        };
        Self {
            status,
            reason,
            body,
        }
    }

    /// A one-line JSON error, so a producer always gets a readable reason.
    fn error(status: u16, message: impl Into<String>) -> Self {
        let message = message.into();
        let body = serde_json::json!({ "error": message }).to_string();
        Self::json(status, body)
    }
}

/// Read one request, route it, and write the response.
fn handle_connection(mut stream: TcpStream, registry: &HarnessRegistry) -> Result<(), String> {
    stream
        .set_read_timeout(Some(REQUEST_TIMEOUT))
        .map_err(|error| format!("could not set a read timeout: {error}"))?;

    let request = match read_request(&mut stream) {
        Ok(request) => request,
        Err(error) => {
            // An oversized body is a 413; everything else is a malformed
            // request. Both are answered, never dropped.
            let status = if error.contains("the limit is") { 413 } else { 400 };
            let response = Response::error(status, error);
            return write_response(&mut stream, &response);
        }
    };

    let response = route(&request, registry);
    write_response(&mut stream, &response)
}

/// Parse the request line, headers and body.
///
/// The body is read with a hard limit: a request larger than
/// [`MAX_SNAPSHOT_BYTES`] is refused before it is buffered in full, so an
/// oversized payload cannot exhaust memory.
fn read_request(stream: &mut TcpStream) -> Result<Request, String> {
    let mut reader = BufReader::new(stream);

    let mut request_line = String::new();
    reader
        .read_line(&mut request_line)
        .map_err(|error| format!("could not read the request line: {error}"))?;
    if request_line.trim().is_empty() {
        return Err("empty request line".to_string());
    }

    let mut parts = request_line.split_whitespace();
    let method = parts
        .next()
        .ok_or_else(|| "missing method".to_string())?
        .to_string();
    let path = parts
        .next()
        .ok_or_else(|| "missing path".to_string())?
        .to_string();

    let mut content_length: Option<usize> = None;

    for _ in 0..MAX_HEADER_LINES {
        let mut line = String::new();
        let read = reader
            .read_line(&mut line)
            .map_err(|error| format!("could not read a header: {error}"))?;
        if read == 0 || line == "\r\n" || line == "\n" {
            break;
        }
        if let Some((name, value)) = line.split_once(':') {
            if name.trim().eq_ignore_ascii_case("content-length") {
                let parsed: usize = value
                    .trim()
                    .parse()
                    .map_err(|_| format!("invalid Content-Length: {}", value.trim()))?;
                content_length = Some(parsed);
            }
        }
    }

    let mut body = String::new();
    if let Some(length) = content_length {
        if length > MAX_SNAPSHOT_BYTES {
            // The body is never buffered, so it must still be consumed from the
            // socket. Leaving it unread makes the client see a connection reset
            // instead of the rejection, which is a much worse answer than a
            // clean "too large".
            drain(reader.get_mut(), length);
            return Err(format!(
                "snapshot body is {length} bytes; the limit is {MAX_SNAPSHOT_BYTES}"
            ));
        }
        let mut buffer = vec![0_u8; length];
        reader
            .read_exact(&mut buffer)
            .map_err(|error| format!("could not read the body: {error}"))?;
        body = String::from_utf8(buffer)
            .map_err(|_| "snapshot body must be UTF-8".to_string())?;
    }

    Ok(Request { method, path, body })
}

/// Read and discard `length` bytes, so a refused request can still be answered.
///
/// Bounded by the same read timeout as the request, and it gives up rather than
/// blocking forever if the client stops sending mid-body.
fn drain(stream: &mut TcpStream, length: usize) {
    let mut remaining = length;
    let mut scratch = [0_u8; 8 * 1024];
    while remaining > 0 {
        let want = remaining.min(scratch.len());
        match stream.read(&mut scratch[..want]) {
            Ok(0) => return,
            Ok(read) => remaining -= read,
            Err(_) => return,
        }
    }
}

/// Turn a parsed request into a response.
///
/// Three routes only. It is tempting to grow a small CRUD surface here; the
/// protocol does not need one, and every extra route is more to keep safe.
fn route(request: &Request, registry: &HarnessRegistry) -> Response {
    let path = request.path.split('?').next().unwrap_or("").to_string();

    match (request.method.as_str(), path.as_str()) {
        ("GET", "/health") => {
            let body = serde_json::json!({
                "status": "ok",
                "harnesses": registry.len(),
                "active_tasks": registry.list_active_tasks().len(),
            })
            .to_string();
            Response::json(200, body)
        }
        ("POST", "/api/harness/snapshot") => accept_snapshot(request, registry),
        ("GET", "/api/harness/snapshots") => {
            // Cloned rather than borrowed: the registry lock is released as
            // soon as `list` returns, so the list must own what it holds.
            let snapshots: Vec<HarnessSnapshot> = registry
                .list()
                .into_iter()
                .map(|stored| stored.snapshot)
                .collect();
            match serde_json::to_string(&snapshots) {
                Ok(body) => Response::json(200, body),
                Err(error) => Response::error(500, format!("could not serialise snapshots: {error}")),
            }
        }
        (_, "/health") | (_, "/api/harness/snapshot") | (_, "/api/harness/snapshots") => {
            Response::error(405, format!("{} is not allowed on {}", request.method, path))
        }
        _ => Response::error(404, format!("no route for {path}")),
    }
}

/// Validate and store one pushed snapshot.
///
/// A rejected snapshot is reported with the validator's own message and leaves
/// the registry untouched, so a malformed producer cannot damage the state the
/// note is showing.
fn accept_snapshot(request: &Request, registry: &HarnessRegistry) -> Response {
    let mut snapshot: HarnessSnapshot = match serde_json::from_str(&request.body) {
        Ok(snapshot) => snapshot,
        Err(error) => {
            return Response::error(400, format!("malformed snapshot JSON: {error}"));
        }
    };

    // An HTTP push is a push, whatever the producer sent; only fill in what is
    // missing rather than overriding a producer that knows better.
    if snapshot.source.source_type.trim().is_empty() {
        snapshot.source = HarnessSource::push();
    }

    match registry.upsert(snapshot) {
        Ok(stored) => {
            let body = serde_json::json!({
                "accepted": true,
                "harness_id": stored.snapshot.harness_id,
                "harnesses": registry.len(),
                "active_tasks": registry.list_active_tasks().len(),
            })
            .to_string();
            Response::json(200, body)
        }
        Err(error) => Response::error(400, error),
    }
}

/// Write the response and close the connection.
///
/// `Connection: close` keeps the loop trivial: no keep-alive state to track for
/// a client that talks to us a few times a minute.
fn write_response(stream: &mut TcpStream, response: &Response) -> Result<(), String> {
    let head = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        response.status,
        response.reason,
        response.body.len()
    );

    stream
        .write_all(head.as_bytes())
        .and_then(|()| stream.write_all(response.body.as_bytes()))
        .and_then(|()| stream.flush())
        .map_err(|error| format!("could not write the response: {error}"))
}

/// The port the server uses, exposed for tests and logs.
pub fn default_port() -> u16 {
    HARNESS_API_PORT
}
