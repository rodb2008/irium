use reqwest::Client;
use serde::de::DeserializeOwned;
use serde::Serialize;

#[derive(Clone)]
pub struct RpcClient {
    client: Client,
    base: String,
    token: Option<String>,
}

impl RpcClient {
    pub fn new(base: &str, token: Option<String>, ca_path: Option<String>, allow_insecure: bool) -> Result<Self, String> {
        let mut builder = Client::builder();
        if let Some(path) = ca_path {
            let pem = std::fs::read(&path).map_err(|e| format!("read CA {path}: {e}"))?;
            let cert = reqwest::Certificate::from_pem(&pem).map_err(|e| format!("invalid CA {path}: {e}"))?;
            builder = builder.add_root_certificate(cert);
        }
        if allow_insecure {
            builder = builder.danger_accept_invalid_certs(true);
        }
        let client = builder.build().map_err(|e| e.to_string())?;
        Ok(Self {
            client,
            base: base.trim_end_matches('/').to_string(),
            token,
        })
    }

    fn url(&self, path: &str) -> String {
        format!("{}/{}", self.base, path.trim_start_matches('/'))
    }

    pub async fn get_json<T: DeserializeOwned>(&self, path: &str) -> Result<T, String> {
        let url = self.url(path);
        let mut req = self.client.get(url);
        if let Some(token) = &self.token {
            req = req.bearer_auth(token);
        }
        let resp = req.send().await.map_err(|e| e.to_string())?;
        if !resp.status().is_success() {
            return Err(format!("HTTP {}", resp.status()));
        }
        resp.json::<T>().await.map_err(|e| e.to_string())
    }

    pub async fn post_json<T: DeserializeOwned, B: Serialize>(&self, path: &str, body: &B) -> Result<T, String> {
        let url = self.url(path);
        let mut req = self.client.post(url).json(body);
        if let Some(token) = &self.token {
            req = req.bearer_auth(token);
        }
        let resp = req.send().await.map_err(|e| e.to_string())?;
        if !resp.status().is_success() {
            return Err(format!("HTTP {}", resp.status()));
        }
        resp.json::<T>().await.map_err(|e| e.to_string())
    }
}
