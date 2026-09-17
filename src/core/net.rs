use std::borrow::Borrow;
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock};

use futures::StreamExt;
use reqwest::Client;
use reqwest::header::HeaderMap;
use reqwest::multipart;
use serde_json::Value;

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

/// Mirrors `dynxx_net_http_set_cert_path`: `None` or an empty path disables peer
/// verification, any other value is used as the CA bundle.
pub fn set_cert_path(path: Option<&str>) {
    {
        let mut config = global_config();
        config.cert = match path {
            Some(path) if !path.is_empty() => CertConfig::Path(PathBuf::from(path)),
            _ => CertConfig::Disabled,
        };
    }
    CONFIG_VERSION.fetch_add(1, Ordering::SeqCst);
}

/// Mirrors `dynxx_net_http_set_proxy`: `None` or an empty host clears the proxy.
pub fn set_proxy(host: Option<&str>, port: u16, username: &str, password: &str) {
    {
        let mut config = global_config();
        config.proxy = match host {
            Some(host) if !host.is_empty() => Some(ProxyConfig {
                host: host.to_string(),
                port,
                username: username.to_string(),
                password: password.to_string(),
            }),
            _ => None,
        };
    }
    CONFIG_VERSION.fetch_add(1, Ordering::SeqCst);
}

/// Mirrors `dynxx_net_http_set_dns_configs`: an empty list clears the overrides.
/// Entries with an empty host or with an address that is not a literal IP are skipped,
/// mirroring how DynXX drops invalid configs.
pub fn set_dns_configs(configs: &[DnsConfig]) {
    {
        let mut config = global_config();
        config.dns = configs
            .iter()
            .filter(|cfg| !cfg.host.is_empty() && cfg.address.parse::<IpAddr>().is_ok())
            .cloned()
            .collect();
    }
    CONFIG_VERSION.fetch_add(1, Ordering::SeqCst);
}

fn build_client(
    cert: &CertConfig,
    proxy: Option<&ProxyConfig>,
    dns: &[DnsConfig],
) -> Result<Client, Box<dyn std::error::Error>> {
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
fn global_client() -> Result<Client, Box<dyn std::error::Error>> {
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

/// One multipart field of an upload: name, data, optional MIME type and file name.
pub type UploadPart = (String, Vec<u8>, Option<String>, Option<String>);

pub struct HttpClient {
    /// Set when the caller passed an explicit cert path to `new`; such a handle keeps a
    /// fixed client and ignores the global config.
    fixed_client: Option<Client>,
}

pub struct HttpResponse {
    pub status: reqwest::StatusCode,
    pub headers: HeaderMap,
    pub body: Option<String>,
}

impl HttpClient {
    pub fn new(ca_cert_path: Option<&Path>) -> Result<Self, Box<dyn std::error::Error>> {
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
    fn client(&self) -> Result<Client, Box<dyn std::error::Error>> {
        match &self.fixed_client {
            Some(client) => Ok(client.clone()),
            None => global_client(),
        }
    }

    async fn execute_request(
        &self,
        request: reqwest::RequestBuilder,
    ) -> Result<HttpResponse, Box<dyn std::error::Error>> {
        let response = request.send().await?;
        let status = response.status();
        let headers = response.headers().clone();
        let body = response.text().await.ok();

        Ok(HttpResponse {
            status,
            headers,
            body,
        })
    }

    pub async fn get<K, V>(
        &self,
        url: &str,
        headers: Option<HashMap<K, V>>,
        body: Option<&str>,
    ) -> Result<HttpResponse, Box<dyn std::error::Error>>
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
    ) -> Result<HttpResponse, Box<dyn std::error::Error>>
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
    ) -> Result<HttpResponse, Box<dyn std::error::Error>>
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
        })
    }

    pub async fn upload<K, V>(
        &self,
        url: &str,
        headers: Option<HashMap<K, V>>,
        parts: Vec<UploadPart>,
    ) -> Result<HttpResponse, Box<dyn std::error::Error>>
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
        for (name, data, mime_type, filename) in parts {
            let part = multipart::Part::bytes(data);
            let part = match mime_type {
                Some(mime) => part.mime_str(&mime)?,
                None => part,
            };
            let part = match filename {
                Some(name) => part.file_name(name),
                None => part,
            };
            form = form.part(name, part);
        }

        request = request.multipart(form);
        self.execute_request(request).await
    }
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
        set_cert_path(None);
        set_proxy(None, 0, "", "");
        set_dns_configs(&[]);
    }

    #[test]
    fn cert_and_proxy_config_lifecycle() {
        let _guard = lock_config();
        reset_config();

        // A null/empty cert path disables verification, mirroring DynXX.
        set_cert_path(None);
        assert!(matches!(global_config().cert, CertConfig::Disabled));

        // Any other path is used as the CA bundle.
        set_cert_path(Some("/tmp/ca.pem"));
        assert!(matches!(global_config().cert, CertConfig::Path(_)));

        set_proxy(Some("127.0.0.1"), 8888, "user", "pwd");
        {
            let proxy = global_config().proxy.clone().expect("proxy should be set");
            assert_eq!(proxy.host, "127.0.0.1");
            assert_eq!(proxy.port, 8888);
            assert_eq!(proxy.username, "user");
            assert_eq!(proxy.password, "pwd");
        }

        // A null/empty host clears the proxy.
        set_proxy(Some(""), 0, "", "");
        assert!(global_config().proxy.is_none());
        set_proxy(None, 0, "", "");
        assert!(global_config().proxy.is_none());
    }

    #[test]
    fn dns_configs_are_validated_and_versioned() {
        let _guard = lock_config();
        reset_config();

        set_dns_configs(&[
            DnsConfig {
                host: "pinned.test".to_string(),
                port: 443,
                address: "10.1.2.3".to_string(),
            },
            // An empty host is dropped, like DynXX does.
            DnsConfig {
                host: String::new(),
                port: 443,
                address: "10.1.2.4".to_string(),
            },
            // So is an address no resolver could ever answer with.
            DnsConfig {
                host: "broken.test".to_string(),
                port: 0,
                address: "not-an-ip".to_string(),
            },
        ]);

        let (_, _, dns) = config_snapshot();
        assert_eq!(dns.len(), 1);
        assert_eq!(dns[0].host, "pinned.test");
        assert_eq!(dns[0].address, "10.1.2.3");

        let version = CONFIG_VERSION.load(Ordering::SeqCst);
        global_client().expect("client builds with DNS overrides");
        assert_eq!(CONFIG_VERSION.load(Ordering::SeqCst), version);

        // An empty list clears the overrides again.
        set_dns_configs(&[]);
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

        set_proxy(Some("127.0.0.1"), 1, "u", "p");
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

        let parts = vec![(
            "field".to_string(),
            b"data".to_vec(),
            Some("text/plain".to_string()),
            Some("a.txt".to_string()),
        )];
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
