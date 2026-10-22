//! Per-browser HTTP proxy that resolves origin hosts through Google's DoH JSON API.
//! It is bound to loopback and used by Pip's WebViews and optional background HTTP clients.

use reqwest::blocking::Client;
use serde_json::Value;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{IpAddr, SocketAddr, TcpListener, TcpStream, ToSocketAddrs};
#[cfg(any(debug_assertions, test))]
use std::sync::atomic::AtomicU64;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, SyncSender, TrySendError};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const MAX_HEADER_BYTES: usize = 64 * 1024;
const CACHE_MAX_AGE: Duration = Duration::from_secs(600);
const CACHE_MAX_ENTRIES: usize = 2048;
/// Bound accepted client handlers, including CONNECT tunnels, to avoid an
/// unbounded number of OS threads when the loopback proxy is overloaded.
const MAX_ACTIVE_PROXY_HANDLERS: usize = 32;
/// Bound concurrent A/AAAA HTTP requests independently of client connections.
const MAX_CONCURRENT_DOH_QUERIES: usize = 8;
/// A stuck OS resolver must not consume all of the loopback proxy's request threads.
const MAX_SYSTEM_DNS_WORKERS: usize = 4;
const MAX_QUEUED_SYSTEM_DNS_LOOKUPS: usize = 16;
const SYSTEM_DNS_FALLBACK_TIMEOUT: Duration = Duration::from_secs(4);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DnsResolverReport {
    SystemFallbackUsed,
    BothResolversFailed,
}

type DnsResolverReporter = Arc<dyn Fn(DnsResolverReport) + Send + Sync>;

#[derive(Clone)]
struct DnsFallbackSignal {
    system_fallback_used: Arc<AtomicBool>,
    both_resolvers_failed: Arc<AtomicBool>,
    reporter: Option<DnsResolverReporter>,
}

impl DnsFallbackSignal {
    fn report_system_fallback(&self) {
        if !self.system_fallback_used.swap(true, Ordering::AcqRel) {
            eprintln!(
                "Pip Google DNS lookup failed; using Windows DNS fallback for some requests."
            );
            if let Some(reporter) = &self.reporter {
                reporter(DnsResolverReport::SystemFallbackUsed);
            }
        }
    }

    fn report_both_resolvers_failed(&self) {
        if !self.both_resolvers_failed.swap(true, Ordering::AcqRel) {
            eprintln!(
                "Pip Google DNS and Windows DNS both failed to resolve a requested hostname."
            );
            if let Some(reporter) = &self.reporter {
                reporter(DnsResolverReport::BothResolversFailed);
            }
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
enum DnsResolutionSource {
    GoogleDoh,
    SystemFallback,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GoogleDohFailure {
    Transient,
    NameNotFound,
    NoData,
    InvalidResponse,
}

#[derive(Debug, PartialEq, Eq)]
struct DnsResolution {
    addresses: Vec<IpAddr>,
    source: DnsResolutionSource,
}

#[derive(Debug, PartialEq, Eq)]
enum DnsResolutionFailure {
    GoogleDohRejected,
    BothResolversFailed,
}

/// Configure a Reqwest client to use Pip's already-running local proxy when one
/// is available. A configured proxy is explicit: request failures must not be
/// retried by callers without it.
pub fn with_optional_proxy(
    builder: reqwest::blocking::ClientBuilder,
    proxy_port: Option<u16>,
) -> Result<reqwest::blocking::ClientBuilder, String> {
    let Some(port) = proxy_port else {
        return Ok(builder);
    };
    // These clients use HTTPS for Google DNS and other background requests.
    // Proxy::http only intercepts `http://` destinations, silently sending
    // HTTPS requests direct and resolving them with the system resolver.
    let proxy = reqwest::Proxy::all(format!("http://127.0.0.1:{port}"))
        .map_err(|error| format!("cannot configure Pip DNS proxy: {error}"))?;
    Ok(builder.proxy(proxy))
}

/// Configure a background HTTP client consistently with the persisted Google
/// DNS setting. When enabled, a missing local proxy is an error: silently
/// returning an unproxied client would send its hostname lookups to Windows DNS.
pub fn with_google_dns_proxy(
    builder: reqwest::blocking::ClientBuilder,
    enabled: bool,
    proxy_port: Option<u16>,
) -> Result<reqwest::blocking::ClientBuilder, String> {
    if !enabled {
        // Keep explicit optional-proxy callers working even when Google DNS is
        // disabled; production strict callers pass this flag from the setting.
        return with_optional_proxy(builder, proxy_port);
    }
    let port = proxy_port.ok_or_else(|| {
        "Google DNS is enabled, but Pip's local DNS proxy is unavailable. Restart Pip or turn Google DNS off.".to_string()
    })?;
    with_optional_proxy(builder, Some(port))
}

pub struct DohProxy {
    port: u16,
    running: Arc<AtomicBool>,
    system_dns_fallback_used: Arc<AtomicBool>,
    both_resolvers_failed: Arc<AtomicBool>,
    #[cfg(debug_assertions)]
    accepted_connections: Arc<AtomicU64>,
    #[cfg(test)]
    accepted_connections_for_test: Arc<AtomicU64>,
    #[cfg(debug_assertions)]
    successful_doh_resolutions: Arc<AtomicU64>,
    #[cfg(debug_assertions)]
    successful_connect_tunnels: Arc<AtomicU64>,
    #[cfg(test)]
    active_handlers: Arc<ActiveHandlerLimit>,
    listener_thread: Option<JoinHandle<()>>,
}

/// A loopback HTTP proxy used only when Google DNS is enabled but the DoH proxy
/// cannot start. Returning an explicit 503 keeps WebView2 from attempting a
/// direct connection while avoiding a fixed sentinel port in normal failures.
pub struct RejectingProxy {
    port: u16,
    running: Arc<AtomicBool>,
    listener_thread: Option<JoinHandle<()>>,
}

impl RejectingProxy {
    pub fn start() -> Result<Self, String> {
        let listener = TcpListener::bind(("127.0.0.1", 0))
            .map_err(|error| format!("cannot bind DNS failure proxy: {error}"))?;
        let port = listener
            .local_addr()
            .map_err(|error| format!("cannot read DNS failure proxy port: {error}"))?
            .port();
        let running = Arc::new(AtomicBool::new(true));
        let thread_running = running.clone();
        let listener_thread = thread::Builder::new()
            .name("pip-google-dns-fail-closed".into())
            .spawn(move || {
                while thread_running.load(Ordering::Relaxed) {
                    match listener.accept() {
                        Ok((mut stream, _)) => {
                            if !thread_running.load(Ordering::Relaxed) {
                                break;
                            }
                            let _ = stream.set_read_timeout(Some(Duration::from_secs(3)));
                            let _ = stream.set_write_timeout(Some(Duration::from_secs(3)));
                            if read_request_head(&mut stream).is_ok() {
                                respond_error(&mut stream, "503 Service Unavailable");
                            }
                        }
                        Err(error) => {
                            eprintln!("Pip Google DNS failure proxy listener: {error}");
                            if !thread_running.load(Ordering::Relaxed) {
                                break;
                            }
                            thread::sleep(Duration::from_millis(100));
                        }
                    }
                }
            })
            .map_err(|error| format!("cannot start DNS failure proxy: {error}"))?;
        Ok(Self {
            port,
            running,
            listener_thread: Some(listener_thread),
        })
    }

    pub fn port(&self) -> u16 {
        self.port
    }
}

impl Drop for RejectingProxy {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
        let _ = TcpStream::connect(("127.0.0.1", self.port));
        if let Some(thread) = self.listener_thread.take() {
            let _ = thread.join();
        }
    }
}

struct ActiveHandlerLimit {
    active: AtomicUsize,
    limit: usize,
}

impl ActiveHandlerLimit {
    fn new(limit: usize) -> Self {
        Self {
            active: AtomicUsize::new(0),
            limit,
        }
    }

    fn try_acquire(self: &Arc<Self>) -> Option<ActiveHandlerPermit> {
        let mut active = self.active.load(Ordering::Acquire);
        loop {
            if active >= self.limit {
                return None;
            }
            match self.active.compare_exchange_weak(
                active,
                active + 1,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return Some(ActiveHandlerPermit(self.clone())),
                Err(observed) => active = observed,
            }
        }
    }
}

struct ActiveHandlerPermit(Arc<ActiveHandlerLimit>);

impl Drop for ActiveHandlerPermit {
    fn drop(&mut self) {
        self.0.active.fetch_sub(1, Ordering::AcqRel);
    }
}

struct DohQueryLimit {
    active: Mutex<usize>,
    available: Condvar,
    limit: usize,
}

impl DohQueryLimit {
    fn new(limit: usize) -> Self {
        Self {
            active: Mutex::new(0),
            available: Condvar::new(),
            limit,
        }
    }

    fn acquire(self: &Arc<Self>) -> DohQueryPermit {
        let mut active = self
            .active
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        while *active >= self.limit {
            active = self
                .available
                .wait(active)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
        }
        *active += 1;
        DohQueryPermit(self.clone())
    }
}

struct DohQueryPermit(Arc<DohQueryLimit>);

impl Drop for DohQueryPermit {
    fn drop(&mut self) {
        let mut active = self
            .0
            .active
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        *active -= 1;
        self.0.available.notify_one();
    }
}

impl DohProxy {
    #[cfg(test)]
    pub fn start() -> Result<Self, String> {
        Self::start_inner(None)
    }

    pub fn start_with_fallback_reporter(
        reporter: impl Fn(DnsResolverReport) + Send + Sync + 'static,
    ) -> Result<Self, String> {
        Self::start_inner(Some(Arc::new(reporter)))
    }

    fn start_inner(reporter: Option<DnsResolverReporter>) -> Result<Self, String> {
        let listener = TcpListener::bind(("127.0.0.1", 0))
            .map_err(|error| format!("cannot bind local proxy: {error}"))?;
        let port = listener
            .local_addr()
            .map_err(|error| format!("cannot read local proxy port: {error}"))?
            .port();
        let client = Client::builder()
            .resolve_to_addrs(
                "dns.google",
                &[
                    "8.8.8.8:443".parse().expect("Google DNS bootstrap address"),
                    "8.8.4.4:443".parse().expect("Google DNS bootstrap address"),
                ],
            )
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(8))
            .build()
            .map_err(|error| format!("cannot create Google DNS client: {error}"))?;
        let cache = Arc::new(Mutex::new(HashMap::<String, CachedAnswers>::new()));
        let active_handlers = Arc::new(ActiveHandlerLimit::new(MAX_ACTIVE_PROXY_HANDLERS));
        let query_limit = Arc::new(DohQueryLimit::new(MAX_CONCURRENT_DOH_QUERIES));
        let system_dns_fallback_used = Arc::new(AtomicBool::new(false));
        let both_resolvers_failed = Arc::new(AtomicBool::new(false));
        let fallback_signal = DnsFallbackSignal {
            system_fallback_used: system_dns_fallback_used.clone(),
            both_resolvers_failed: both_resolvers_failed.clone(),
            reporter,
        };
        let running = Arc::new(AtomicBool::new(true));
        let thread_running = running.clone();
        let thread_active_handlers = active_handlers.clone();
        let thread_query_limit = query_limit.clone();
        let thread_fallback_signal = fallback_signal.clone();
        #[cfg(debug_assertions)]
        let accepted_connections = Arc::new(AtomicU64::new(0));
        #[cfg(test)]
        let accepted_connections_for_test = Arc::new(AtomicU64::new(0));
        #[cfg(debug_assertions)]
        let thread_connection_count = accepted_connections.clone();
        #[cfg(test)]
        let thread_test_connection_count = accepted_connections_for_test.clone();
        #[cfg(debug_assertions)]
        let successful_doh_resolutions = Arc::new(AtomicU64::new(0));
        #[cfg(debug_assertions)]
        let thread_doh_resolution_count = successful_doh_resolutions.clone();
        #[cfg(debug_assertions)]
        let successful_connect_tunnels = Arc::new(AtomicU64::new(0));
        #[cfg(debug_assertions)]
        let thread_connect_tunnel_count = successful_connect_tunnels.clone();
        let listener_thread = thread::Builder::new()
            .name("pip-google-dns-proxy".to_string())
            .spawn(move || {
                while thread_running.load(Ordering::Relaxed) {
                    match listener.accept() {
                        Ok((stream, _)) => {
                            if !thread_running.load(Ordering::Relaxed) {
                                break;
                            }
                            #[cfg(debug_assertions)]
                            thread_connection_count.fetch_add(1, Ordering::Relaxed);
                            #[cfg(test)]
                            thread_test_connection_count.fetch_add(1, Ordering::Relaxed);
                            let client = client.clone();
                            let cache = cache.clone();
                            let query_limit = thread_query_limit.clone();
                            let fallback_signal = thread_fallback_signal.clone();
                            #[cfg(debug_assertions)]
                            let doh_resolution_count = thread_doh_resolution_count.clone();
                            #[cfg(debug_assertions)]
                            let connect_tunnel_count = thread_connect_tunnel_count.clone();
                            let Some(handler_permit) = thread_active_handlers.try_acquire() else {
                                // Reject overload without creating another OS thread.
                                drop(stream);
                                continue;
                            };
                            let _ = thread::Builder::new()
                                .name("pip-google-dns-request".to_string())
                                .spawn(move || {
                                    let _handler_permit = handler_permit;
                                    handle_client(
                                        stream,
                                        client,
                                        cache,
                                        query_limit,
                                        fallback_signal,
                                        #[cfg(debug_assertions)]
                                        doh_resolution_count,
                                        #[cfg(debug_assertions)]
                                        connect_tunnel_count,
                                    )
                                });
                        }
                        Err(error) => {
                            eprintln!("Pip Google DNS proxy listener: {error}");
                            if !thread_running.load(Ordering::Relaxed) {
                                break;
                            }
                            thread::sleep(Duration::from_millis(100));
                        }
                    }
                }
            })
            .map_err(|error| format!("cannot start local proxy: {error}"))?;

        Ok(Self {
            port,
            running,
            system_dns_fallback_used,
            both_resolvers_failed,
            #[cfg(debug_assertions)]
            accepted_connections,
            #[cfg(test)]
            accepted_connections_for_test,
            #[cfg(debug_assertions)]
            successful_doh_resolutions,
            #[cfg(debug_assertions)]
            successful_connect_tunnels,
            #[cfg(test)]
            active_handlers,
            listener_thread: Some(listener_thread),
        })
    }

    pub fn system_dns_fallback_used(&self) -> bool {
        self.system_dns_fallback_used.load(Ordering::Acquire)
    }

    pub fn both_resolvers_failed(&self) -> bool {
        self.both_resolvers_failed.load(Ordering::Acquire)
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    #[cfg(debug_assertions)]
    pub fn accepted_connections(&self) -> u64 {
        self.accepted_connections.load(Ordering::Relaxed)
    }

    #[cfg(debug_assertions)]
    pub fn successful_doh_resolutions(&self) -> u64 {
        self.successful_doh_resolutions.load(Ordering::Relaxed)
    }

    #[cfg(debug_assertions)]
    pub fn successful_connect_tunnels(&self) -> u64 {
        self.successful_connect_tunnels.load(Ordering::Relaxed)
    }

    #[cfg(test)]
    fn active_handler_count(&self) -> usize {
        self.active_handlers.active.load(Ordering::Acquire)
    }

    #[cfg(test)]
    fn accepted_connection_count(&self) -> u64 {
        self.accepted_connections_for_test.load(Ordering::Relaxed)
    }
}

/// Exercise the same local HTTP CONNECT proxy used by Pip's WebViews. A direct
/// DoH request only proves Google's resolver is reachable; this also proves
/// Pip can resolve a real destination through that resolver and reach it.
pub fn test_google_doh_through_proxy(port: u16) -> Result<(), String> {
    let builder = with_optional_proxy(Client::builder(), Some(port))?;
    let client = builder
        .connect_timeout(Duration::from_secs(8))
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|error| format!("cannot create Pip DNS proxy test client: {error}"))?;
    let response = client
        .get("https://www.google.com/generate_204")
        .send()
        .map_err(|error| format!("Pip DNS proxy could not reach Google: {error}"))?;
    if response.status().as_u16() != 204 {
        return Err(format!(
            "Google DNS proxy test expected HTTP 204, received {}",
            response.status()
        ));
    }
    Ok(())
}

impl Drop for DohProxy {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
        // Wake the blocking accept call so shutdown does not need a polling loop.
        let _ = TcpStream::connect(("127.0.0.1", self.port));
        if let Some(thread) = self.listener_thread.take() {
            let _ = thread.join();
        }
    }
}

struct CachedAnswers {
    addresses: Vec<IpAddr>,
    expires: Instant,
    last_used: Instant,
}

fn handle_client(
    client_stream: TcpStream,
    client: Client,
    cache: Arc<Mutex<HashMap<String, CachedAnswers>>>,
    query_limit: Arc<DohQueryLimit>,
    fallback_signal: DnsFallbackSignal,
    #[cfg(debug_assertions)] successful_doh_resolutions: Arc<AtomicU64>,
    #[cfg(debug_assertions)] successful_connect_tunnels: Arc<AtomicU64>,
) {
    let resolver = move |host: &str| {
        let result = resolve_with_fallback(
            host,
            |hostname| resolve_google_doh(hostname, &client, &cache, &query_limit),
            resolve_with_system_dns,
        );
        #[cfg(debug_assertions)]
        if matches!(
            &result,
            Ok(resolution) if resolution.source == DnsResolutionSource::GoogleDoh
        ) {
            successful_doh_resolutions.fetch_add(1, Ordering::Relaxed);
        }
        match result {
            Ok(resolution) => {
                if resolution.source == DnsResolutionSource::SystemFallback {
                    fallback_signal.report_system_fallback();
                }
                Some(resolution.addresses)
            }
            Err(DnsResolutionFailure::BothResolversFailed) => {
                fallback_signal.report_both_resolvers_failed();
                None
            }
            Err(DnsResolutionFailure::GoogleDohRejected) => None,
        }
    };
    handle_client_with_resolver_and_tunnel_counter(
        client_stream,
        resolver,
        #[cfg(debug_assertions)]
        Some(successful_connect_tunnels),
    );
}

#[cfg(test)]
fn handle_client_with_resolver(
    client_stream: TcpStream,
    resolve: impl Fn(&str) -> Option<Vec<IpAddr>>,
) {
    handle_client_with_resolver_and_tunnel_counter(
        client_stream,
        resolve,
        #[cfg(debug_assertions)]
        None,
    );
}

fn handle_client_with_resolver_and_tunnel_counter(
    mut client_stream: TcpStream,
    resolve: impl Fn(&str) -> Option<Vec<IpAddr>>,
    #[cfg(debug_assertions)] successful_connect_tunnels: Option<Arc<AtomicU64>>,
) {
    let _ = client_stream.set_read_timeout(Some(Duration::from_secs(20)));
    let _ = client_stream.set_write_timeout(Some(Duration::from_secs(20)));
    let Ok(head) = read_request_head(&mut client_stream) else {
        return;
    };
    let Ok(request) = String::from_utf8(head) else {
        return;
    };
    let Some((method, target, _version)) = request.lines().next().and_then(parse_request_line)
    else {
        respond_error(&mut client_stream, "400 Bad Request");
        return;
    };

    if method.eq_ignore_ascii_case("CONNECT") {
        let Some((host, port)) = parse_authority(target, 443) else {
            respond_error(&mut client_stream, "400 Bad Request");
            return;
        };
        let Some(upstream) = connect_to_host(&host, port, &resolve) else {
            respond_error(&mut client_stream, "502 Bad Gateway");
            return;
        };
        if client_stream
            .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
            .is_err()
        {
            return;
        }
        #[cfg(debug_assertions)]
        if let Some(counter) = &successful_connect_tunnels {
            counter.fetch_add(1, Ordering::Relaxed);
        }
        let _ = client_stream.set_read_timeout(None);
        let _ = client_stream.set_write_timeout(None);
        let _ = upstream.set_read_timeout(None);
        let _ = upstream.set_write_timeout(None);
        tunnel(client_stream, upstream);
        return;
    }

    let Some((host, port, path)) = parse_http_target(target, &request) else {
        respond_error(&mut client_stream, "400 Bad Request");
        return;
    };
    let Some(mut upstream) = connect_to_host(&host, port, &resolve) else {
        respond_error(&mut client_stream, "502 Bad Gateway");
        return;
    };

    let mut lines = request.lines();
    let _ = lines.next();
    if write!(upstream, "{method} {path} HTTP/1.1\r\n").is_err() {
        return;
    }
    for line in lines {
        if line.is_empty() {
            break;
        }
        let lower = line.to_ascii_lowercase();
        if lower.starts_with("proxy-") || lower.starts_with("connection:") {
            continue;
        }
        if upstream.write_all(line.as_bytes()).is_err() || upstream.write_all(b"\r\n").is_err() {
            return;
        }
    }
    if upstream.write_all(b"Connection: close\r\n\r\n").is_err() {
        return;
    }
    if request
        .lines()
        .skip(1)
        .any(|line| line.eq_ignore_ascii_case("Expect: 100-continue"))
        && client_stream
            .write_all(b"HTTP/1.1 100 Continue\r\n\r\n")
            .is_err()
    {
        return;
    }
    let chunked = request.lines().skip(1).any(|line| {
        line.split_once(':').is_some_and(|(name, value)| {
            name.eq_ignore_ascii_case("transfer-encoding")
                && value.to_ascii_lowercase().contains("chunked")
        })
    });
    if chunked {
        if copy_chunked_body(&mut client_stream, &mut upstream).is_err() {
            return;
        }
    } else if let Some(length) = request.lines().skip(1).find_map(|line| {
        line.split_once(':')
            .filter(|(name, _)| name.eq_ignore_ascii_case("content-length"))
            .and_then(|(_, value)| value.trim().parse::<u64>().ok())
    }) {
        let copied = std::io::copy(&mut (&mut client_stream).take(length), &mut upstream);
        if !matches!(copied, Ok(copied_length) if copied_length == length) {
            return;
        }
    }
    let _ = std::io::copy(&mut upstream, &mut client_stream);
}

fn read_request_head(stream: &mut TcpStream) -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::with_capacity(4096);
    let mut peeked = [0_u8; 4096];
    let mut consumed = [0_u8; 4096];
    loop {
        let remaining = MAX_HEADER_BYTES.saturating_sub(bytes.len());
        if remaining == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "proxy request headers are too large",
            ));
        }
        let peek_len = peeked.len().min(remaining);
        let available = stream.peek(&mut peeked[..peek_len])?;
        if available == 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "proxy request ended before headers completed",
            ));
        }
        let mut probe = Vec::with_capacity(bytes.len() + available);
        probe.extend_from_slice(&bytes);
        probe.extend_from_slice(&peeked[..available]);
        if let Some(end) = probe.windows(4).position(|window| window == b"\r\n\r\n") {
            let needed = end + 4 - bytes.len();
            stream.read_exact(&mut consumed[..needed])?;
            bytes.extend_from_slice(&consumed[..needed]);
            return Ok(bytes);
        }
        if available >= remaining {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "proxy request headers are too large",
            ));
        }
        stream.read_exact(&mut consumed[..available])?;
        bytes.extend_from_slice(&consumed[..available]);
    }
}

fn copy_chunked_body(reader: &mut TcpStream, writer: &mut TcpStream) -> std::io::Result<()> {
    loop {
        let size_line = read_crlf_line(reader)?;
        let size_text = std::str::from_utf8(&size_line).map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, "invalid chunk size")
        })?;
        let size = size_text
            .trim_end_matches(['\r', '\n'])
            .split(';')
            .next()
            .unwrap_or_default()
            .trim();
        let size = usize::from_str_radix(size, 16).map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::InvalidData, "invalid chunk size")
        })?;
        writer.write_all(&size_line)?;
        if size == 0 {
            loop {
                let trailer = read_crlf_line(reader)?;
                writer.write_all(&trailer)?;
                if trailer == b"\r\n" {
                    return Ok(());
                }
            }
        }
        std::io::copy(&mut reader.take(size as u64), writer)?;
        let mut crlf = [0; 2];
        reader.read_exact(&mut crlf)?;
        writer.write_all(&crlf)?;
    }
}

fn read_crlf_line(reader: &mut TcpStream) -> std::io::Result<Vec<u8>> {
    let mut line = Vec::with_capacity(32);
    let mut byte = [0; 1];
    while line.len() < 8192 {
        reader.read_exact(&mut byte)?;
        line.push(byte[0]);
        if line.ends_with(b"\r\n") {
            return Ok(line);
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        "HTTP chunk line is too large",
    ))
}

fn parse_request_line(line: &str) -> Option<(&str, &str, &str)> {
    let mut fields = line.split_whitespace();
    Some((fields.next()?, fields.next()?, fields.next()?))
}

fn parse_authority(authority: &str, default_port: u16) -> Option<(String, u16)> {
    let authority = authority.trim();
    if authority.is_empty()
        || authority
            .chars()
            .any(|character| character.is_ascii_control() || character.is_ascii_whitespace())
        || authority.contains('@')
    {
        return None;
    }
    if let Some(rest) = authority.strip_prefix('[') {
        let end = rest.find(']')?;
        let host = rest[..end].parse::<std::net::Ipv6Addr>().ok()?.to_string();
        let suffix = &rest[end + 1..];
        let port = if suffix.is_empty() {
            default_port
        } else {
            suffix.strip_prefix(':')?.parse::<u16>().ok()?
        };
        return Some((host, port));
    }
    if authority.contains(['[', ']']) {
        return None;
    }
    match authority.rsplit_once(':') {
        Some((host, port)) => {
            // IPv6 literals must use brackets in an HTTP authority. Besides
            // following URI syntax, rejecting ambiguous suffixes avoids
            // interpreting malformed CONNECT targets as a different host.
            if host.is_empty()
                || host.contains(':')
                || port.is_empty()
                || !port.bytes().all(|byte| byte.is_ascii_digit())
            {
                return None;
            }
            Some((host.to_string(), port.parse().ok()?))
        }
        None => Some((authority.to_string(), default_port)),
    }
}

fn parse_http_target(target: &str, request: &str) -> Option<(String, u16, String)> {
    if let Some(rest) = target.strip_prefix("http://") {
        // The absolute-form request target may omit the slash when the URL has
        // only a query (for example `http://example.test?x=1`). Split authority
        // at any URI delimiter so the query is never sent to DNS as part of
        // the hostname. Fragments are not sent over HTTP.
        let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
        let authority = &rest[..authority_end];
        let suffix = &rest[authority_end..];
        let path_and_query = suffix.split('#').next().unwrap_or_default();
        let path = if path_and_query.is_empty() {
            "/".to_string()
        } else if path_and_query.starts_with('?') {
            format!("/{path_and_query}")
        } else {
            path_and_query.to_string()
        };
        let (host, port) = parse_authority(authority, 80)?;
        return Some((host, port, path));
    }

    let host_header = request
        .lines()
        .skip(1)
        .find_map(|line| {
            line.split_once(':')
                .filter(|(name, _)| name.eq_ignore_ascii_case("host"))
        })?
        .1
        .trim();
    let (host, port) = parse_authority(host_header, 80)?;
    Some((host, port, target.to_string()))
}

fn connect_to_host(
    host: &str,
    port: u16,
    resolve: &impl Fn(&str) -> Option<Vec<IpAddr>>,
) -> Option<TcpStream> {
    if let Ok(ip) = host.parse::<IpAddr>() {
        return TcpStream::connect_timeout(&SocketAddr::new(ip, port), Duration::from_secs(4)).ok();
    }
    let addresses = resolve(host)?;
    let (ipv6, ipv4): (Vec<_>, Vec<_>) =
        addresses.into_iter().partition(|address| address.is_ipv6());
    // On dual-stack Windows networks, IPv4 can complete a TCP handshake
    // through a filtering gateway and then reset TLS. Try IPv6 quickly first,
    // then fall back to IPv4 so IPv4-only networks keep working.
    for ip in ipv6.into_iter().take(4) {
        let result =
            TcpStream::connect_timeout(&SocketAddr::new(ip, port), Duration::from_millis(250));
        if let Ok(stream) = result {
            return Some(stream);
        }
    }
    ipv4.into_iter().find_map(|ip| {
        let result = TcpStream::connect_timeout(&SocketAddr::new(ip, port), Duration::from_secs(3));
        result.ok()
    })
}

fn resolve_google_doh(
    host: &str,
    client: &Client,
    cache: &Arc<Mutex<HashMap<String, CachedAnswers>>>,
    query_limit: &Arc<DohQueryLimit>,
) -> Result<Vec<IpAddr>, GoogleDohFailure> {
    let hostname = host.trim_end_matches('.').to_ascii_lowercase();
    if hostname.is_empty() || hostname.chars().any(char::is_control) {
        return Err(GoogleDohFailure::InvalidResponse);
    }
    if let Ok(ip) = hostname.parse::<IpAddr>() {
        return Ok(vec![ip]);
    }
    let now = Instant::now();
    let mut entries = cache
        .lock()
        .map_err(|_| GoogleDohFailure::InvalidResponse)?;
    if let Some(cached) = entries.get_mut(&hostname) {
        if cached.expires > now {
            cached.last_used = now;
            return Ok(cached.addresses.clone());
        }
    }
    entries.remove(&hostname);
    drop(entries);

    let (v4, v6) = thread::scope(|scope| {
        let v4 = scope.spawn(|| {
            let _permit = query_limit.acquire();
            query_google_json(client, &hostname, "A")
        });
        let v6 = scope.spawn(|| {
            let _permit = query_limit.acquire();
            query_google_json(client, &hostname, "AAAA")
        });
        (
            v4.join().unwrap_or(Err(GoogleDohFailure::InvalidResponse)),
            v6.join().unwrap_or(Err(GoogleDohFailure::InvalidResponse)),
        )
    });
    let (addresses, ttl) = aggregate_google_answers([v4, v6])?;
    // DNS TTLs are upper bounds: do not extend short CNAME/address lifetimes
    // to a convenience minimum, which can keep stale answers after a change.
    let ttl = ttl.min(CACHE_MAX_AGE);
    let now = Instant::now();
    let mut entries = cache
        .lock()
        .map_err(|_| GoogleDohFailure::InvalidResponse)?;
    prune_dns_cache(&mut entries, now);
    entries.insert(
        hostname,
        CachedAnswers {
            addresses: addresses.clone(),
            expires: now + ttl,
            last_used: now,
        },
    );
    Ok(addresses)
}

fn aggregate_google_answers(
    results: [Result<(Vec<IpAddr>, Duration), GoogleDohFailure>; 2],
) -> Result<(Vec<IpAddr>, Duration), GoogleDohFailure> {
    let mut addresses = Vec::new();
    let mut ttl = CACHE_MAX_AGE;
    let mut failures = Vec::new();
    for result in results {
        match result {
            Ok((found, result_ttl)) => {
                addresses.extend(found);
                ttl = ttl.min(result_ttl);
            }
            Err(failure) => failures.push(failure),
        }
    }
    addresses.sort_by_key(|address| (u8::from(address.is_ipv4()), *address));
    addresses.dedup();
    if addresses.is_empty() {
        if failures.len() == 2
            && failures
                .iter()
                .all(|failure| *failure == GoogleDohFailure::Transient)
        {
            return Err(GoogleDohFailure::Transient);
        }
        if failures.contains(&GoogleDohFailure::NameNotFound) {
            return Err(GoogleDohFailure::NameNotFound);
        }
        if failures.contains(&GoogleDohFailure::NoData) {
            return Err(GoogleDohFailure::NoData);
        }
        return Err(GoogleDohFailure::InvalidResponse);
    }
    Ok((addresses, ttl))
}

fn resolve_with_fallback(
    host: &str,
    doh_resolver: impl FnOnce(&str) -> Result<Vec<IpAddr>, GoogleDohFailure>,
    system_resolver: impl FnOnce(&str) -> Result<Vec<IpAddr>, String>,
) -> Result<DnsResolution, DnsResolutionFailure> {
    match doh_resolver(host) {
        Ok(addresses) if !addresses.is_empty() => {
            return Ok(DnsResolution {
                addresses,
                source: DnsResolutionSource::GoogleDoh,
            });
        }
        Err(GoogleDohFailure::Transient) => {}
        Ok(_) | Err(_) => return Err(DnsResolutionFailure::GoogleDohRejected),
    }

    match system_resolver(host) {
        Ok(addresses) if !addresses.is_empty() => Ok(DnsResolution {
            addresses,
            source: DnsResolutionSource::SystemFallback,
        }),
        Ok(_) | Err(_) => Err(DnsResolutionFailure::BothResolversFailed),
    }
}

type ResolverJob = Box<dyn FnOnce() + Send + 'static>;

/// A fixed-size worker pool keeps OS DNS resolution from occupying every proxy handler when the
/// Windows resolver stalls. Timed-out queued work is canceled before it starts; already-running
/// OS calls are isolated to this small, bounded pool.
struct BoundedResolverPool {
    sender: SyncSender<ResolverJob>,
}

impl BoundedResolverPool {
    fn new(worker_count: usize, queue_capacity: usize) -> Result<Self, String> {
        let (sender, receiver) = mpsc::sync_channel::<ResolverJob>(queue_capacity);
        let receiver = Arc::new(Mutex::new(receiver));

        for worker_index in 0..worker_count {
            let receiver = Arc::clone(&receiver);
            thread::Builder::new()
                .name(format!("pip-system-dns-{worker_index}"))
                .spawn(move || loop {
                    let job = {
                        let receiver = receiver
                            .lock()
                            .unwrap_or_else(|poisoned| poisoned.into_inner());
                        receiver.recv()
                    };
                    match job {
                        Ok(job) => job(),
                        Err(_) => break,
                    }
                })
                .map_err(|error| format!("Could not start the Windows DNS worker: {error}"))?;
        }

        Ok(Self { sender })
    }

    fn run_with_timeout<T>(
        &self,
        timeout: Duration,
        work: impl FnOnce() -> Result<T, String> + Send + 'static,
    ) -> Result<T, String>
    where
        T: Send + 'static,
    {
        let cancelled = Arc::new(AtomicBool::new(false));
        let worker_cancelled = Arc::clone(&cancelled);
        let (result_sender, result_receiver) = mpsc::sync_channel(1);
        let job: ResolverJob = Box::new(move || {
            if worker_cancelled.load(Ordering::Acquire) {
                return;
            }
            let result = work();
            let _ = result_sender.send(result);
        });

        match self.sender.try_send(job) {
            Ok(()) => {}
            Err(TrySendError::Full(_)) => {
                return Err("Windows DNS fallback is busy".to_string());
            }
            Err(TrySendError::Disconnected(_)) => {
                return Err("Windows DNS fallback is unavailable".to_string());
            }
        }

        match result_receiver.recv_timeout(timeout) {
            Ok(result) => result,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                cancelled.store(true, Ordering::Release);
                Err("Windows DNS lookup timed out".to_string())
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                Err("Windows DNS worker stopped unexpectedly".to_string())
            }
        }
    }
}

static SYSTEM_DNS_RESOLVER: OnceLock<Result<BoundedResolverPool, String>> = OnceLock::new();

fn system_dns_resolver_pool() -> Result<&'static BoundedResolverPool, String> {
    match SYSTEM_DNS_RESOLVER.get_or_init(|| {
        BoundedResolverPool::new(MAX_SYSTEM_DNS_WORKERS, MAX_QUEUED_SYSTEM_DNS_LOOKUPS)
    }) {
        Ok(pool) => Ok(pool),
        Err(error) => Err(error.clone()),
    }
}

fn resolve_with_system_dns(host: &str) -> Result<Vec<IpAddr>, String> {
    let host = host.to_owned();
    system_dns_resolver_pool()?.run_with_timeout(SYSTEM_DNS_FALLBACK_TIMEOUT, move || {
        resolve_with_system_dns_blocking(&host)
    })
}

fn resolve_with_system_dns_blocking(host: &str) -> Result<Vec<IpAddr>, String> {
    let resolved = (host, 0)
        .to_socket_addrs()
        .map_err(|_| "Windows DNS lookup failed".to_string())?;
    let mut addresses: Vec<IpAddr> = resolved.map(|address| address.ip()).collect();
    addresses.sort_by_key(|address| (u8::from(address.is_ipv4()), *address));
    addresses.dedup();
    if addresses.is_empty() {
        return Err("Windows DNS returned no usable addresses".to_string());
    }
    Ok(addresses)
}

fn prune_dns_cache(cache: &mut HashMap<String, CachedAnswers>, now: Instant) {
    cache.retain(|_, answers| answers.expires > now);
    while cache.len() >= CACHE_MAX_ENTRIES {
        let least_recently_used = cache
            .iter()
            .min_by_key(|(_, answers)| answers.last_used)
            .map(|(hostname, _)| hostname.clone());
        if let Some(hostname) = least_recently_used {
            cache.remove(&hostname);
        } else {
            break;
        }
    }
}

fn query_google_json(
    client: &Client,
    host: &str,
    record_type: &str,
) -> Result<(Vec<IpAddr>, Duration), GoogleDohFailure> {
    let response = client
        .get("https://dns.google/resolve")
        .query(&[("name", host), ("type", record_type)])
        .header("accept", "application/dns-json")
        .send()
        .map_err(|_| GoogleDohFailure::Transient)?
        .error_for_status()
        .map_err(|_| GoogleDohFailure::Transient)?;
    let result: Value = response
        .json()
        .map_err(|_| GoogleDohFailure::InvalidResponse)?;
    let record_code = match record_type {
        "A" => 1,
        "AAAA" => 28,
        _ => return Err(GoogleDohFailure::InvalidResponse),
    };
    parse_google_json_answers(&result, record_code)
}

fn parse_google_json_answers(
    result: &Value,
    requested_record_code: u64,
) -> Result<(Vec<IpAddr>, Duration), GoogleDohFailure> {
    match result.get("Status").and_then(Value::as_u64) {
        Some(0) => {}
        Some(2) => return Err(GoogleDohFailure::Transient),
        Some(3) => return Err(GoogleDohFailure::NameNotFound),
        _ => return Err(GoogleDohFailure::InvalidResponse),
    }
    let mut addresses = Vec::new();
    let mut ttl = CACHE_MAX_AGE;
    if let Some(answers) = result.get("Answer").and_then(Value::as_array) {
        for answer in answers {
            let record_code = answer.get("type").and_then(Value::as_u64);
            // The alias lifetime constrains the cached address too. Ignoring
            // CNAME TTLs can keep an old target after the alias has changed.
            if record_code == Some(5) || record_code == Some(requested_record_code) {
                if let Some(seconds) = answer.get("TTL").and_then(Value::as_u64) {
                    ttl = ttl.min(Duration::from_secs(seconds));
                }
            }
            if record_code == Some(requested_record_code) {
                if let Some(ip) = answer
                    .get("data")
                    .and_then(Value::as_str)
                    .and_then(|data| data.parse::<IpAddr>().ok())
                    .filter(|ip| match requested_record_code {
                        1 => ip.is_ipv4(),
                        28 => ip.is_ipv6(),
                        _ => false,
                    })
                {
                    addresses.push(ip);
                }
            }
        }
    }
    if addresses.is_empty() {
        Err(GoogleDohFailure::NoData)
    } else {
        Ok((addresses, ttl))
    }
}

fn respond_error(stream: &mut TcpStream, status: &str) {
    let body = format!("Pip DNS proxy: {status}\n");
    let _ = write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
    );
}

fn tunnel(mut client: TcpStream, mut upstream: TcpStream) {
    let Ok(mut client_reader) = client.try_clone() else {
        return;
    };
    let Ok(mut upstream_writer) = upstream.try_clone() else {
        return;
    };
    let outbound = thread::spawn(move || {
        let result = std::io::copy(&mut client_reader, &mut upstream_writer);
        let _ = upstream_writer.shutdown(std::net::Shutdown::Write);
        result
    });
    let _ = std::io::copy(&mut upstream, &mut client);
    let _ = client.shutdown(std::net::Shutdown::Write);
    let _ = outbound.join();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bounded_system_resolver_times_out_and_rejects_work_when_its_queue_is_full() {
        let pool = Arc::new(BoundedResolverPool::new(1, 1).unwrap());
        let (started_sender, started_receiver) = mpsc::channel();
        let (release_sender, release_receiver) = mpsc::sync_channel(1);
        let first_pool = pool.clone();
        let first = thread::spawn(move || {
            first_pool.run_with_timeout(Duration::from_secs(2), move || {
                started_sender.send(()).unwrap();
                release_receiver.recv().unwrap();
                Ok::<_, String>(1)
            })
        });
        started_receiver
            .recv_timeout(Duration::from_secs(1))
            .expect("the first resolver job should start");

        let queued_job_started = Arc::new(AtomicBool::new(false));
        let queued_job_started_in_worker = Arc::clone(&queued_job_started);
        let queued_result = pool.run_with_timeout(Duration::from_millis(20), move || {
            queued_job_started_in_worker.store(true, Ordering::Release);
            Ok::<_, String>(2)
        });
        assert!(queued_result.unwrap_err().contains("timed out"));

        let overloaded_result =
            pool.run_with_timeout(Duration::from_millis(20), || Ok::<_, String>(3));
        assert!(overloaded_result.unwrap_err().contains("busy"));

        release_sender.send(()).unwrap();
        assert_eq!(first.join().unwrap().unwrap(), 1);

        let deadline = Instant::now() + Duration::from_secs(1);
        let probe_result = loop {
            match pool.run_with_timeout(Duration::from_millis(500), || Ok::<_, String>(4)) {
                Err(error) if error.contains("busy") && Instant::now() < deadline => {
                    thread::sleep(Duration::from_millis(5));
                }
                result => break result,
            }
        };
        assert_eq!(probe_result.unwrap(), 4);
        assert!(!queued_job_started.load(Ordering::Acquire));
    }

    #[test]
    fn resolver_fallback_google_doh_success_does_not_use_system_dns() {
        let system_calls = AtomicUsize::new(0);
        let resolution = resolve_with_fallback(
            "primary.fixture.test",
            |_| Ok(vec!["192.0.2.10".parse().unwrap()]),
            |_| {
                system_calls.fetch_add(1, Ordering::Relaxed);
                Ok(vec!["192.0.2.11".parse().unwrap()])
            },
        )
        .unwrap();

        assert_eq!(resolution.source, DnsResolutionSource::GoogleDoh);
        assert_eq!(
            resolution.addresses,
            vec!["192.0.2.10".parse::<IpAddr>().unwrap()]
        );
        assert_eq!(system_calls.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn resolver_fallback_transient_google_doh_error_uses_system_dns() {
        let resolution = resolve_with_fallback(
            "fallback.fixture.test",
            |_| Err(GoogleDohFailure::Transient),
            |_| Ok(vec!["192.0.2.12".parse().unwrap()]),
        )
        .unwrap();

        assert_eq!(resolution.source, DnsResolutionSource::SystemFallback);
        assert_eq!(
            resolution.addresses,
            vec!["192.0.2.12".parse::<IpAddr>().unwrap()]
        );
    }

    #[test]
    fn resolver_fallback_does_not_leak_authoritative_or_invalid_results_to_system_dns() {
        for failure in [
            GoogleDohFailure::NameNotFound,
            GoogleDohFailure::NoData,
            GoogleDohFailure::InvalidResponse,
        ] {
            let system_calls = AtomicUsize::new(0);
            let resolution = resolve_with_fallback(
                "negative.fixture.test",
                |_| Err(failure),
                |_| {
                    system_calls.fetch_add(1, Ordering::Relaxed);
                    Ok(vec!["192.0.2.13".parse().unwrap()])
                },
            );

            assert_eq!(resolution, Err(DnsResolutionFailure::GoogleDohRejected));
            assert_eq!(system_calls.load(Ordering::Relaxed), 0);
        }
    }

    #[test]
    fn google_doh_aggregation_only_falls_back_when_both_record_queries_are_transient() {
        let cases = [
            (
                [
                    Err(GoogleDohFailure::Transient),
                    Err(GoogleDohFailure::Transient),
                ],
                Err(GoogleDohFailure::Transient),
            ),
            (
                [
                    Err(GoogleDohFailure::NameNotFound),
                    Err(GoogleDohFailure::Transient),
                ],
                Err(GoogleDohFailure::NameNotFound),
            ),
            (
                [
                    Err(GoogleDohFailure::NoData),
                    Err(GoogleDohFailure::Transient),
                ],
                Err(GoogleDohFailure::NoData),
            ),
        ];

        for (results, expected) in cases {
            assert_eq!(aggregate_google_answers(results), expected);
        }

        let addresses = aggregate_google_answers([
            Ok((vec!["192.0.2.14".parse().unwrap()], Duration::from_secs(30))),
            Err(GoogleDohFailure::Transient),
        ])
        .unwrap();
        assert_eq!(addresses.0, vec!["192.0.2.14".parse::<IpAddr>().unwrap()]);
    }

    #[test]
    fn resolver_fallback_total_failure_rejects_that_request() {
        let failed = resolve_with_fallback(
            "failed.fixture.test",
            |_| Err(GoogleDohFailure::Transient),
            |_| Err("system DNS unavailable".into()),
        );
        assert_eq!(failed, Err(DnsResolutionFailure::BothResolversFailed));

        let empty =
            resolve_with_fallback("empty.fixture.test", |_| Ok(Vec::new()), |_| Ok(Vec::new()));
        assert_eq!(empty, Err(DnsResolutionFailure::GoogleDohRejected));
    }

    #[test]
    fn proxy_can_reach_origin_when_google_doh_fails_and_system_dns_recovers() {
        let origin = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let origin_port = origin.local_addr().unwrap().port();
        let origin_thread = thread::spawn(move || {
            let (mut stream, _) = origin.accept().unwrap();
            let request = read_request_head(&mut stream).unwrap();
            assert!(String::from_utf8(request)
                .unwrap()
                .starts_with("GET /offline-recovery HTTP/1.1\r\n"));
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
                .unwrap();
        });

        let proxy = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let proxy_port = proxy.local_addr().unwrap().port();
        let used_fallback = Arc::new(AtomicBool::new(false));
        let server_used_fallback = Arc::clone(&used_fallback);
        let proxy_thread = thread::spawn(move || {
            let (stream, _) = proxy.accept().unwrap();
            handle_client_with_resolver(stream, move |host| {
                assert_eq!(host, "fallback.fixture.test");
                let resolution = resolve_with_fallback(
                    host,
                    |_| Err(GoogleDohFailure::Transient),
                    |_| Ok(vec!["127.0.0.1".parse().unwrap()]),
                )
                .ok()?;
                server_used_fallback.store(
                    resolution.source == DnsResolutionSource::SystemFallback,
                    Ordering::Release,
                );
                Some(resolution.addresses)
            });
        });

        let mut client = TcpStream::connect(("127.0.0.1", proxy_port)).unwrap();
        client
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        write!(
            client,
            "GET http://fallback.fixture.test:{origin_port}/offline-recovery HTTP/1.1\r\nHost: fallback.fixture.test:{origin_port}\r\n\r\n"
        )
        .unwrap();
        let mut response = String::new();
        client.read_to_string(&mut response).unwrap();

        proxy_thread.join().unwrap();
        origin_thread.join().unwrap();
        assert!(used_fallback.load(Ordering::Acquire));
        assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
        assert!(response.ends_with("ok"), "{response}");
    }

    fn proxy_response_with_resolver(
        request: &str,
        hostname: &'static str,
        resolver_result: Option<Vec<IpAddr>>,
    ) -> (String, usize) {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let resolver_calls = Arc::new(AtomicUsize::new(0));
        let server_calls = resolver_calls.clone();
        let server = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            handle_client_with_resolver(stream, move |requested_host| {
                assert_eq!(requested_host, hostname);
                server_calls.fetch_add(1, Ordering::Relaxed);
                resolver_result.clone()
            });
        });

        let mut client = TcpStream::connect(("127.0.0.1", port)).unwrap();
        client
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        client.write_all(request.as_bytes()).unwrap();
        let mut response = String::new();
        client.read_to_string(&mut response).unwrap();
        server.join().unwrap();
        let calls = resolver_calls.load(Ordering::Relaxed);
        (response, calls)
    }

    fn assert_unresolvable_hostname_rejected_without_system_dns_fallback(
        request: &str,
        hostname: &'static str,
        resolver_result: Option<Vec<IpAddr>>,
    ) {
        let (response, resolver_calls) =
            proxy_response_with_resolver(request, hostname, resolver_result);
        assert!(
            response.starts_with("HTTP/1.1 502 Bad Gateway"),
            "{response}"
        );
        assert!(
            response.contains("Pip DNS proxy: 502 Bad Gateway"),
            "{response}"
        );
        // This injected resolver is the only hostname-resolution seam in the
        // handler. A failed result is rejected instead of trying OS DNS.
        assert_eq!(resolver_calls, 1);
    }

    #[test]
    fn reqwest_optional_proxy_routes_requests_only_when_configured() {
        let proxy_listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let proxy_port = proxy_listener.local_addr().unwrap().port();
        let proxy_thread = thread::spawn(move || {
            let (mut stream, _) = proxy_listener.accept().unwrap();
            let request = read_request_head(&mut stream).unwrap();
            assert!(String::from_utf8(request)
                .unwrap()
                .starts_with("GET http://example.test/proxy HTTP/1.1\r\n"));
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
                .unwrap();
        });
        let client = with_google_dns_proxy(
            Client::builder().timeout(Duration::from_secs(3)),
            false,
            Some(proxy_port),
        )
        .unwrap()
        .build()
        .unwrap();
        let response = client.get("http://example.test/proxy").send().unwrap();
        assert_eq!(response.status(), 200);
        assert_eq!(response.text().unwrap(), "ok");
        proxy_thread.join().unwrap();

        let origin_listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let origin_port = origin_listener.local_addr().unwrap().port();
        let origin_thread = thread::spawn(move || {
            let (mut stream, _) = origin_listener.accept().unwrap();
            let request = read_request_head(&mut stream).unwrap();
            assert!(String::from_utf8(request)
                .unwrap()
                .starts_with("GET /direct HTTP/1.1\r\n"));
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
                .unwrap();
        });
        let client = with_optional_proxy(Client::builder().timeout(Duration::from_secs(3)), None)
            .unwrap()
            .build()
            .unwrap();
        let response = client
            .get(format!("http://127.0.0.1:{origin_port}/direct"))
            .send()
            .unwrap();
        assert_eq!(response.status(), 200);
        assert_eq!(response.text().unwrap(), "ok");
        origin_thread.join().unwrap();
    }

    #[test]
    fn enabled_google_dns_never_builds_an_unproxied_background_client() {
        let result = with_google_dns_proxy(Client::builder(), true, None);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .contains("local DNS proxy is unavailable"));

        // DNS off preserves the existing direct-network behavior.
        assert!(with_google_dns_proxy(Client::builder(), false, None).is_ok());
    }

    #[test]
    fn rejecting_proxy_returns_a_local_error_for_webview_requests() {
        let proxy = RejectingProxy::start().expect("start local rejection proxy");
        let mut client = TcpStream::connect(("127.0.0.1", proxy.port())).unwrap();
        client
            .write_all(b"CONNECT example.com:443 HTTP/1.1\r\nHost: example.com:443\r\n\r\n")
            .unwrap();
        let mut response = String::new();
        client.read_to_string(&mut response).unwrap();
        assert!(response.starts_with("HTTP/1.1 503 Service Unavailable"));
        assert!(response.contains("Pip DNS proxy: 503 Service Unavailable"));
    }

    #[test]
    fn reqwest_optional_proxy_routes_https_through_connect_tunnel() {
        let proxy_listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let proxy_port = proxy_listener.local_addr().unwrap().port();
        let proxy_thread = thread::spawn(move || {
            let (mut stream, _) = proxy_listener.accept().unwrap();
            let request = read_request_head(&mut stream).unwrap();
            assert!(String::from_utf8(request)
                .unwrap()
                .starts_with("CONNECT dns-route-test.invalid:443 HTTP/1.1\r\n"));
            stream
                .write_all(
                    b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                )
                .unwrap();
        });

        let client = with_google_dns_proxy(
            Client::builder().timeout(Duration::from_secs(3)),
            true,
            Some(proxy_port),
        )
        .unwrap()
        .build()
        .unwrap();
        let result = client.get("https://dns-route-test.invalid/").send();
        assert!(
            result.is_err(),
            "the test proxy deliberately rejects CONNECT"
        );
        proxy_thread.join().unwrap();
    }

    #[test]
    fn enabled_google_dns_with_unavailable_proxy_never_connects_to_origin_directly() {
        let dead_proxy = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let dead_proxy_port = dead_proxy.local_addr().unwrap().port();
        drop(dead_proxy);

        let origin = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        origin.set_nonblocking(true).unwrap();
        let origin_port = origin.local_addr().unwrap().port();
        let client = with_google_dns_proxy(
            Client::builder().timeout(Duration::from_secs(2)),
            true,
            Some(dead_proxy_port),
        )
        .unwrap()
        .build()
        .unwrap();

        let result = client
            .get(format!("http://127.0.0.1:{origin_port}/must-not-bypass"))
            .send();
        assert!(
            result.is_err(),
            "unavailable local proxy must fail the request"
        );
        assert!(
            matches!(
                origin.accept(),
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock
            ),
            "the request must not reach the origin directly when Google DNS is enabled"
        );
    }

    #[test]
    fn parses_connect_and_ipv6_authorities() {
        assert_eq!(
            parse_authority("example.com:443", 80),
            Some(("example.com".into(), 443))
        );
        assert_eq!(
            parse_authority("[2001:4860:4860::8888]:8443", 80),
            Some(("2001:4860:4860::8888".into(), 8443))
        );
        assert_eq!(parse_authority("[::1]", 80), Some(("::1".into(), 80)));
    }

    #[test]
    fn rejects_malformed_or_ambiguous_proxy_authorities() {
        for authority in [
            "",
            "example.com:",
            "example.com:not-a-port",
            "example.com:443:8443",
            "2001:db8::1:443",
            "[::1]junk",
            "[::1]:",
            "[::1]:port",
            "[not-ipv6]:443",
            "user@example.com:443",
            "example.com\r\nX-Injected: yes",
        ] {
            assert_eq!(
                parse_authority(authority, 80),
                None,
                "malformed authority should be rejected: {authority:?}"
            );
        }
    }

    #[test]
    fn parses_absolute_http_urls_with_query_only_path_without_polluting_host() {
        assert_eq!(
            parse_http_target(
                "http://example.test?campaign=casino",
                "GET http://example.test?campaign=casino HTTP/1.1\r\nHost: example.test\r\n"
            ),
            Some(("example.test".into(), 80, "/?campaign=casino".into()))
        );
        assert_eq!(
            parse_http_target(
                "http://example.test#ignored",
                "GET http://example.test#ignored HTTP/1.1\r\nHost: example.test\r\n"
            ),
            Some(("example.test".into(), 80, "/".into()))
        );
    }

    #[test]
    fn google_dns_answer_cache_ttl_includes_cname_and_uses_requested_record_type() {
        let response = serde_json::json!({
            "Status": 0,
            "Answer": [
                { "name": "alias.example.", "type": 5, "TTL": 3, "data": "target.example." },
                { "name": "target.example.", "type": 1, "TTL": 120, "data": "192.0.2.7" },
                { "name": "target.example.", "type": 28, "TTL": 1, "data": "2001:db8::7" }
            ]
        });

        let (addresses, ttl) = parse_google_json_answers(&response, 1).unwrap();
        assert_eq!(addresses, vec!["192.0.2.7".parse::<IpAddr>().unwrap()]);
        assert_eq!(ttl, Duration::from_secs(3));

        let (addresses, ttl) = parse_google_json_answers(&response, 28).unwrap();
        assert_eq!(addresses, vec!["2001:db8::7".parse::<IpAddr>().unwrap()]);
        assert_eq!(ttl, Duration::from_secs(1));

        let zero_ttl_response = serde_json::json!({
            "Status": 0,
            "Answer": [
                { "name": "alias.example.", "type": 5, "TTL": 0, "data": "target.example." },
                { "name": "target.example.", "type": 1, "TTL": 120, "data": "192.0.2.7" }
            ]
        });
        let (_, ttl) = parse_google_json_answers(&zero_ttl_response, 1).unwrap();
        assert_eq!(ttl, Duration::ZERO);
    }

    #[test]
    fn google_dns_answer_parser_rejects_unsuccessful_dns_status() {
        let response = serde_json::json!({
            "Status": 3,
            "Answer": [
                { "name": "missing.example.", "type": 1, "TTL": 60, "data": "192.0.2.7" }
            ]
        });
        assert_eq!(
            parse_google_json_answers(&response, 1),
            Err(GoogleDohFailure::NameNotFound)
        );

        let server_failure = serde_json::json!({ "Status": 2 });
        assert_eq!(
            parse_google_json_answers(&server_failure, 1),
            Err(GoogleDohFailure::Transient)
        );

        let refused = serde_json::json!({ "Status": 5 });
        assert_eq!(
            parse_google_json_answers(&refused, 1),
            Err(GoogleDohFailure::InvalidResponse)
        );
    }

    #[test]
    fn google_dns_answer_parser_classifies_no_data_without_system_dns_fallback() {
        let response = serde_json::json!({
            "Status": 0,
            "Question": [{ "name": "ipv6-only.example.", "type": 1 }],
            "Authority": [{ "name": "example.", "type": 6, "TTL": 300 }]
        });
        assert_eq!(
            parse_google_json_answers(&response, 1),
            Err(GoogleDohFailure::NoData)
        );
    }

    #[test]
    fn request_head_reader_keeps_pipelined_body_bytes_after_terminator() {
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let address = listener.local_addr().unwrap();
        let writer = thread::spawn(move || {
            let mut stream = TcpStream::connect(address).unwrap();
            stream
                .write_all(b"POST http://example.test/upload HTTP/1.1\r\nHost: example.test\r\nContent-Length: 4\r\n\r\nbody")
                .unwrap();
        });
        let (mut stream, _) = listener.accept().unwrap();
        let head = read_request_head(&mut stream).unwrap();
        assert!(head.ends_with(b"\r\n\r\n"));
        let mut body = [0_u8; 4];
        stream.read_exact(&mut body).unwrap();
        assert_eq!(&body, b"body");
        writer.join().unwrap();
    }

    #[test]
    fn parses_absolute_and_origin_form_http_targets() {
        let absolute = "GET http://example.com:8080/path?q=1 HTTP/1.1\r\nHost: example.com\r\n\r\n";
        assert_eq!(
            parse_http_target("http://example.com:8080/path?q=1", absolute),
            Some(("example.com".into(), 8080, "/path?q=1".into()))
        );
        let origin = "GET /path?q=1 HTTP/1.1\r\nHost: example.com\r\n\r\n";
        assert_eq!(
            parse_http_target("/path?q=1", origin),
            Some(("example.com".into(), 80, "/path?q=1".into()))
        );
    }

    #[test]
    fn dns_cache_evicts_expired_and_least_recently_used_entries() {
        let now = Instant::now();
        let mut cache = HashMap::new();
        cache.insert(
            "expired.example".to_string(),
            CachedAnswers {
                addresses: vec!["192.0.2.1".parse().unwrap()],
                expires: now - Duration::from_secs(1),
                last_used: now,
            },
        );
        for index in 0..CACHE_MAX_ENTRIES {
            cache.insert(
                format!("host-{index}.example"),
                CachedAnswers {
                    addresses: vec!["192.0.2.2".parse().unwrap()],
                    expires: now + CACHE_MAX_AGE,
                    last_used: now + Duration::from_secs(index as u64),
                },
            );
        }

        prune_dns_cache(&mut cache, now);
        assert_eq!(cache.len(), CACHE_MAX_ENTRIES - 1);
        assert!(!cache.contains_key("expired.example"));
        assert!(!cache.contains_key("host-0.example"));
        assert!(cache.contains_key(&format!("host-{}.example", CACHE_MAX_ENTRIES - 1)));
    }

    #[test]
    #[ignore = "requires outbound HTTPS to Google Public DNS"]
    fn google_doh_resolves_public_hosts() {
        let resolver_client = Client::builder()
            .resolve_to_addrs(
                "dns.google",
                &[
                    "8.8.8.8:443".parse().unwrap(),
                    "8.8.4.4:443".parse().unwrap(),
                ],
            )
            .build()
            .unwrap();
        let addresses = resolve_google_doh(
            "example.com",
            &resolver_client,
            &Arc::new(Mutex::new(HashMap::new())),
            &Arc::new(DohQueryLimit::new(MAX_CONCURRENT_DOH_QUERIES)),
        )
        .unwrap();
        assert!(!addresses.is_empty());
    }

    #[test]
    #[ignore = "requires outbound HTTPS through the local proxy to Google"]
    fn google_doh_proxy_reaches_google_using_its_own_resolution_route() {
        let proxy = DohProxy::start().expect("start local DoH proxy");
        test_google_doh_through_proxy(proxy.port()).expect("fetch Google through proxy");
    }

    #[test]
    fn local_proxy_tunnels_connect_requests() {
        let origin = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let origin_port = origin.local_addr().unwrap().port();
        let origin_thread = thread::spawn(move || {
            let (mut stream, _) = origin.accept().unwrap();
            let mut input = [0; 4];
            stream.read_exact(&mut input).unwrap();
            assert_eq!(&input, b"ping");
            stream.write_all(b"pong").unwrap();
        });
        let proxy = DohProxy::start().unwrap();
        let mut client = TcpStream::connect(("127.0.0.1", proxy.port())).unwrap();
        write!(
            client,
            "CONNECT 127.0.0.1:{origin_port} HTTP/1.1\r\nHost: 127.0.0.1:{origin_port}\r\n\r\n"
        )
        .unwrap();
        let mut response = Vec::new();
        let mut byte = [0; 1];
        while !response.ends_with(b"\r\n\r\n") {
            client.read_exact(&mut byte).unwrap();
            response.push(byte[0]);
        }
        assert!(String::from_utf8(response)
            .unwrap()
            .starts_with("HTTP/1.1 200"));
        client.write_all(b"ping").unwrap();
        let mut answer = [0; 4];
        client.read_exact(&mut answer).unwrap();
        assert_eq!(&answer, b"pong");
        origin_thread.join().unwrap();
    }

    #[test]
    fn local_proxy_resolves_connect_hostname_before_tunneling() {
        let origin = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let origin_port = origin.local_addr().unwrap().port();
        let origin_thread = thread::spawn(move || {
            let (mut stream, _) = origin.accept().unwrap();
            let mut input = [0; 4];
            stream.read_exact(&mut input).unwrap();
            assert_eq!(&input, b"ping");
            stream.write_all(b"pong").unwrap();
        });

        let proxy = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let proxy_port = proxy.local_addr().unwrap().port();
        let proxy_thread = thread::spawn(move || {
            let (stream, _) = proxy.accept().unwrap();
            handle_client_with_resolver(stream, |host| {
                assert_eq!(host, "dns.fixture.test");
                Some(vec!["127.0.0.1".parse().unwrap()])
            });
        });

        let mut client = TcpStream::connect(("127.0.0.1", proxy_port)).unwrap();
        write!(
            client,
            "CONNECT dns.fixture.test:{origin_port} HTTP/1.1\r\nHost: dns.fixture.test:{origin_port}\r\n\r\n"
        )
        .unwrap();
        let mut response = Vec::new();
        let mut byte = [0; 1];
        while !response.ends_with(b"\r\n\r\n") {
            client.read_exact(&mut byte).unwrap();
            response.push(byte[0]);
        }
        assert!(String::from_utf8(response)
            .unwrap()
            .starts_with("HTTP/1.1 200"));
        client.write_all(b"ping").unwrap();
        let mut answer = [0; 4];
        client.read_exact(&mut answer).unwrap();
        assert_eq!(&answer, b"pong");
        drop(client);
        proxy_thread.join().unwrap();
        origin_thread.join().unwrap();
    }

    #[test]
    fn connect_resolution_failure_and_empty_answers_return_502_without_system_dns_fallback() {
        let hostname = "pip-dns-failure.invalid";
        let request = format!("CONNECT {hostname}:443 HTTP/1.1\r\nHost: {hostname}:443\r\n\r\n");

        for resolver_result in [None, Some(Vec::new())] {
            assert_unresolvable_hostname_rejected_without_system_dns_fallback(
                &request,
                hostname,
                resolver_result,
            );
        }
    }

    #[test]
    fn plain_http_resolution_failure_and_empty_answers_return_502_without_system_dns_fallback() {
        let hostname = "pip-dns-failure.invalid";
        let request = format!("GET http://{hostname}/ HTTP/1.1\r\nHost: {hostname}\r\n\r\n");

        for resolver_result in [None, Some(Vec::new())] {
            assert_unresolvable_hostname_rejected_without_system_dns_fallback(
                &request,
                hostname,
                resolver_result,
            );
        }
    }

    #[test]
    fn local_proxy_forwards_plain_http_requests() {
        let origin = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let origin_port = origin.local_addr().unwrap().port();
        let origin_thread = thread::spawn(move || {
            let (mut stream, _) = origin.accept().unwrap();
            let request = read_request_head(&mut stream).unwrap();
            assert!(String::from_utf8(request)
                .unwrap()
                .contains("GET /health HTTP/1.1"));
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
                .unwrap();
        });
        let proxy = DohProxy::start().unwrap();
        let mut client = TcpStream::connect(("127.0.0.1", proxy.port())).unwrap();
        write!(
            client,
            "GET http://127.0.0.1:{origin_port}/health HTTP/1.1\r\nHost: 127.0.0.1:{origin_port}\r\n\r\n"
        )
        .unwrap();
        let mut response = String::new();
        client.read_to_string(&mut response).unwrap();
        assert!(response.starts_with("HTTP/1.1 200 OK"));
        assert!(response.ends_with("ok"));
        origin_thread.join().unwrap();
    }

    #[test]
    fn local_proxy_caps_active_handlers_and_recovers_after_overload() {
        const EXTRA_CLIENTS: usize = 12;
        let proxy = DohProxy::start().unwrap();
        let mut stalled_clients = (0..MAX_ACTIVE_PROXY_HANDLERS + EXTRA_CLIENTS)
            .map(|_| TcpStream::connect(("127.0.0.1", proxy.port())).unwrap())
            .collect::<Vec<_>>();
        for client in &mut stalled_clients {
            // Incomplete request headers hold each accepted handler in its bounded read.
            client.write_all(b"G").unwrap();
        }

        let deadline = Instant::now() + Duration::from_secs(5);
        while proxy.accepted_connection_count() < stalled_clients.len() as u64
            && Instant::now() < deadline
        {
            thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(
            proxy.accepted_connection_count(),
            stalled_clients.len() as u64,
            "listener should accept and reject overflow sockets without spawning handlers"
        );
        let deadline = Instant::now() + Duration::from_secs(3);
        while proxy.active_handler_count() < MAX_ACTIVE_PROXY_HANDLERS && Instant::now() < deadline
        {
            thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(
            proxy.active_handler_count(),
            MAX_ACTIVE_PROXY_HANDLERS,
            "active client handler count must never exceed its hard cap"
        );

        drop(stalled_clients);
        let deadline = Instant::now() + Duration::from_secs(3);
        while proxy.active_handler_count() != 0 && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(proxy.active_handler_count(), 0);

        // Released permits allow ordinary traffic to proceed after overload.
        let origin = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let origin_port = origin.local_addr().unwrap().port();
        let origin_thread = thread::spawn(move || {
            let (mut stream, _) = origin.accept().unwrap();
            let _ = read_request_head(&mut stream).unwrap();
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok")
                .unwrap();
        });
        let mut client = TcpStream::connect(("127.0.0.1", proxy.port())).unwrap();
        write!(
            client,
            "GET http://127.0.0.1:{origin_port}/after-overload HTTP/1.1\r\nHost: 127.0.0.1:{origin_port}\r\n\r\n"
        )
        .unwrap();
        let mut response = String::new();
        client.read_to_string(&mut response).unwrap();
        assert!(response.starts_with("HTTP/1.1 200 OK"));
        assert!(response.ends_with("ok"));
        origin_thread.join().unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        while proxy.active_handler_count() != 0 && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(proxy.active_handler_count(), 0);

        let shutdown_started = Instant::now();
        drop(proxy);
        assert!(
            shutdown_started.elapsed() < Duration::from_secs(1),
            "proxy shutdown should wake and join its listener promptly"
        );
    }

    #[test]
    fn google_doh_query_limit_bounds_parallel_requests() {
        const WORKERS: usize = MAX_CONCURRENT_DOH_QUERIES * 4;
        let limit = Arc::new(DohQueryLimit::new(MAX_CONCURRENT_DOH_QUERIES));
        let peak = Arc::new(AtomicUsize::new(0));
        let rendezvous = Arc::new(std::sync::Barrier::new(MAX_CONCURRENT_DOH_QUERIES));
        let workers = (0..WORKERS)
            .map(|_| {
                let limit = limit.clone();
                let peak = peak.clone();
                let rendezvous = rendezvous.clone();
                thread::spawn(move || {
                    let _permit = limit.acquire();
                    let active = *limit.active.lock().unwrap();
                    peak.fetch_max(active, Ordering::Relaxed);
                    rendezvous.wait();
                })
            })
            .collect::<Vec<_>>();
        for worker in workers {
            worker.join().unwrap();
        }
        assert_eq!(peak.load(Ordering::Relaxed), MAX_CONCURRENT_DOH_QUERIES);
        assert_eq!(*limit.active.lock().unwrap(), 0);
    }
}
