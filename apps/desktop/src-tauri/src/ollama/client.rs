//! The loopback-only base URL and the HTTP client that talks to Ollama.

use std::fmt;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Duration;

use serde::Deserialize;
use serde::de::DeserializeOwned;
use url::{Host, Url};

use crate::error::AppError;

pub const DEFAULT_BASE_URL: &str = "http://127.0.0.1:11434";

/// Overrides the base URL in debug builds only; still loopback-guarded.
pub const BASE_URL_ENV: &str = "PRIVATE_CONTEXT_OLLAMA_URL";

const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);

/// Cap for small JSON responses (`/api/version`, `/api/tags`).
pub const MAX_JSON_BODY: usize = 1024 * 1024;

/// Cap for an error response body read for its `error` field.
pub const MAX_ERROR_BODY: usize = 64 * 1024;

/// Error text from Ollama is cut to this many characters.
pub const MAX_ERROR_CHARS: usize = 300;

/// An Ollama base URL that passed the loopback guard: scheme `http`, host in
/// `127.0.0.0/8`, `::1` or `localhost`, no credentials, path, query or
/// fragment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OllamaBaseUrl {
    url: Url,
}

impl OllamaBaseUrl {
    /// Rejects anything that is not plain `http` on loopback. The input is
    /// never echoed back, since it could carry credentials.
    pub fn parse(input: &str) -> Result<Self, AppError> {
        let reject = |why: &str| AppError::InvalidBaseUrl(why.to_owned());
        let url = Url::parse(input.trim()).map_err(|_| reject("not a valid URL"))?;
        if url.scheme() != "http" {
            return Err(reject("scheme must be http"));
        }
        if !url.username().is_empty() || url.password().is_some() {
            return Err(reject("credentials are not allowed"));
        }
        if url.query().is_some() || url.fragment().is_some() {
            return Err(reject("query and fragment are not allowed"));
        }
        if !url.path().is_empty() && url.path() != "/" {
            return Err(reject("a path is not allowed"));
        }
        match url.host() {
            Some(Host::Ipv4(ip)) if ip.is_loopback() => {}
            Some(Host::Ipv6(ip)) if ip.is_loopback() => {}
            Some(Host::Domain(domain)) if domain.eq_ignore_ascii_case("localhost") => {}
            _ => return Err(reject("host must be a loopback address")),
        }
        Ok(Self { url })
    }

    pub fn port(&self) -> u16 {
        self.url.port_or_known_default().unwrap_or(80)
    }

    /// `host:port`, with IPv6 hosts in brackets.
    pub fn host_port(&self) -> String {
        format!(
            "{}:{}",
            self.url.host_str().unwrap_or_default(),
            self.port()
        )
    }

    /// The URL of an API path such as `/api/chat`.
    pub fn endpoint(&self, path: &str) -> Url {
        let mut url = self.url.clone();
        url.set_path(path);
        url
    }

    fn is_localhost_name(&self) -> bool {
        matches!(self.url.host(), Some(Host::Domain(_)))
    }
}

impl fmt::Display for OllamaBaseUrl {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "http://{}", self.host_port())
    }
}

/// `http://127.0.0.1:11434`, or in debug builds the value of
/// [`BASE_URL_ENV`] when it is set.
pub fn default_base_url() -> Result<OllamaBaseUrl, AppError> {
    if cfg!(debug_assertions)
        && let Ok(value) = std::env::var(BASE_URL_ENV)
    {
        return OllamaBaseUrl::parse(&value);
    }
    OllamaBaseUrl::parse(DEFAULT_BASE_URL)
}

/// A client that ignores proxy settings, never follows redirects and pins
/// `localhost` to `127.0.0.1`, so DNS can never point it elsewhere.
pub fn build_http(base: &OllamaBaseUrl) -> Result<reqwest::Client, AppError> {
    let mut builder = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(CONNECT_TIMEOUT);
    if base.is_localhost_name() {
        builder = builder.resolve(
            "localhost",
            SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), base.port()),
        );
    }
    builder
        .build()
        .map_err(|err| AppError::Internal(format!("HTTP client: {err}")))
}

pub fn map_reqwest(err: &reqwest::Error) -> AppError {
    if err.is_timeout() {
        AppError::Timeout("Ollama did not respond in time".into())
    } else if err.is_connect() {
        AppError::Unreachable("could not connect to Ollama".into())
    } else {
        AppError::Ollama(truncate(&format!("request failed: {err}"), MAX_ERROR_CHARS))
    }
}

pub fn truncate(text: &str, max_chars: usize) -> String {
    text.chars().take(max_chars).collect()
}

/// `HTTP <status>: <error field>` from a non-2xx response body.
pub fn http_error(status: u16, body: &[u8]) -> AppError {
    #[derive(Deserialize)]
    struct ErrorBody {
        error: String,
    }
    let message = match serde_json::from_slice::<ErrorBody>(body) {
        Ok(body) => format!("HTTP {status}: {}", body.error),
        Err(_) => format!("HTTP {status}"),
    };
    AppError::Ollama(truncate(&message, MAX_ERROR_CHARS))
}

/// Reads the whole body, failing once it exceeds `max` bytes.
pub async fn read_limited(resp: &mut reqwest::Response, max: usize) -> Result<Vec<u8>, AppError> {
    let mut body = Vec::new();
    while let Some(chunk) = resp.chunk().await.map_err(|err| map_reqwest(&err))? {
        if body.len() + chunk.len() > max {
            return Err(AppError::TooLarge(format!("response is over {max} bytes")));
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

/// GETs a small JSON document within `timeout`. Returns the HTTP status (when
/// one arrived) next to the result, for the audit event.
pub async fn get_json<T: DeserializeOwned>(
    http: &reqwest::Client,
    url: Url,
    timeout: Duration,
) -> (Option<u16>, Result<T, AppError>) {
    let mut status = None;
    let request = async {
        let mut resp = http
            .get(url)
            .send()
            .await
            .map_err(|err| map_reqwest(&err))?;
        let code = resp.status();
        status = Some(code.as_u16());
        let limit = if code.is_success() {
            MAX_JSON_BODY
        } else {
            MAX_ERROR_BODY
        };
        let body = read_limited(&mut resp, limit).await?;
        if !code.is_success() {
            return Err(http_error(code.as_u16(), &body));
        }
        serde_json::from_slice(&body)
            .map_err(|err| AppError::Ollama(format!("unexpected response: {err}")))
    };
    let result = match tokio::time::timeout(timeout, request).await {
        Ok(result) => result,
        Err(_) => Err(AppError::Timeout("Ollama did not respond in time".into())),
    };
    (status, result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base_url_accepts_loopback() {
        for (input, host_port) in [
            ("http://127.0.0.1:11434", "127.0.0.1:11434"),
            ("http://127.1.2.3:8080/", "127.1.2.3:8080"),
            ("http://localhost:11434", "localhost:11434"),
            ("http://LOCALHOST:11434", "localhost:11434"),
            ("http://[::1]:11434", "[::1]:11434"),
            ("  http://127.0.0.1:11434/  ", "127.0.0.1:11434"),
        ] {
            let base = OllamaBaseUrl::parse(input).unwrap_or_else(|err| panic!("{input}: {err}"));
            assert_eq!(base.host_port(), host_port, "{input}");
        }
        let base = OllamaBaseUrl::parse(DEFAULT_BASE_URL).unwrap();
        assert_eq!(
            base.endpoint("/api/chat").as_str(),
            "http://127.0.0.1:11434/api/chat"
        );
        assert_eq!(base.to_string(), DEFAULT_BASE_URL);
    }

    #[test]
    fn base_url_rejects_non_loopback() {
        for input in [
            "http://10.0.0.1:11434",
            "http://0.0.0.0:11434",
            "http://192.168.1.2:11434",
            "http://example.com:11434",
            "http://localhost.example.com:11434",
            "http://[::ffff:127.0.0.1]:11434",
            "https://127.0.0.1:11434",
            "ftp://127.0.0.1",
            "http://user:pw@127.0.0.1:11434",
            "http://user@127.0.0.1:11434",
            "http://127.0.0.1:11434/api",
            "http://127.0.0.1:11434/?q=1",
            "http://127.0.0.1:11434/#f",
            "127.0.0.1:11434",
            "",
        ] {
            assert!(
                matches!(
                    OllamaBaseUrl::parse(input),
                    Err(AppError::InvalidBaseUrl(_))
                ),
                "{input} was accepted"
            );
        }
    }

    #[test]
    fn rejection_message_does_not_echo_input() {
        let err = OllamaBaseUrl::parse("http://user:hunter2@10.0.0.1").unwrap_err();
        assert!(!err.to_string().contains("hunter2"));
    }

    #[test]
    fn http_error_reads_error_field_and_truncates() {
        assert_eq!(
            http_error(404, br#"{"error":"model 'x' not found"}"#),
            AppError::Ollama("HTTP 404: model 'x' not found".into())
        );
        assert_eq!(
            http_error(500, b"<html>"),
            AppError::Ollama("HTTP 500".into())
        );
        let long = format!(r#"{{"error":"{}"}}"#, "x".repeat(1000));
        let AppError::Ollama(message) = http_error(400, long.as_bytes()) else {
            panic!("wrong variant");
        };
        assert_eq!(message.chars().count(), MAX_ERROR_CHARS);
    }
}
