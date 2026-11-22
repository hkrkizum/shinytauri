use std::{
    fs::File,
    io::{self, Read, Write},
    net::{Ipv4Addr, SocketAddr, TcpStream},
    path::Path,
    process::Child,
    thread,
    time::{Duration, Instant},
};

const POLL_INTERVAL: Duration = Duration::from_millis(100);
const MAX_HEADERS: usize = 32 * 1024;

fn remaining(deadline: Instant) -> io::Result<Duration> {
    let left = deadline.saturating_duration_since(Instant::now());
    if left.is_zero() {
        Err(io::Error::new(
            io::ErrorKind::TimedOut,
            "Shiny startup timed out",
        ))
    } else {
        Ok(left)
    }
}

fn child_running(child: &mut Child) -> io::Result<()> {
    match child.try_wait()? {
        None => Ok(()),
        Some(status) => Err(io::Error::other(format!(
            "R exited during startup ({status})"
        ))),
    }
}

fn ready_port(path: &Path) -> io::Result<u16> {
    let mut text = String::new();
    File::open(path)?.take(128).read_to_string(&mut text)?;
    let port = text
        .trim()
        .strip_prefix("http://127.0.0.1:")
        .map(|s| s.strip_suffix('/').unwrap_or(s))
        .filter(|s| s.bytes().all(|c| c.is_ascii_digit()))
        .and_then(|s| s.parse::<u16>().ok())
        .filter(|port| *port > 0);
    if text.len() >= 128 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "unexpected Shiny address",
        ));
    }
    port.ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "unexpected Shiny address"))
}

pub fn wait_for_port(path: &Path, child: &mut Child, deadline: Instant) -> io::Result<u16> {
    loop {
        child_running(child)?;
        remaining(deadline)?;
        match ready_port(path) {
            Ok(port) => return Ok(port),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                thread::sleep(POLL_INTERVAL.min(remaining(deadline)?));
            }
            Err(error) => return Err(error),
        }
    }
}

// Read only the response headers. A redirect or authentication challenge is a
// valid response to display in the WebView. Never follow it from the host.
fn http_probe(port: u16, deadline: Instant, child: &mut Child) -> io::Result<u16> {
    let address = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let mut stream = TcpStream::connect_timeout(&address, remaining(deadline)?.min(POLL_INTERVAL))?;
    stream.set_write_timeout(Some(remaining(deadline)?.min(POLL_INTERVAL)))?;
    write!(
        stream,
        "HEAD / HTTP/1.0\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\n\r\n"
    )?;

    let mut headers = Vec::new();
    let mut block = [0; 1024];
    loop {
        child_running(child)?;
        stream.set_read_timeout(Some(remaining(deadline)?.min(POLL_INTERVAL)))?;
        let mut search_from = headers.len().saturating_sub(3);
        match stream.read(&mut block) {
            Ok(0) => {
                return Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "incomplete HTTP response",
                ));
            }
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::WouldBlock
                        | io::ErrorKind::TimedOut
                        | io::ErrorKind::Interrupted
                ) =>
            {
                continue;
            }
            Err(e) => return Err(e),
            Ok(n) => headers.extend_from_slice(&block[..n]),
        }
        // Only rescan the tail crossing the previous read boundary.
        while headers[search_from..].windows(4).any(|w| w == b"\r\n\r\n") {
            let mut fields = [httparse::EMPTY_HEADER; 128];
            let mut response = httparse::Response::new(&mut fields);
            let parsed = response.parse(&headers).map_err(io::Error::other)?;
            match (parsed, response.code) {
                (httparse::Status::Complete(n), Some(100..=199)) => {
                    headers.drain(..n);
                    search_from = 0;
                }
                (httparse::Status::Complete(_), Some(status @ 200..=599)) => return Ok(status),
                _ => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "invalid HTTP response",
                    ));
                }
            }
        }
        if headers.len() >= MAX_HEADERS {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "HTTP response headers too large",
            ));
        }
    }
}

pub fn wait_for_http(port: u16, child: &mut Child, deadline: Instant) -> io::Result<u16> {
    loop {
        child_running(child)?;
        remaining(deadline)?;
        match http_probe(port, deadline, child) {
            Ok(status) => {
                child_running(child)?;
                remaining(deadline)?;
                return Ok(status);
            }
            Err(error) => {
                child_running(child)?;
                if remaining(deadline).is_err() {
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        format!("Shiny HTTP startup timed out: {error}"),
                    ));
                }
                thread::sleep(POLL_INTERVAL.min(remaining(deadline)?));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        env, fs,
        net::TcpListener,
        process::{Command, Stdio},
    };

    #[test]
    #[ignore = "subprocess used to observe R-like lifetime"]
    fn hold_child() {
        let mut gate = String::new();
        io::stdin().read_line(&mut gate).unwrap();
    }

    // The child has the same start/exit controls on Linux and Windows.
    struct Running(Child);
    impl Running {
        fn new() -> Self {
            Self(
                Command::new(env::current_exe().unwrap())
                    .args([
                        "--exact",
                        "readiness::tests::hold_child",
                        "tests::hold_child",
                        "--ignored",
                    ])
                    .stdin(Stdio::piped())
                    .stdout(Stdio::null())
                    .spawn()
                    .unwrap(),
            )
        }
    }
    impl Drop for Running {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }

    #[test]
    fn rejects_unexpected_ready_addresses_and_reads_the_published_port() {
        let mut child = Running::new();
        let path = env::temp_dir().join(format!("shinytauri-ready-test-{}", std::process::id()));
        let deadline = Instant::now() + Duration::from_secs(5);
        for invalid in [
            "http://localhost:1234",
            "http://127.0.0.1:0",
            "http://127.0.0.1:65536",
            "http://127.0.0.1:1234/a",
            "http://127.0.0.1:1234//",
            "http://127.0.0.1:+123",
        ] {
            fs::write(&path, invalid).unwrap();
            assert_eq!(
                wait_for_port(&path, &mut child.0, deadline)
                    .unwrap_err()
                    .kind(),
                io::ErrorKind::InvalidData,
                "{invalid}"
            );
        }
        fs::write(&path, "http://127.0.0.1:4567/\r\n").unwrap();
        assert_eq!(wait_for_port(&path, &mut child.0, deadline).unwrap(), 4567);
        fs::remove_file(path).unwrap();
    }

    fn read_request(socket: &mut TcpStream) {
        let mut request = String::new();
        let mut byte = [0];
        while !request.ends_with("\r\n\r\n") {
            socket.read_exact(&mut byte).unwrap();
            request.push(char::from(byte[0]));
        }
        assert!(request.starts_with("HEAD / HTTP/1.0\r\nHost: 127.0.0.1:"));
    }

    #[test]
    fn retries_truncated_and_invalid_replies_and_accepts_auth_after_early_hints() {
        let mut child = Running::new();
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = thread::spawn(move || {
            for response in [
                "HTTP/1.1 200 OK\r\n",
                "not HTTP\r\n\r\n",
                "HTTP/1.1 103 Early Hints\r\n\r\nHTTP/1.1 401 Unauthorized\r\n\r\n",
            ] {
                let (mut stream, _) = listener.accept().unwrap();
                read_request(&mut stream);
                stream.write_all(response.as_bytes()).unwrap();
            }
        });
        assert_eq!(
            wait_for_http(port, &mut child.0, Instant::now() + Duration::from_secs(3)).unwrap(),
            401
        );
        server.join().unwrap();
    }

    #[test]
    fn follows_no_redirects_and_waits_for_no_body() {
        let mut child = Running::new();
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            read_request(&mut stream);
            stream.write_all(b"HTTP/1.0 302 Found\r\nLocation: https://example.invalid/\r\nContent-Length: 999\r\n\r\n").unwrap();
        });
        assert_eq!(
            wait_for_http(port, &mut child.0, Instant::now() + Duration::from_secs(3)).unwrap(),
            302
        );
        server.join().unwrap();
    }

    #[test]
    fn slow_trickle_cannot_extend_the_total_deadline() {
        let mut child = Running::new();
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            read_request(&mut stream);
            for &byte in b"HTTP/1.1 200 OK\r\nServer: slow\r\n\r\n" {
                if stream.write_all(&[byte]).is_err() {
                    break;
                }
                thread::sleep(Duration::from_millis(40));
            }
        });
        let start = Instant::now();
        assert_eq!(
            wait_for_http(port, &mut child.0, start + Duration::from_millis(250))
                .unwrap_err()
                .kind(),
            io::ErrorKind::TimedOut
        );
        assert!(start.elapsed() < Duration::from_secs(1));
        server.join().unwrap();
    }

    #[test]
    fn notices_child_exit_during_http_and_before_address_publication() {
        let mut child = Running::new();
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap();
        let port = listener.local_addr().unwrap().port();
        let gate = child.0.stdin.take().unwrap();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            read_request(&mut stream);
            drop(gate);
            thread::sleep(Duration::from_millis(600));
        });
        let deadline = Instant::now() + Duration::from_secs(4);
        assert!(
            wait_for_http(port, &mut child.0, deadline)
                .unwrap_err()
                .to_string()
                .contains("R exited")
        );
        assert!(
            wait_for_port(Path::new("unused"), &mut child.0, deadline)
                .unwrap_err()
                .to_string()
                .contains("R exited")
        );
        server.join().unwrap();
    }
}
