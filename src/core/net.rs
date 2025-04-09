use std::collections::HashMap;
use std::path::Path;
use reqwest::Client;
use reqwest::header::HeaderMap;
use reqwest::multipart;
use futures::StreamExt;
use std::borrow::Borrow;
use serde_json::Value;

pub struct HttpClient {
    client: Client,
}

pub struct HttpResponse {
    pub status: reqwest::StatusCode,
    pub headers: HeaderMap,
    pub body: Option<String>,
}

impl HttpClient {
    pub fn new(ca_cert_path: Option<&Path>) -> Result<Self, Box<dyn std::error::Error>> {
        let mut builder = reqwest::Client::builder()
            .danger_accept_invalid_certs(false);

        if let Some(cert_path) = ca_cert_path {
            let cert = std::fs::read(cert_path)?;
            builder = builder.add_root_certificate(
                reqwest::Certificate::from_pem(&cert)?
            );
        }

        Ok(Self {
            client: builder.build()?
        })
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
        let mut request = self.client.get(url);

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
        let mut request = self.client.post(url);

        if let Some(headers_map) = headers {
            for (key, value) in headers_map {
                request = request.header(key.borrow(), value.borrow());
            }
        }

        if let Some(params_map) = params {
            let json_map = params_map.into_iter()
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
        let mut request = self.client.get(url);

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
        parts: Vec<(String, Vec<u8>, Option<String>, Option<String>)>,
    ) -> Result<HttpResponse, Box<dyn std::error::Error>>
    where
        K: Borrow<str>,
        V: Borrow<str>,
    {
        let mut request = self.client.post(url);

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