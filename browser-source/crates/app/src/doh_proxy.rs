//! Per-browser HTTP proxy that resolves origin hosts through Google's DoH JSON API.
//! It is bound to loopback and used only by Pip's content WebViews.

use reqwest::blocking::Client;
use serde_json::Value;
use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::{IpAddr, SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const MAX_HEADER_BYTES: usize = 64 * 1024;
const CACHE_MAX_AGE: Duration = Duration::from_secs(600);

pub struct DohProxy {
    port: u16,
    running: Arc<AtomicBool>,
    listener_thread: Option<JoinHandle<()>>,
}

impl DohProxy {
    pub fn start() -> Result<Self, String> {
        let listener = TcpListener::bind(("127.0.0.1", 0))
            .map_err(|error| format!("cannot bind local proxy: {error}"))?;
        listener
            .set_nonblocking(true)
            .map_err(|error| format!("cannot configure local proxy: {error}"))?;
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
        let running = Arc::new(AtomicBool::new(true));
        let thread_running = running.clone();
        let listener_thread = thread::Builder::new()
            .name("pip-google-dns-proxy".to_string())
            .spawn(move || {
                while thread_running.load(Ordering::Relaxed) {
                    match listener.accept() {
                        Ok((stream, _)) => {
                            let client = client.clone();
                            let cache = cache.clone();
                            let _ = thread::Builder::new()
                                .name("pip-google-dns-request".to_string())
                                .spawn(move || handle_client(stream, client, cache));
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(25));
                        }
                        Err(error) => {
                            eprintln!("Pip Google DNS proxy listener: {error}");
                            thread::sleep(Duration::from_millis(100));
                        }
                    }
                }
            })
            .map_err(|error| format!("cannot start local proxy: {error}"))?;

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

pub fn test_google_doh() -> Result<(), String> {
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
        .map_err(|error| error.to_string())?;
    let (addresses, _) = query_google_json(&client, "example.com", "A")?;
    if addresses.is_empty() {
        return Err("Google DNS returned no IPv4 addresses".to_string());
    }
    Ok(())
}

impl Drop for DohProxy {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
        if let Some(thread) = self.listener_thread.take() {
            let _ = thread.join();
        }
    }
}

struct CachedAnswers {
    addresses: Vec<IpAddr>,
    expires: Instant,
}

fn handle_client(
    mut client_stream: TcpStream,
    client: Client,
    cache: Arc<Mutex<HashMap<String, CachedAnswers>>>,
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
        let Some(upstream) = connect_to_host(&host, port, &client, &cache) else {
            respond_error(&mut client_stream, "502 Bad Gateway");
            return;
        };
        if client_stream
            .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
            .is_err()
        {
            return;
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
    let Some(mut upstream) = connect_to_host(&host, port, &client, &cache) else {
        respond_error(&mut client_stream, "502 Bad Gateway");
        return;
    };

    let mut lines = request.lines();
    let _ = lines.next();
    if writeln!(upstream, "{method} {path} HTTP/1.1").is_err() {
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
    let mut byte = [0_u8; 1];
    while bytes.len() < MAX_HEADER_BYTES {
        if stream.read_exact(&mut byte).is_err() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                "proxy request ended before headers completed",
            ));
        }
        bytes.push(byte[0]);
        if bytes.ends_with(b"\r\n\r\n") {
            return Ok(bytes);
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        "proxy request headers are too large",
    ))
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
    if let Some(rest) = authority.strip_prefix('[') {
        let end = rest.find(']')?;
        let host = rest[..end].to_string();
        let port = rest[end + 1..]
            .strip_prefix(':')
            .map(str::parse::<u16>)
            .transpose()
            .ok()?
            .unwrap_or(default_port);
        return Some((host, port));
    }
    match authority.rsplit_once(':') {
        Some((host, port)) if port.bytes().all(|byte| byte.is_ascii_digit()) => {
            Some((host.to_string(), port.parse().ok()?))
        }
        _ => Some((authority.to_string(), default_port)),
    }
}

fn parse_http_target(target: &str, request: &str) -> Option<(String, u16, String)> {
    if let Some(rest) = target.strip_prefix("http://") {
        let (authority, path) = rest
            .find('/')
            .map_or((rest, "/"), |index| (&rest[..index], &rest[index..]));
        let (host, port) = parse_authority(authority, 80)?;
        return Some((host, port, path.to_string()));
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
    client: &Client,
    cache: &Arc<Mutex<HashMap<String, CachedAnswers>>>,
) -> Option<TcpStream> {
    if let Ok(ip) = host.parse::<IpAddr>() {
        return TcpStream::connect_timeout(&SocketAddr::new(ip, port), Duration::from_secs(4)).ok();
    }
    let addresses = resolve_google_doh(host, client, cache).ok()?;
    addresses.into_iter().find_map(|ip| {
        TcpStream::connect_timeout(&SocketAddr::new(ip, port), Duration::from_secs(3)).ok()
    })
}

fn resolve_google_doh(
    host: &str,
    client: &Client,
    cache: &Arc<Mutex<HashMap<String, CachedAnswers>>>,
) -> Result<Vec<IpAddr>, String> {
    let hostname = host.trim_end_matches('.').to_ascii_lowercase();
    if hostname.is_empty() || hostname.chars().any(char::is_control) {
        return Err("invalid host name".to_string());
    }
    if let Ok(ip) = hostname.parse::<IpAddr>() {
        return Ok(vec![ip]);
    }
    if let Some(cached) = cache
        .lock()
        .map_err(|_| "DNS cache is unavailable".to_string())?
        .get(&hostname)
    {
        if cached.expires > Instant::now() {
            return Ok(cached.addresses.clone());
        }
    }

    let (v4, v6) = thread::scope(|scope| {
        let v4 = scope.spawn(|| query_google_json(client, &hostname, "A"));
        let v6 = scope.spawn(|| query_google_json(client, &hostname, "AAAA"));
        (
            v4.join()
                .unwrap_or_else(|_| Err("IPv4 DoH worker failed".into())),
            v6.join()
                .unwrap_or_else(|_| Err("IPv6 DoH worker failed".into())),
        )
    });
    let mut addresses = Vec::new();
    let mut ttl = CACHE_MAX_AGE;
    for (found, result_ttl) in [v4, v6].into_iter().flatten() {
        addresses.extend(found);
        ttl = ttl.min(result_ttl);
    }
    addresses.sort();
    addresses.dedup();
    if addresses.is_empty() {
        return Err("Google DNS returned no address".to_string());
    }
    ttl = ttl.min(CACHE_MAX_AGE).max(Duration::from_secs(5));
    cache
        .lock()
        .map_err(|_| "DNS cache is unavailable".to_string())?
        .insert(
            hostname,
            CachedAnswers {
                addresses: addresses.clone(),
                expires: Instant::now() + ttl,
            },
        );
    Ok(addresses)
}

fn query_google_json(
    client: &Client,
    host: &str,
    record_type: &str,
) -> Result<(Vec<IpAddr>, Duration), String> {
    let result: Value = client
        .get("https://dns.google/resolve")
        .query(&[("name", host), ("type", record_type)])
        .header("accept", "application/dns-json")
        .send()
        .and_then(reqwest::blocking::Response::error_for_status)
        .map_err(|error| error.to_string())?
        .json()
        .map_err(|error| error.to_string())?;
    let mut addresses = Vec::new();
    let mut ttl = CACHE_MAX_AGE;
    if let Some(answers) = result.get("Answer").and_then(Value::as_array) {
        for answer in answers {
            if let Some(ip) = answer
                .get("data")
                .and_then(Value::as_str)
                .and_then(|data| data.parse().ok())
            {
                addresses.push(ip);
                if let Some(seconds) = answer.get("TTL").and_then(Value::as_u64) {
                    ttl = ttl.min(Duration::from_secs(seconds));
                }
            }
        }
    }
    Ok((addresses, ttl))
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
    let Ok(mut client_to_upstream) = client.try_clone() else {
        return;
    };
    let Ok(mut upstream_to_client) = upstream.try_clone() else {
        return;
    };
    let outbound = thread::spawn(move || {
        let _ = std::io::copy(&mut client_to_upstream, &mut upstream);
        let _ = upstream.shutdown(std::net::Shutdown::Write);
    });
    let _ = std::io::copy(&mut upstream_to_client, &mut client);
    let _ = client.shutdown(std::net::Shutdown::Write);
    let _ = outbound.join();
}

#[cfg(test)]
mod tests {
    use super::*;

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
        )
        .unwrap();
        assert!(!addresses.is_empty());
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
}
