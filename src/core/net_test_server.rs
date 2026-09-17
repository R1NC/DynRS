//! A minimal HTTP server on `127.0.0.1` for the tests that exercise the client over a real
//! socket, so that they never leave the machine.

use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

/// A one-thread HTTP server on `127.0.0.1`. It answers every request with the same response and
/// keeps what it was asked, which is how a test can look at both halves of an exchange.
pub(crate) struct Server {
    port: u16,
    requests: Arc<Mutex<Vec<String>>>,
}

impl Server {
    pub(crate) fn start(response: String) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a local port is free");
        let port = listener
            .local_addr()
            .expect("a bound listener has an address")
            .port();
        let requests: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
        let recorded = Arc::clone(&requests);
        thread::spawn(move || {
            for mut stream in listener.incoming().flatten() {
                if let Ok(request) = read_request(&mut stream) {
                    recorded.lock().unwrap().push(request);
                }
                let _ = stream.write_all(response.as_bytes());
            }
        });
        Self { port, requests }
    }

    pub(crate) fn url(&self, path: &str) -> String {
        format!("http://127.0.0.1:{}{path}", self.port)
    }

    /// The `index`-th request that arrived, waiting for it to get there.
    pub(crate) fn request(&self, index: usize) -> String {
        for _ in 0..500 {
            if let Some(request) = self.requests.lock().unwrap().get(index) {
                return request.clone();
            }
            thread::sleep(Duration::from_millis(10));
        }
        panic!("the server received {} requests", index);
    }
}

/// A `200` that carries `body`, with a header of its own so that a response can be recognized.
pub(crate) fn ok(body: &str) -> String {
    format!(
        "HTTP/1.1 200 OK\r\nx-reply: yes\r\ncontent-length: {}\r\n\r\n{body}",
        body.len()
    )
}

/// The requests of these tests go to a server on this machine, so a proxy in the environment would
/// keep them away from it. A developer machine may have one, and then the tests are skipped; a CI
/// run must not, or it would report success without ever having reached a server.
pub(crate) fn behind_an_environment_proxy() -> bool {
    let variables = ["http_proxy", "https_proxy", "all_proxy"];
    let behind = variables
        .into_iter()
        .any(|name| std::env::var_os(name).is_some());
    if behind && std::env::var_os("CI").is_some() {
        panic!(
            "a proxy is set in the environment of this CI run, so the tests that talk to a local \
             server would be skipped instead of run"
        );
    }
    behind
}

/// Reads one request: the head, and as much of a body as `content-length` announces.
fn read_request(stream: &mut TcpStream) -> io::Result<String> {
    stream.set_read_timeout(Some(Duration::from_secs(10)))?;
    let mut request = String::new();
    let mut length = 0;
    {
        let mut reader = BufReader::new(&mut *stream);
        loop {
            let mut line = String::new();
            if reader.read_line(&mut line)? == 0 {
                break;
            }
            if let Some(value) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                length = value.trim().parse().unwrap_or(0);
            }
            let head_ended = line.trim_end().is_empty();
            request.push_str(&line);
            if head_ended {
                break;
            }
        }
        if length > 0 {
            let mut body = vec![0u8; length];
            reader.read_exact(&mut body)?;
            request.push_str(&String::from_utf8_lossy(&body));
        }
    }
    Ok(request)
}
