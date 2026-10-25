use crate::adapters::keyring;
use crate::i18n::Text;
use anyhow::{Context, Result, bail, ensure};
use base64::{Engine, engine::general_purpose::URL_SAFE};
use chrono::Utc;
use flate2::{Compression, read::GzDecoder, write::GzEncoder};
use reqwest::{Client, Url};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    io::{Read, Write},
    time::Duration,
};

pub const PROTOCOL_VERSION: i64 = 41402;
const TOKEN_URL: &str = "https://id.tradeskillmaster.com/realms/app/protocol/openid-connect/token";
// Fixed protocol compatibility constant from AppAPI._make_request, not an account credential.
const SIGNING_SALT: &str = "3FB1CC5EDC5B43F21CB8ACC23B42B703";
const MAX_BODY: usize = 128 * 1024 * 1024;
// Additional price-data origin observed in the authenticated TSM status response.
const PRICE_CDN_HOST: &str = "app-cdn.tradeskillmaster.xyz";

fn transport_error(operation: impl Into<Text>, error: reqwest::Error) -> anyhow::Error {
    let operation = operation.into();
    let host = error
        .url()
        .and_then(Url::host_str)
        .unwrap_or("TSM-Server")
        .to_owned();
    let error = error.without_url();
    let reason = if error.is_timeout() {
        msg!(TransportTimeout)
    } else {
        transport_cause(&error).unwrap_or(if error.is_connect() {
            msg!(ConnectionFailed)
        } else {
            msg!(RequestFailed)
        })
    };
    // Emit only classified causes and the host: URLs, payloads and raw server text may contain tokens.
    anyhow::anyhow!(msg!(
        TransportError,
        operation = operation,
        host = host,
        reason = reason
    ))
}

fn transport_cause(error: &(dyn std::error::Error + 'static)) -> Option<Text> {
    use std::io::ErrorKind;
    let mut source = Some(error);
    while let Some(error) = source {
        let reason = error
            .downcast_ref::<std::io::Error>()
            .and_then(|error| match error.kind() {
                ErrorKind::ConnectionRefused => Some(msg!(ConnectionRefused)),
                ErrorKind::ConnectionReset | ErrorKind::ConnectionAborted => {
                    Some(msg!(ConnectionReset))
                }
                ErrorKind::UnexpectedEof | ErrorKind::BrokenPipe => Some(msg!(ConnectionEnded)),
                ErrorKind::NetworkUnreachable | ErrorKind::HostUnreachable => {
                    Some(msg!(Unreachable))
                }
                ErrorKind::PermissionDenied => Some(msg!(NetworkBlocked)),
                ErrorKind::TimedOut => Some(msg!(ConnectionTimeout)),
                _ => None,
            });
        if reason.is_some() {
            return reason;
        }
        // reqwest 0.13.1 has no DNS/TLS predicates, so recognize native transport causes without logging them.
        let description = error.to_string().to_ascii_lowercase();
        if description.contains("hostname mismatch") {
            return Some(msg!(CertificateName));
        }
        if description.contains("certificate has expired") {
            return Some(msg!(CertificateExpired));
        }
        if description.contains("certificate verify failed") {
            return Some(msg!(CertificateUnverified));
        }
        if description.contains("dns error") || description.contains("failed to lookup address") {
            return Some(msg!(DnsFailed));
        }
        if description.contains("http2") || description.contains("http/2") {
            return Some(msg!(Http2Failed));
        }
        if description.contains("ssl") || description.contains("tls") {
            return Some(msg!(TlsFailed));
        }
        source = error.source();
    }
    None
}

pub fn encoded(s: &str) -> String {
    URL_SAFE.encode(s.as_bytes())
}
pub fn signature(time: i64) -> String {
    format!(
        "{:x}",
        Sha256::digest(format!("{PROTOCOL_VERSION}:{time}:{SIGNING_SALT}"))
    )
}
pub enum Body {
    Json(Value),
    Text(String),
    Binary(Vec<u8>),
    GzippedJsonBinary(Value),
}
pub struct Api {
    client: Client,
    pub user: Value,
    oauth: Value,
    expires: i64,
    pub email: String,
    remember: bool,
    legacy_http: bool,
}
impl Api {
    pub fn new(legacy_http: bool) -> Result<Self> {
        let client = Client::builder()
            .user_agent(concat!("Tradewind/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(Duration::from_secs(15))
            .timeout(Duration::from_secs(120))
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        Ok(Self {
            client,
            user: json!({"session":"", "isPremium":false}),
            oauth: Value::Null,
            expires: 0,
            email: String::new(),
            remember: false,
            legacy_http,
        })
    }
    fn token_request(&self, form: &[(&str, &str)]) -> reqwest::RequestBuilder {
        // Passwords and refresh tokens always go to the verified HTTPS identity endpoint.
        self.client.post(TOKEN_URL).form(form)
    }
    pub fn logged_in(&self) -> bool {
        self.user["session"].as_str().is_some_and(|s| !s.is_empty())
    }
    pub fn premium(&self) -> bool {
        self.user["isPremium"].as_bool().unwrap_or(false)
    }
    pub async fn login(
        &mut self,
        email: String,
        password: String,
        remember: bool,
    ) -> Result<Option<Text>> {
        ensure!(
            email.contains('@') && !password.is_empty(),
            msg!(CredentialsRequired)
        );
        let form = [
            ("username", email.as_str()),
            ("password", password.as_str()),
            ("client_id", "legacy-desktop-app"),
            ("grant_type", "password"),
            ("code", ""),
            ("redirect_uri", ""),
            ("scope", "openid"),
        ];
        let response = self
            .token_request(&form)
            .send()
            .await
            .map_err(|e| transport_error(msg!(Authentication), e))?;
        if matches!(response.status().as_u16(), 400 | 401 | 403) {
            bail!(msg!(LoginRejected));
        }
        let response = response.error_for_status().map_err(|e| e.without_url())?;
        let token = self.read_json(response).await?;
        self.email = email;
        self.remember = remember;
        self.apply_token(token).await?;
        let warning = if remember {
            let email = self.email.clone();
            let token = self.oauth["refresh_token"]
                .as_str()
                .context(msg!(MissingRefreshToken))?
                .to_owned();
            tokio::task::spawn_blocking(move || keyring::store(&email, &token))
                .await?
                .err()
                .map(|_| msg!(KeyringLoginWarning))
        } else {
            let email = self.email.clone();
            // Clear a refresh token from an earlier remembered login to this same account.
            let _ = tokio::task::spawn_blocking(move || keyring::clear(&email)).await;
            None
        };
        Ok(warning)
    }
    async fn apply_token(&mut self, token: Value) -> Result<()> {
        let access = token["access_token"]
            .as_str()
            .context(msg!(MissingAccessToken))?
            .to_owned();
        let user = self
            .json(
                &["auth"],
                Some(Body::Json(json!({"token":access}))),
                None,
                None,
            )
            .await?;
        ensure!(
            user["session"].as_str().is_some_and(|s| !s.is_empty()),
            msg!(InvalidSession)
        );
        self.expires = Utc::now().timestamp() + token["expires_in"].as_i64().unwrap_or(300) - 30;
        self.oauth = token;
        self.user = user;
        Ok(())
    }
    pub async fn resume(&mut self, email: String) -> Result<()> {
        let lookup_email = email.clone();
        let token = tokio::task::spawn_blocking(move || keyring::lookup(&lookup_email)).await??;
        self.email = email;
        self.remember = true;
        self.oauth = json!({"refresh_token":token});
        self.refresh().await
    }
    async fn refresh(&mut self) -> Result<()> {
        let refresh = self.oauth["refresh_token"]
            .as_str()
            .context(msg!(SessionExpired))?
            .to_owned();
        let response = self
            .token_request(&[
                ("client_id", "legacy-desktop-app"),
                ("grant_type", "refresh_token"),
                ("refresh_token", refresh.as_str()),
            ])
            .send()
            .await
            .map_err(|e| transport_error(msg!(RefreshSession), e))?;
        if matches!(response.status().as_u16(), 400 | 401 | 403) {
            self.user = Value::Null;
            bail!(msg!(SessionExpired));
        }
        let token = self
            .read_json(response.error_for_status().map_err(|e| e.without_url())?)
            .await?;
        self.apply_token(token).await?;
        if self.remember {
            let email = self.email.clone();
            let token = self.oauth["refresh_token"]
                .as_str()
                .context(msg!(RefreshTokenMissing))?
                .to_owned();
            tokio::task::spawn_blocking(move || keyring::store(&email, &token)).await??;
        }
        Ok(())
    }
    pub async fn ensure_session(&mut self) -> Result<()> {
        ensure!(self.logged_in(), msg!(SignInFirst));
        if Utc::now().timestamp() >= self.expires {
            self.refresh().await?;
        }
        Ok(())
    }
    pub async fn logout(&mut self) -> Result<()> {
        self.user = Value::Null;
        self.oauth = Value::Null;
        self.expires = 0;
        let email = std::mem::take(&mut self.email);
        if std::mem::take(&mut self.remember) {
            tokio::task::spawn_blocking(move || keyring::clear(&email)).await??;
        }
        Ok(())
    }
    fn url(&self, segments: &[&str]) -> Result<Url> {
        let endpoint = *segments.first().context(msg!(EndpointMissing))?;
        let subdomain = if ["auth", "log", "realms2"].contains(&endpoint) {
            "app-server"
        } else {
            self.user["endpointSubdomains"][endpoint]
                .as_str()
                .context(msg!(ServiceDisabled))?
        };
        ensure!(
            !subdomain.is_empty()
                && subdomain
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-'),
            msg!(InvalidServerName)
        );
        let scheme = if self.legacy_http { "http" } else { "https" };
        let mut url = Url::parse(&format!("{scheme}://{subdomain}.tradeskillmaster.com/v2/"))?;
        {
            let mut path = url
                .path_segments_mut()
                .map_err(|_| anyhow::anyhow!(msg!(InvalidApiUrl)))?;
            path.pop_if_empty();
            for segment in segments {
                path.push(segment);
            }
        }
        Ok(url)
    }
    fn envelope(
        &self,
        segments: &[&str],
        body: Option<Body>,
        channel: Option<&str>,
        tsm_version: Option<&str>,
        time: i64,
    ) -> Result<reqwest::RequestBuilder> {
        let url = self.url(segments)?;
        let mut query = vec![
            (
                "session",
                self.user["session"].as_str().unwrap_or("").to_owned(),
            ),
            ("version", PROTOCOL_VERSION.to_string()),
            ("time", time.to_string()),
            ("token", signature(time)),
        ];
        if let Some(channel) = channel {
            query.push(("channel", channel.into()));
        }
        if let Some(version) = tsm_version {
            query.push(("tsm_version", version.into()));
        }
        let mut request = if body.is_some() {
            self.client.post(url)
        } else {
            self.client.get(url)
        };
        if let Some(body) = body {
            let (bytes, content_type, gzip) = match body {
                Body::Binary(b) => (b, "application/octet-stream", false),
                Body::Json(v) => (serde_json::to_vec(&v)?, "application/json", true),
                Body::Text(s) => (s.into_bytes(), "text/plain", true),
                Body::GzippedJsonBinary(v) => {
                    (serde_json::to_vec(&v)?, "application/octet-stream", true)
                }
            };
            request = request.header("Content-Type", content_type);
            if gzip {
                let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
                encoder.write_all(&bytes)?;
                request = request
                    .header("Content-Encoding", "gzip")
                    .body(encoder.finish()?);
            } else {
                request = request.body(bytes);
            }
        }
        Ok(request.query(&query))
    }
    async fn request(
        &self,
        segments: &[&str],
        body: Option<Body>,
        channel: Option<&str>,
        tsm_version: Option<&str>,
    ) -> Result<reqwest::Response> {
        let response = self
            .envelope(segments, body, channel, tsm_version, Utc::now().timestamp())?
            .send()
            .await
            .map_err(|e| transport_error("TSM-App-API", e))?;
        if matches!(response.status().as_u16(), 401 | 403) {
            bail!(msg!(SessionRejected));
        }
        response
            .error_for_status()
            .map_err(|e| e.without_url())
            .context(msg!(HttpError))
    }
    async fn bytes_limited(&self, mut response: reqwest::Response) -> Result<Vec<u8>> {
        ensure!(
            response
                .content_length()
                .is_none_or(|n| n <= MAX_BODY as u64),
            msg!(ResponseTooLarge)
        );
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|e| e.without_url())? {
            ensure!(
                bytes.len() + chunk.len() <= MAX_BODY,
                msg!(ResponseTooLarge)
            );
            bytes.extend_from_slice(&chunk);
        }
        Ok(bytes)
    }
    async fn read_json(&self, response: reqwest::Response) -> Result<Value> {
        Ok(serde_json::from_slice(
            &self.bytes_limited(response).await?,
        )?)
    }
    pub async fn get(&self, segments: &[&str]) -> Result<Value> {
        self.json(segments, None, None, None).await
    }
    pub async fn post(&self, segments: &[&str], body: Body) -> Result<Value> {
        self.json(segments, Some(body), None, None).await
    }
    /// Account, realm and addon status for the update channel and installed TSM version.
    pub async fn status(&self, channel: &str, tsm_version: Option<&str>) -> Result<Value> {
        self.json(&["status"], None, Some(channel), tsm_version)
            .await
    }
    async fn json(
        &self,
        segments: &[&str],
        body: Option<Body>,
        channel: Option<&str>,
        tsm_version: Option<&str>,
    ) -> Result<Value> {
        let response = self.request(segments, body, channel, tsm_version).await?;
        let value = self.read_json(response).await?;
        if value["success"].as_bool() == Some(true) {
            return Ok(value);
        }
        // The official client shows the server's `error` text. Cap it since it is remote input.
        match value["error"]
            .as_str()
            .map(str::trim)
            .filter(|e| !e.is_empty())
        {
            Some(reason) => bail!(msg!(
                ServiceError,
                reason = reason.chars().take(200).collect::<String>()
            )),
            None => bail!(msg!(InvalidResponse)),
        }
    }
    pub async fn binary(&self, segments: &[&str], channel: Option<&str>) -> Result<Vec<u8>> {
        let response = self.request(segments, None, channel, None).await?;
        let content_type = response
            .headers()
            .get("Content-Type")
            .and_then(|s| s.to_str().ok())
            .unwrap_or("");
        ensure!(
            content_type.contains("application/zip")
                || content_type.contains("application/octet-stream"),
            msg!(MissingArchive)
        );
        self.bytes_limited(response).await
    }
    fn download_url(&self, url: &str) -> Result<Url> {
        let mut url = Url::parse(url)?;
        if url.scheme() == "http" && !self.legacy_http {
            url.set_scheme("https")
                .map_err(|_| anyhow::anyhow!(msg!(InvalidUrlScheme)))?;
        }
        ensure!(
            (url.scheme() == "https" || (self.legacy_http && url.scheme() == "http"))
                && url.username().is_empty()
                && url.password().is_none()
                && url.port().is_none(),
            msg!(InvalidDownloadUrl)
        );
        let host = url.host_str().context(msg!(MissingDownloadHost))?;
        ensure!(
            host == PRICE_CDN_HOST
                || host == "tradeskillmaster.com"
                || host.ends_with(".tradeskillmaster.com"),
            msg!(DownloadHostRejected, host = host)
        );
        Ok(url)
    }
    pub async fn download(&self, url: &str) -> Result<String> {
        let response = self
            .client
            .get(self.download_url(url)?)
            .send()
            .await
            .map_err(|e| transport_error("TSM-Download", e))?
            .error_for_status()
            .map_err(|e| e.without_url())?;
        let bytes = self.bytes_limited(response).await?;
        let text = if bytes.starts_with(&[0x1f, 0x8b]) {
            let mut decoded = Vec::new();
            GzDecoder::new(bytes.as_slice())
                .take(MAX_BODY as u64 + 1)
                .read_to_end(&mut decoded)?;
            ensure!(decoded.len() <= MAX_BODY, msg!(PriceDataTooLarge));
            String::from_utf8(decoded)?
        } else {
            String::from_utf8(bytes)?
        };
        ensure!(
            !text.trim().is_empty() && !text.trim_start().starts_with('<'),
            msg!(InvalidPriceData)
        );
        Ok(text.trim().to_owned())
    }
    pub async fn upload_newer(&self, segments: &[&str], time: i64, body: Body) -> Result<bool> {
        let status = self.get(segments).await?;
        let last = status["lastUpload"]
            .as_i64()
            .context(msg!(MissingUploadTime))?;
        if time <= last {
            return Ok(false);
        }
        self.post(segments, body).await?;
        Ok(true)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn network_failures_show_the_cause_without_request_secrets() {
        let refused = std::io::Error::new(
            std::io::ErrorKind::ConnectionRefused,
            "connect failed: https://example.com/auth?token=PRIVATE",
        );
        let wrapped = anyhow::Error::new(refused).context("connect failed");
        assert_eq!(
            transport_cause(wrapped.as_ref()),
            Some(msg!(ConnectionRefused))
        );
        let tls = std::io::Error::other("certificate verify failed; token=PRIVATE");
        assert_eq!(transport_cause(&tls), Some(msg!(CertificateUnverified)));
        let mismatch = std::io::Error::other("certificate verify failed (hostname mismatch)");
        assert_eq!(transport_cause(&mismatch), Some(msg!(CertificateName)));
        let expired = std::io::Error::other("certificate verify failed (certificate has expired)");
        assert_eq!(transport_cause(&expired), Some(msg!(CertificateExpired)));
        let dns = std::io::Error::other("dns error: failed to lookup address information");
        assert_eq!(transport_cause(&dns), Some(msg!(DnsFailed)));
        let error = Client::new()
            .get("https://[invalid")
            .build()
            .unwrap_err()
            .with_url(Url::parse("https://USER:PRIVATE@app-server.tradeskillmaster.com/v2/auth?session=SESSION&token=TOKEN").unwrap());
        let error = transport_error("TSM-App-API", error);
        let displayed = format!("{error:#} {error:?}");
        assert!(displayed.contains("app-server.tradeskillmaster.com"));
        for secret in ["USER", "PRIVATE", "SESSION", "TOKEN", "/v2/auth"] {
            assert!(!displayed.contains(secret), "leaked {secret}");
        }
    }
    #[test]
    fn protocol_paths_and_signatures() {
        assert_eq!(encoded("Realm ü"), "UmVhbG0gw7w=");
        assert_eq!(
            signature(1700000000),
            "ea86bf52b7be5e219b7985e645db45e01db8cbd6139e330fb107842d2528442f"
        );
        let mut api = Api::new(false).unwrap();
        api.user = json!({"endpointSubdomains":{"backup":"evil.example"}});
        assert!(api.url(&["backup"]).is_err());
        assert_eq!(
            api.url(&["realms2", "add", "retail", "42"]).unwrap().path(),
            "/v2/realms2/add/retail/42"
        );
    }
}

#[cfg(test)]
mod fixtures {
    use super::*;
    #[test]
    fn price_cdn_is_allowed_without_opening_other_xyz_hosts_or_leaking_signed_urls() {
        let fixture: Value =
            serde_json::from_str(include_str!("../../tests/fixtures/protocol.json")).unwrap();
        let origin = Url::parse(fixture["cdnPriceUrl"].as_str().unwrap()).unwrap();
        for legacy_http in [false, true] {
            let api = Api::new(legacy_http).unwrap();
            let mut expected = origin.clone();
            if !legacy_http {
                expected.set_scheme("https").unwrap();
            }
            assert_eq!(api.download_url(origin.as_str()).unwrap(), expected);
            for host in [
                "app-cdn.tradeskillmaster.xyz.evil.example",
                "other.tradeskillmaster.xyz",
                "tradeskillmaster.xyz",
                "evil-app-cdn.tradeskillmaster.xyz",
            ] {
                let mut untrusted = origin.clone();
                untrusted.set_host(Some(host)).unwrap();
                let error = api
                    .download_url(untrusted.as_str())
                    .unwrap_err()
                    .to_string();
                assert!(error.contains(host));
                assert!(!error.contains("signature"));
                assert!(!error.contains("fixture-only"));
                assert!(!error.contains("fixture_gzip"));
            }
        }
    }
    #[test]
    fn legacy_http_is_explicit_and_oauth_stays_on_https() {
        let fixture: Value =
            serde_json::from_str(include_str!("../../tests/fixtures/protocol.json")).unwrap();
        for legacy_http in [false, true] {
            let mut api = Api::new(legacy_http).unwrap();
            api.user = json!({"endpointSubdomains":{"backup":"app-backup"}});
            let request = api
                .envelope(
                    &["auth"],
                    Some(Body::Json(fixture["authBody"].clone())),
                    None,
                    None,
                    fixture["time"].as_i64().unwrap(),
                )
                .unwrap()
                .build()
                .unwrap();
            let expected_scheme = if legacy_http { "http" } else { "https" };
            let mut actual = request.url().clone();
            actual.set_query(None);
            let mut expected = Url::parse(fixture["legacyApiUrl"].as_str().unwrap()).unwrap();
            expected.set_scheme(expected_scheme).unwrap();
            assert_eq!(actual, expected);
            assert_eq!(api.url(&["backup"]).unwrap().scheme(), expected_scheme);
            for form in [
                vec![("grant_type", "password"), ("password", "fixture-password")],
                vec![
                    ("grant_type", "refresh_token"),
                    ("refresh_token", "fixture-refresh"),
                ],
            ] {
                let request = api.token_request(&form).build().unwrap();
                assert_eq!(
                    request.url().as_str(),
                    fixture["oauthUrl"].as_str().unwrap()
                );
                assert_eq!(request.method(), reqwest::Method::POST);
            }
            assert_eq!(
                api.download_url("http://data.tradeskillmaster.com/data.txt")
                    .unwrap()
                    .scheme(),
                expected_scheme
            );
            // A server-provided HTTPS URL never gets downgraded, including in legacy mode.
            assert_eq!(
                api.download_url("https://data.tradeskillmaster.com/data.txt")
                    .unwrap()
                    .scheme(),
                "https"
            );
            for url in [
                "http://tradeskillmaster.com.evil.example/data.txt",
                "https://evil.example/data.txt",
                "http://user:secret@data.tradeskillmaster.com/data.txt",
                "http://data.tradeskillmaster.com:8080/data.txt",
                "https://data.tradeskillmaster.com:80/data.txt",
                "ftp://data.tradeskillmaster.com/data.txt",
            ] {
                assert!(api.download_url(url).is_err(), "accepted {url}");
            }
        }
    }
    #[test]
    fn signed_gzip_and_binary_envelopes_match_legacy_fixture() {
        let fixture: Value =
            serde_json::from_str(include_str!("../../tests/fixtures/protocol.json")).unwrap();
        let api = Api::new(false).unwrap();
        let request = api
            .envelope(
                &["auth"],
                Some(Body::Json(fixture["authBody"].clone())),
                Some("release"),
                Some("4.14.2"),
                fixture["time"].as_i64().unwrap(),
            )
            .unwrap()
            .build()
            .unwrap();
        assert_eq!(request.url().scheme(), "https");
        assert_eq!(request.method(), reqwest::Method::POST);
        assert_eq!(request.url().path(), "/v2/auth");
        let query: std::collections::BTreeMap<_, _> = request.url().query_pairs().collect();
        assert_eq!(query["version"], fixture["version"].to_string());
        assert_eq!(query["token"], fixture["signature"].as_str().unwrap());
        assert_eq!(query["session"], "");
        assert_eq!(query["channel"], "release");
        assert_eq!(query["tsm_version"], "4.14.2");
        assert_eq!(request.headers()["content-encoding"], "gzip");
        assert_eq!(request.headers()["content-type"], "application/json");
        let body = request.body().unwrap().as_bytes().unwrap();
        let json: Value = serde_json::from_reader(GzDecoder::new(body)).unwrap();
        assert_eq!(json, fixture["authBody"]);
        let request = api
            .envelope(&["auth"], Some(Body::Binary(vec![1, 2, 3])), None, None, 1)
            .unwrap()
            .build()
            .unwrap();
        assert!(!request.headers().contains_key("content-encoding"));
        assert_eq!(request.body().unwrap().as_bytes().unwrap(), [1, 2, 3]);
        let request = api
            .envelope(
                &["auth"],
                Some(Body::GzippedJsonBinary(json!({"tsmDB":{}}))),
                None,
                None,
                1,
            )
            .unwrap()
            .build()
            .unwrap();
        assert_eq!(request.headers()["content-encoding"], "gzip");
        assert_eq!(
            request.headers()["content-type"],
            "application/octet-stream"
        );
    }
}
