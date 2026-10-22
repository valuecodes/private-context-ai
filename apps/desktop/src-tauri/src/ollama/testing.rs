//! A fake Ollama on loopback for tests.

use std::net::SocketAddr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

/// Picks the reply from a request's path and JSON body.
pub type Route = Arc<dyn Fn(&str, &serde_json::Value) -> Reply + Send + Sync>;

/// What the fake Ollama answers to a connection.
#[derive(Clone)]
pub enum Reply {
    /// `200` with these NDJSON lines, each sent after `delay`.
    Lines(Vec<&'static str>, Duration),
    /// `200` with a JSON body.
    Json(&'static str),
    /// An error status with a JSON body.
    Status(u16, &'static str),
    /// Accepts and reads the request, then never answers.
    Silent,
    /// `200`, then `chunk` repeated until the client hangs up.
    Endless(&'static str),
    /// Whatever the route answers for this request.
    Route(Route),
}

pub type Bodies = Arc<Mutex<Vec<serde_json::Value>>>;
/// `> <path> <model>` once a request is read, `< <path> <model>` just before
/// its last line is written.
pub type Log = Arc<Mutex<Vec<String>>>;

pub struct Fake {
    pub addr: SocketAddr,
    pub accepted: Arc<AtomicUsize>,
    /// Every JSON request body received, in order.
    pub bodies: Bodies,
    pub log: Log,
}

pub async fn fake_ollama(reply: Reply) -> Fake {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let accepted = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&accepted);
    let bodies = Bodies::default();
    let log = Log::default();
    let (received, logged) = (Arc::clone(&bodies), Arc::clone(&log));
    tokio::spawn(async move {
        while let Ok((socket, _)) = listener.accept().await {
            counter.fetch_add(1, Ordering::SeqCst);
            tokio::spawn(serve(
                socket,
                reply.clone(),
                Arc::clone(&received),
                Arc::clone(&logged),
            ));
        }
    });
    Fake {
        addr,
        accepted,
        bodies,
        log,
    }
}

/// Reads one request (headers plus `content-length` body), then answers.
async fn serve(mut socket: TcpStream, reply: Reply, bodies: Bodies, log: Log) {
    let mut request = Vec::new();
    let mut buf = [0u8; 4096];
    let body_start = loop {
        let Ok(n) = socket.read(&mut buf).await else {
            return;
        };
        if n == 0 {
            return;
        }
        request.extend_from_slice(&buf[..n]);
        if let Some(pos) = request.windows(4).position(|w| w == b"\r\n\r\n") {
            break pos + 4;
        }
    };
    let headers = String::from_utf8_lossy(&request[..body_start]).to_ascii_lowercase();
    let path = headers
        .split_whitespace()
        .nth(1)
        .unwrap_or_default()
        .to_owned();
    let length: usize = headers
        .lines()
        .find_map(|line| line.strip_prefix("content-length:"))
        .map_or(0, |value| value.trim().parse().unwrap());
    while request.len() < body_start + length {
        match socket.read(&mut buf).await {
            Ok(0) | Err(_) => return,
            Ok(n) => request.extend_from_slice(&buf[..n]),
        }
    }
    let body: serde_json::Value =
        serde_json::from_slice(&request[body_start..]).unwrap_or_default();
    if !body.is_null() {
        bodies.lock().unwrap().push(body.clone());
    }
    let model = body["model"].as_str().unwrap_or_default().to_owned();
    log.lock().unwrap().push(format!("> {path} {model}"));
    let ended = || log.lock().unwrap().push(format!("< {path} {model}"));

    let reply = match reply {
        Reply::Route(route) => route(&path, &body),
        other => other,
    };
    let head = |status: u16| {
        format!(
            "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\nconnection: close\r\n\r\n"
        )
    };
    match reply {
        Reply::Lines(lines, delay) => {
            let _ = socket.write_all(head(200).as_bytes()).await;
            let count = lines.len();
            for (index, line) in lines.into_iter().enumerate() {
                tokio::time::sleep(delay).await;
                if index + 1 == count {
                    ended();
                }
                if socket
                    .write_all(format!("{line}\n").as_bytes())
                    .await
                    .is_err()
                {
                    return;
                }
            }
        }
        Reply::Json(body) => {
            ended();
            let _ = socket.write_all(head(200).as_bytes()).await;
            let _ = socket.write_all(body.as_bytes()).await;
        }
        Reply::Status(status, body) => {
            ended();
            let _ = socket.write_all(head(status).as_bytes()).await;
            let _ = socket.write_all(body.as_bytes()).await;
        }
        Reply::Silent => std::future::pending::<()>().await,
        Reply::Endless(chunk) => {
            let _ = socket.write_all(head(200).as_bytes()).await;
            while socket.write_all(chunk.as_bytes()).await.is_ok() {
                tokio::time::sleep(Duration::from_millis(1)).await;
            }
        }
        Reply::Route(_) => panic!("a route answered with another route"),
    }
    let _ = socket.shutdown().await;
}
