use std::borrow::Borrow;
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

use futures::StreamExt;
use reqwest::Client;
use reqwest::header::HeaderMap;
use reqwest::multipart;
use serde_json::Value;
use tokio::runtime::Runtime;

/// Mirrors DynXX's global TLS certificate setting (`dynxx_net_http_set_cert_path`).
#[derive(Clone, Default, PartialEq, Eq)]
pub enum CertConfig {
    /// Never configured. Keeps the platform trust store, which is what a plain
    /// `reqwest`/`native-tls` build does.
    #[default]
    Default,
    /// Verify peers against this CA bundle.
    Path(PathBuf),
    /// Explicitly configured with an empty path, which disables peer verification --
    /// the behaviour DynXX applies whenever no cert path is set. DynRS only disables
    /// verification when a caller asks for it explicitly.
    Disabled,
}

/// Mirrors `DynXXHttpProxyConfig`.
#[derive(Clone, PartialEq, Eq)]
pub struct ProxyConfig {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
}

/// Mirrors `DynXXHttpDnsConfig`: pins a host name to a fixed address, the way
/// `/etc/hosts` does.
///
/// `port` is kept for C-ABI parity with DynXX, which feeds curl's `CURLOPT_RESOLVE`
/// list where an entry is a host+port pair. `reqwest` on the other hand looks an
/// override up by host name only and `hyper` always takes the port from the URL, so
/// DynRS cannot match on a port: an override applies to every port of its host, while
/// hosts without an override are resolved normally.
#[derive(Clone, PartialEq, Eq)]
pub struct DnsConfig {
    pub host: String,
    pub port: u16,
    /// A numeric IP address. `set_dns_configs` drops entries whose address cannot be
    /// parsed, because a resolver can only answer with socket addresses.
    pub address: String,
}

#[derive(Clone, Default)]
struct HttpGlobalConfig {
    cert: CertConfig,
    proxy: Option<ProxyConfig>,
    dns: Vec<DnsConfig>,
}

static GLOBAL_CONFIG: OnceLock<Mutex<HttpGlobalConfig>> = OnceLock::new();
static CONFIG_VERSION: AtomicU64 = AtomicU64::new(0);
static CACHED_CLIENT: OnceLock<Mutex<Option<(u64, Client)>>> = OnceLock::new();

/// Serializes the tests that touch the process-wide config, so that one test's
/// `set_*` call cannot land between another test's reads.
#[cfg(test)]
pub(crate) static CONFIG_TEST_LOCK: Mutex<()> = Mutex::new(());

fn global_config() -> MutexGuard<'static, HttpGlobalConfig> {
    GLOBAL_CONFIG
        .get_or_init(|| Mutex::new(HttpGlobalConfig::default()))
        .lock()
        .unwrap()
}

/// A copy of the global config, only used by the tests.
#[cfg(test)]
pub(crate) fn config_snapshot() -> (CertConfig, Option<ProxyConfig>, Vec<DnsConfig>) {
    let config = global_config();
    (
        config.cert.clone(),
        config.proxy.clone(),
        config.dns.clone(),
    )
}

/// Why a network configuration call was refused.
///
/// The setters used to signal this with an `Option`, which could not be told apart from the
/// deliberate "clear it" value — and for the certificate path that meant a malformed path
/// *disabled* peer verification instead of leaving the configuration alone.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum ConfigError {
    /// The path was not valid UTF-8.
    InvalidUtf8,
    /// An address that the configuration needs is not a literal IP.
    InvalidAddress,
    /// The proxy port does not fit the field it has to travel in.
    InvalidPort,
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let text = match self {
            ConfigError::InvalidUtf8 => "the value is not valid UTF-8",
            ConfigError::InvalidAddress => "the address is not a literal IP",
            ConfigError::InvalidPort => "the port does not fit",
        };
        f.write_str(text)
    }
}

impl std::error::Error for ConfigError {}

/// Why a network operation failed.
///
/// The client used to return `NetError`, which a caller could only report as "it
/// failed": the box said nothing about which part went wrong, and the C layer could not tell a
/// request that never left from a body that was cut off. Each variant here is an answer a caller can
/// act on differently.
#[derive(Debug)]
pub enum NetError {
    /// The client could not be built from the global configuration.
    Client(String),
    /// The request itself failed: no connection, a timeout, a refused TLS handshake.
    Request(String),
    /// The response arrived, but its body could not be read.
    Body(String),
    /// The download target could not be written.
    Write(String),
    /// The multipart form was rejected by the client, or the async runtime could not be built.
    Internal(String),
}

impl std::fmt::Display for NetError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let text = match self {
            NetError::Client(message) => format!("the HTTP client is unusable: {message}"),
            NetError::Request(message) => format!("the request failed: {message}"),
            NetError::Body(message) => format!("the response body could not be read: {message}"),
            NetError::Write(message) => format!("the response could not be written: {message}"),
            NetError::Internal(message) => format!("the request could not be prepared: {message}"),
        };
        f.write_str(&text)
    }
}

impl std::error::Error for NetError {}

impl From<String> for NetError {
    fn from(message: String) -> Self {
        NetError::Internal(message)
    }
}

impl From<reqwest::Error> for NetError {
    /// A `reqwest::Error` only becomes a request failure once the request was sent: an error while
    /// *building* it — a bad header value, a malformed multipart part — is the caller's input and
    /// belongs with `Internal`, which the C layer reports as a rejected argument.
    fn from(error: reqwest::Error) -> Self {
        if error.is_builder() {
            NetError::Internal(error.to_string())
        } else if error.is_body() || error.is_decode() {
            NetError::Body(error.to_string())
        } else {
            NetError::Request(error.to_string())
        }
    }
}

impl From<std::io::Error> for NetError {
    fn from(error: std::io::Error) -> Self {
        NetError::Write(error.to_string())
    }
}

/// Mirrors `dynxx_net_http_set_cert_path`: an empty path disables peer verification and any other
/// value is the CA bundle path. `None` means "not valid UTF-8" and changes nothing: a path the
/// caller could not express must never be read as "turn verification off".
pub fn set_cert_path(path: Option<&str>) -> Result<(), ConfigError> {
    {
        let mut config = global_config();
        match path {
            Some("") => config.cert = CertConfig::Disabled,
            Some(path) => config.cert = CertConfig::Path(PathBuf::from(path)),
            None => return Err(ConfigError::InvalidUtf8),
        }
    }
    CONFIG_VERSION.fetch_add(1, Ordering::SeqCst);
    Ok(())
}

/// Mirrors `dynxx_net_http_set_proxy`: `None` or an empty host clears the proxy.
pub fn set_proxy(
    host: Option<&str>,
    port: u16,
    username: &str,
    password: &str,
) -> Result<(), ConfigError> {
    {
        let mut config = global_config();
        config.proxy = match host {
            Some(host) if !host.is_empty() => Some(ProxyConfig {
                host: host.to_string(),
                port,
                username: username.to_string(),
                password: password.to_string(),
            }),
            Some(_) => None,
            None => return Err(ConfigError::InvalidUtf8),
        };
    }
    CONFIG_VERSION.fetch_add(1, Ordering::SeqCst);
    Ok(())
}

/// Mirrors `dynxx_net_http_set_dns_configs`: an empty list clears the overrides.
///
/// An entry with an empty host or with an address that is not a literal IP used to be dropped
/// silently, which meant a pin the caller believed in was simply not there. It is now refused, and
/// nothing is changed.
pub fn set_dns_configs(configs: &[DnsConfig]) -> Result<(), ConfigError> {
    {
        let mut config = global_config();
        let resolved: Vec<DnsConfig> = configs
            .iter()
            .filter(|cfg| !cfg.host.is_empty() && cfg.address.parse::<IpAddr>().is_ok())
            .cloned()
            .collect();
        if resolved.len() != configs.len() {
            return Err(ConfigError::InvalidAddress);
        }
        config.dns = resolved;
    }
    CONFIG_VERSION.fetch_add(1, Ordering::SeqCst);
    Ok(())
}

fn build_client(
    cert: &CertConfig,
    proxy: Option<&ProxyConfig>,
    dns: &[DnsConfig],
) -> Result<Client, NetError> {
    let mut builder = reqwest::Client::builder();

    builder = match cert {
        CertConfig::Default => builder,
        CertConfig::Disabled => builder.danger_accept_invalid_certs(true),
        CertConfig::Path(path) => {
            let cert = std::fs::read(path)?;
            builder.add_root_certificate(reqwest::Certificate::from_pem(&cert)?)
        }
    };

    if let Some(proxy) = proxy {
        // DynXX passes `host[:port]` straight to curl; reqwest needs a URL with a scheme.
        let mut url = if proxy.host.contains("://") {
            proxy.host.clone()
        } else {
            format!("http://{}", proxy.host)
        };
        if proxy.port != 0 {
            url.push_str(&format!(":{}", proxy.port));
        }
        let mut reqwest_proxy = reqwest::Proxy::all(&url)?;
        if !proxy.username.is_empty() {
            reqwest_proxy = reqwest_proxy.basic_auth(&proxy.username, &proxy.password);
        }
        builder = builder.proxy(reqwest_proxy);
    }

    // Each entry pins a host name to a fixed address, the way DynXX feeds curl's
    // `CURLOPT_RESOLVE` list. Later entries win for the same host, matching the map
    // insert that `ClientBuilder::resolve` performs.
    for config in dns {
        if let Ok(address) = config.address.parse::<IpAddr>() {
            builder = builder.resolve(&config.host, SocketAddr::new(address, config.port));
        }
    }

    Ok(builder.build()?)
}

/// The client bound to the current global config. It is rebuilt whenever the config
/// changes; cloning a `reqwest::Client` is cheap (it is refcounted internally).
fn global_client() -> Result<Client, NetError> {
    let version = CONFIG_VERSION.load(Ordering::SeqCst);
    let cache = CACHED_CLIENT.get_or_init(|| Mutex::new(None));

    {
        let cached = cache.lock().unwrap();
        if let Some((cached_version, client)) = cached.as_ref()
            && *cached_version == version
        {
            return Ok(client.clone());
        }
    }

    let (cert, proxy, dns) = {
        let config = global_config();
        (
            config.cert.clone(),
            config.proxy.clone(),
            config.dns.clone(),
        )
    };
    let client = build_client(&cert, proxy.as_ref(), &dns)?;

    let mut cached = cache.lock().unwrap();
    *cached = Some((version, client.clone()));
    Ok(client)
}

/// One multipart field of an upload.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct UploadPart {
    /// The form field name. An empty name is refused by [`UploadPart::new`].
    pub name: String,
    /// The bytes of the field.
    pub data: Vec<u8>,
    /// The MIME type, when the caller named one.
    pub mime: Option<String>,
    /// The file name, when the field is a file.
    pub filename: Option<String>,
}

impl UploadPart {
    /// Builds a part, or reports the name as missing.
    ///
    /// The rule lives here rather than at the C boundary because a part without a name is not a
    /// valid multipart field at all: a caller that dropped it used to send the rest and report
    /// success, which is how someone came to believe a file had been uploaded.
    pub fn new(
        name: impl Into<String>,
        data: Vec<u8>,
        mime: Option<String>,
        filename: Option<String>,
    ) -> Result<Self, NetError> {
        let name = name.into();
        if name.is_empty() {
            return Err(NetError::Internal(
                "a multipart field must have a name".to_string(),
            ));
        }
        Ok(Self {
            name,
            data,
            mime,
            filename,
        })
    }
}

/// The runtime the blocking entry points share.
///
/// It lives here rather than beside the C entry points because it is what turns the asynchronous
/// client above into a blocking one: that is a property of this module, not of the C ABI, and a
/// Rust caller that needs a blocking request wants the same guarantee.
static RUNTIME: Mutex<Option<Result<Arc<Runtime>, String>>> = Mutex::new(None);

/// The shared runtime, built on first use.
///
/// The build result is cached even when it failed, so a host that cannot create a runtime is not
/// asked again on every call; the `Arc` is cloned out before it is used, so the lock is only held
/// while the runtime is created.
fn runtime() -> Result<Arc<Runtime>, String> {
    let mut slot = RUNTIME
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if slot.is_none() {
        *slot = Some(
            Runtime::new()
                .map(Arc::new)
                .map_err(|e| format!("could not create the async runtime: {e}")),
        );
    }
    match slot.as_ref() {
        Some(Ok(runtime)) => Ok(Arc::clone(runtime)),
        Some(Err(message)) => Err(message.clone()),
        None => Err("the async runtime is unavailable".to_string()),
    }
}

/// Drives `operation` on the shared runtime.
///
/// `Err` carries the reason, so a caller can report a runtime that could not be built instead of
/// reading it as a failed request.
pub fn block_on<T>(operation: impl std::future::Future<Output = T>) -> Result<T, String> {
    Ok(runtime()?.block_on(operation))
}

pub struct HttpClient {
    /// Set when the caller passed an explicit cert path to `new`; such a handle keeps a
    /// fixed client and ignores the global config.
    fixed_client: Option<Client>,
}

pub struct HttpResponse {
    pub status: reqwest::StatusCode,
    pub headers: HeaderMap,
    pub body: Option<String>,
    /// Set when the body could not be read at all, with the reason.
    ///
    /// A failed read used to be folded into `body: None`, which made it identical to a response
    /// that has no body to begin with — a download, whose bytes went to a file. A caller could not
    /// tell "there is nothing here" from "I could not get it".
    pub body_error: Option<String>,
}

impl HttpClient {
    pub fn new(ca_cert_path: Option<&Path>) -> Result<Self, NetError> {
        let fixed_client = match ca_cert_path {
            // A fixed client keeps its own TLS setting and ignores the global proxy
            // and DNS configs, so it is built with those turned off.
            Some(path) => Some(build_client(
                &CertConfig::Path(path.to_path_buf()),
                None,
                &[],
            )?),
            None => None,
        };
        Ok(Self { fixed_client })
    }

    /// Resolves the client for a request: the fixed one, or the global-config one.
    fn client(&self) -> Result<Client, NetError> {
        match &self.fixed_client {
            Some(client) => Ok(client.clone()),
            None => global_client(),
        }
    }

    async fn execute_request(
        &self,
        request: reqwest::RequestBuilder,
    ) -> Result<HttpResponse, NetError> {
        let response = request.send().await?;
        let status = response.status();
        let headers = response.headers().clone();
        // The reason a body could not be read is kept: a response with no body and a response whose
        // body failed to arrive are different answers, and only one of them is the caller's problem.
        let (body, body_error) = match response.text().await {
            Ok(text) => (Some(text), None),
            Err(error) => (None, Some(error.to_string())),
        };

        Ok(HttpResponse {
            status,
            headers,
            body,
            body_error,
        })
    }

    pub async fn get<K, V>(
        &self,
        url: &str,
        headers: Option<HashMap<K, V>>,
        body: Option<&str>,
    ) -> Result<HttpResponse, NetError>
    where
        K: Borrow<str>,
        V: Borrow<str>,
    {
        let client = self.client()?;
        let mut request = client.get(url);

        if let Some(headers_map) = headers {
            for (key, value) in headers_map {
                request = request.header(key.borrow(), value.borrow());
            }
        }

        if let Some(body_content) = body {
            request = request.body(body_content.to_string());
        }

        self.execute_request(request).await
    }

    pub async fn post<K, V>(
        &self,
        url: &str,
        headers: Option<HashMap<K, V>>,
        body: Option<&str>,
        params: Option<HashMap<K, V>>,
    ) -> Result<HttpResponse, NetError>
    where
        K: Borrow<str>,
        V: Borrow<str>,
    {
        let client = self.client()?;
        let mut request = client.post(url);

        if let Some(headers_map) = headers {
            for (key, value) in headers_map {
                request = request.header(key.borrow(), value.borrow());
            }
        }

        if let Some(params_map) = params {
            let json_map = params_map
                .into_iter()
                .filter_map(|(k, v)| {
                    serde_json::from_str::<Value>(v.borrow())
                        .map(|val| (k.borrow().to_string(), val))
                        .ok()
                })
                .collect::<HashMap<String, Value>>();
            request = request.json(&json_map);
        } else if let Some(body_content) = body {
            request = request.body(body_content.to_string());
        }

        self.execute_request(request).await
    }

    pub async fn download<K, V>(
        &self,
        url: &str,
        headers: Option<HashMap<K, V>>,
        output_path: &Path,
    ) -> Result<HttpResponse, NetError>
    where
        K: Borrow<str>,
        V: Borrow<str>,
    {
        let client = self.client()?;
        let mut request = client.get(url);

        if let Some(headers_map) = headers {
            for (key, value) in headers_map {
                request = request.header(key.borrow(), value.borrow());
            }
        }

        let response = request.send().await?;
        let status = response.status();
        let headers = response.headers().clone();

        // Stream the response body to file
        let mut file = tokio::fs::File::create(output_path).await?;
        let mut stream = response.bytes_stream();

        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            tokio::io::copy(&mut chunk.as_ref(), &mut file).await?;
        }

        Ok(HttpResponse {
            status,
            headers,
            body: None,
            body_error: None,
        })
    }

    pub async fn upload<K, V>(
        &self,
        url: &str,
        headers: Option<HashMap<K, V>>,
        parts: Vec<UploadPart>,
    ) -> Result<HttpResponse, NetError>
    where
        K: Borrow<str>,
        V: Borrow<str>,
    {
        let client = self.client()?;
        let mut request = client.post(url);

        if let Some(headers_map) = headers {
            for (key, value) in headers_map {
                request = request.header(key.borrow(), value.borrow());
            }
        }

        let mut form = multipart::Form::new();
        for part in parts {
            let UploadPart {
                name,
                data,
                mime,
                filename,
            } = part;
            let field = multipart::Part::bytes(data);
            let field = match mime {
                Some(mime) => field.mime_str(&mime)?,
                None => field,
            };
            let field = match filename {
                Some(name) => field.file_name(name),
                None => field,
            };
            form = form.part(name, field);
        }

        request = request.multipart(form);
        self.execute_request(request).await
    }

    /// `get`, driven to completion on the shared runtime.
    ///
    /// A synchronous caller — the C ABI, or any Rust caller that is not already inside an async
    /// runtime — needs these rather than the `async` methods above. Keeping them here means the
    /// *shape* of a request belongs to this module, and the ABI layer only reads arguments and
    /// writes results.
    pub fn blocking_get<K, V>(
        &self,
        url: &str,
        headers: Option<HashMap<K, V>>,
        body: Option<&str>,
    ) -> Result<HttpResponse, NetError>
    where
        K: Borrow<str>,
        V: Borrow<str>,
    {
        block_on_request(self.get(url, headers, body))
    }

    /// `post`, driven to completion on the shared runtime.
    pub fn blocking_post<K, V>(
        &self,
        url: &str,
        headers: Option<HashMap<K, V>>,
        body: Option<&str>,
        params: Option<HashMap<K, V>>,
    ) -> Result<HttpResponse, NetError>
    where
        K: Borrow<str>,
        V: Borrow<str>,
    {
        block_on_request(self.post(url, headers, body, params))
    }

    /// `download`, driven to completion on the shared runtime.
    pub fn blocking_download<K, V>(
        &self,
        url: &str,
        headers: Option<HashMap<K, V>>,
        output_path: &Path,
    ) -> Result<HttpResponse, NetError>
    where
        K: Borrow<str>,
        V: Borrow<str>,
    {
        block_on_request(self.download(url, headers, output_path))
    }

    /// `upload`, driven to completion on the shared runtime.
    pub fn blocking_upload<K, V>(
        &self,
        url: &str,
        headers: Option<HashMap<K, V>>,
        parts: Vec<UploadPart>,
    ) -> Result<HttpResponse, NetError>
    where
        K: Borrow<str>,
        V: Borrow<str>,
    {
        block_on_request(self.upload(url, headers, parts))
    }
}

/// Runs one request on the shared runtime.
///
/// The runtime is built on first use and its failure is reported as a request failure, so a host
/// where a runtime cannot be created gets a status instead of a panic.
fn block_on_request(
    request: impl std::future::Future<Output = Result<HttpResponse, NetError>>,
) -> Result<HttpResponse, NetError> {
    block_on(request).map_err(NetError::Internal)?
}

#[cfg(test)]
// The config lock is held on purpose across the requests of the tests below: they drive the
// process-wide client, and the runtime of `#[tokio::test]` has a single thread, so the guard
// cannot be the source of a deadlock here.
#[allow(clippy::await_holding_lock)]
mod tests {
    use crate::core::net_test_server::{Server, behind_an_environment_proxy, ok};

    use super::*;

    fn lock_config() -> MutexGuard<'static, ()> {
        CONFIG_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Puts the process-wide config back to a baseline, so that no test depends on what
    /// another test left behind, or on the order the tests happen to run in.
    fn reset_config() {
        // An empty path is the deliberate "do not verify" setting; `None` is the unreadable one and
        // is refused, which is the whole point of separating them.
        set_cert_path(Some("")).expect("an empty path disables verification");
        set_proxy(Some(""), 0, "", "").expect("an empty host clears the proxy");
        set_dns_configs(&[]).expect("an empty list clears the overrides");
    }

    #[test]
    fn cert_and_proxy_config_lifecycle() {
        let _guard = lock_config();
        reset_config();

        // An empty cert path disables verification, mirroring DynXX.
        set_cert_path(Some("")).expect("an empty path is accepted");
        assert!(matches!(global_config().cert, CertConfig::Disabled));

        // A path the caller could not express as UTF-8 is refused and changes nothing: reading it
        // as "disable verification" would turn a bad argument into a security downgrade.
        set_cert_path(Some("/tmp/ca.pem")).expect("a path is accepted");
        assert!(matches!(global_config().cert, CertConfig::Path(_)));
        assert_eq!(
            set_cert_path(None),
            Err(ConfigError::InvalidUtf8),
            "an unrepresentable path is refused"
        );
        assert!(
            matches!(global_config().cert, CertConfig::Path(_)),
            "a refused path leaves the previous configuration alone"
        );

        set_proxy(Some("127.0.0.1"), 8888, "user", "pwd").expect("a proxy is accepted");
        {
            let proxy = global_config().proxy.clone().expect("proxy should be set");
            assert_eq!(proxy.host, "127.0.0.1");
            assert_eq!(proxy.port, 8888);
            assert_eq!(proxy.username, "user");
            assert_eq!(proxy.password, "pwd");
        }

        // An empty host clears the proxy.
        set_proxy(Some(""), 0, "", "").expect("an empty host is accepted");
        assert!(global_config().proxy.is_none());

        // A host that is not UTF-8 is refused rather than clearing a working proxy.
        set_proxy(Some("127.0.0.1"), 8888, "user", "pwd").expect("a proxy is accepted");
        assert_eq!(set_proxy(None, 0, "", ""), Err(ConfigError::InvalidUtf8));
        assert!(
            global_config().proxy.is_some(),
            "a refused proxy leaves the previous configuration alone"
        );
    }

    #[test]
    fn dns_configs_are_validated_and_versioned() {
        let _guard = lock_config();
        reset_config();

        // An entry that could not be used is refused instead of silently dropped: a pin the caller
        // believes in must not quietly not be there.
        assert_eq!(
            set_dns_configs(&[
                DnsConfig {
                    host: "pinned.test".to_string(),
                    port: 443,
                    address: "10.1.2.3".to_string(),
                },
                DnsConfig {
                    host: String::new(),
                    port: 443,
                    address: "10.1.2.4".to_string(),
                },
            ]),
            Err(ConfigError::InvalidAddress),
            "an empty host is refused"
        );
        assert_eq!(
            set_dns_configs(&[DnsConfig {
                host: "broken.test".to_string(),
                port: 0,
                address: "not-an-ip".to_string(),
            }]),
            Err(ConfigError::InvalidAddress),
            "an address no resolver could answer with is refused"
        );
        assert!(
            config_snapshot().2.is_empty(),
            "a refused list leaves the previous overrides alone"
        );

        set_dns_configs(&[DnsConfig {
            host: "pinned.test".to_string(),
            port: 443,
            address: "10.1.2.3".to_string(),
        }])
        .expect("a valid override is accepted");

        let (_, _, dns) = config_snapshot();
        assert_eq!(dns.len(), 1);
        assert_eq!(dns[0].host, "pinned.test");
        assert_eq!(dns[0].address, "10.1.2.3");

        let version = CONFIG_VERSION.load(Ordering::SeqCst);
        global_client().expect("client builds with DNS overrides");
        assert_eq!(CONFIG_VERSION.load(Ordering::SeqCst), version);

        // An empty list clears the overrides again.
        set_dns_configs(&[]).expect("an empty list is accepted");
        let (_, _, dns) = config_snapshot();
        assert!(dns.is_empty());
        assert!(CONFIG_VERSION.load(Ordering::SeqCst) > version);
        global_client().expect("client rebuilds without DNS overrides");
    }

    #[test]
    fn config_change_invalidates_cached_client() {
        let _guard = lock_config();
        reset_config();

        let version = CONFIG_VERSION.load(Ordering::SeqCst);
        global_client().expect("client builds with the global config");
        assert_eq!(CONFIG_VERSION.load(Ordering::SeqCst), version);

        set_proxy(Some("127.0.0.1"), 1, "u", "p").expect("a proxy is accepted");
        assert!(CONFIG_VERSION.load(Ordering::SeqCst) > version);
        global_client().expect("client rebuilds after a config change");
    }

    fn test_client() -> HttpClient {
        HttpClient::new(None).expect("a client without a fixed certificate")
    }

    #[tokio::test]
    async fn get_sends_headers_and_a_body_and_reads_the_response() {
        if behind_an_environment_proxy() {
            return;
        }
        let _guard = lock_config();
        reset_config();
        let server = Server::start(ok("hello"));

        let headers = HashMap::from([("x-request", "1")]);
        let response = test_client()
            .get(&server.url("/a"), Some(headers), Some("payload"))
            .await
            .expect("the request goes through");

        assert_eq!(response.status, reqwest::StatusCode::OK);
        assert_eq!(
            response
                .headers
                .get("x-reply")
                .and_then(|value| value.to_str().ok()),
            Some("yes")
        );
        assert_eq!(response.body.as_deref(), Some("hello"));

        let request = server.request(0);
        let lowered = request.to_ascii_lowercase();
        assert!(request.starts_with("GET /a HTTP/1.1"), "{request}");
        assert!(lowered.contains("x-request: 1"), "{request}");
        assert!(request.ends_with("payload"), "{request}");
    }

    #[tokio::test]
    async fn post_turns_params_into_a_json_body() {
        if behind_an_environment_proxy() {
            return;
        }
        let _guard = lock_config();
        reset_config();
        let server = Server::start(ok("taken"));

        let params = HashMap::from([("a", "1"), ("b", "not json")]);
        let response = test_client()
            .post(
                &server.url("/json"),
                None::<HashMap<&str, &str>>,
                None,
                Some(params),
            )
            .await
            .expect("the request goes through");

        assert_eq!(response.status, reqwest::StatusCode::OK);
        let request = server.request(0);
        let lowered = request.to_ascii_lowercase();
        assert!(
            lowered.contains("content-type: application/json"),
            "{request}"
        );
        assert!(request.contains("\"a\":1"), "{request}");
        // A value that is not JSON is dropped instead of failing the request, the way DynXX does.
        assert!(!lowered.contains("not json"), "{request}");
    }

    #[tokio::test]
    async fn post_sends_a_raw_body_when_there_are_no_params() {
        if behind_an_environment_proxy() {
            return;
        }
        let _guard = lock_config();
        reset_config();
        let server = Server::start(ok("taken"));

        let response = test_client()
            .post(
                &server.url("/raw"),
                None::<HashMap<String, String>>,
                Some("plain"),
                None::<HashMap<String, String>>,
            )
            .await
            .expect("the request goes through");

        assert_eq!(response.status, reqwest::StatusCode::OK);
        let request = server.request(0);
        assert!(request.ends_with("plain"), "{request}");
        assert!(
            !request.to_ascii_lowercase().contains("application/json"),
            "{request}"
        );
    }

    #[tokio::test]
    async fn download_writes_the_response_into_a_file() {
        if behind_an_environment_proxy() {
            return;
        }
        let _guard = lock_config();
        reset_config();
        let server = Server::start(ok("hello"));

        let path = std::env::temp_dir().join(format!("dynrs_download_{}.txt", std::process::id()));
        let response = test_client()
            .download(&server.url("/file"), None::<HashMap<String, String>>, &path)
            .await
            .expect("the request goes through");

        assert_eq!(response.status, reqwest::StatusCode::OK);
        assert!(response.body.is_none());
        assert_eq!(
            std::fs::read_to_string(&path).expect("the file was written"),
            "hello"
        );
        std::fs::remove_file(&path).expect("the file is removed again");

        let request = server.request(0);
        assert!(request.starts_with("GET /file"), "{request}");
    }

    #[tokio::test]
    async fn upload_sends_a_multipart_form() {
        if behind_an_environment_proxy() {
            return;
        }
        let _guard = lock_config();
        reset_config();
        let server = Server::start(ok("stored"));

        let parts = vec![
            UploadPart::new(
                "field",
                b"data".to_vec(),
                Some("text/plain".to_string()),
                Some("a.txt".to_string()),
            )
            .expect("the part has a name"),
        ];
        let response = test_client()
            .upload(&server.url("/up"), None::<HashMap<String, String>>, parts)
            .await
            .expect("the request goes through");

        assert_eq!(response.status, reqwest::StatusCode::OK);
        let request = server.request(0);
        let lowered = request.to_ascii_lowercase();
        assert!(
            lowered.contains("content-type: multipart/form-data; boundary="),
            "{request}"
        );
        assert!(request.contains("name=\"field\""), "{request}");
        assert!(request.contains("filename=\"a.txt\""), "{request}");
        assert!(lowered.contains("content-type: text/plain"), "{request}");
        assert!(request.contains("data"), "{request}");
    }
}
