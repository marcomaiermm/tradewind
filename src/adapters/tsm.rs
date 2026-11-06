use crate::adapters::{desktop, keyring};
use crate::i18n::Text;
use anyhow::{Context, Result, bail, ensure};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use chrono::Utc;
use flate2::read::GzDecoder;
use reqwest::{Client, Method, Url, header};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::{
    io::{BufRead, BufReader, Read, Write},
    net::TcpListener,
    time::{Duration, Instant},
};

pub const REALM_API: &str = "https://realm-api.tradeskillmaster.com/v2";
pub const BACKEND: &str = "https://desktop-backend.tradeskillmaster.com";
const AUTH: &str = "https://id.tradeskillmaster.com/realms/app/protocol/openid-connect";
const SOCKET: &str = "https://app-backend-sockets.tradeskillmaster.com/socket.io/";
const CLIENT_ID: &str = "desktop-app";
const SCOPE: &str = "openid roles profile email tsm-profile offline_access";
/// One keyring entry: the offline refresh token of the signed-in TSM account.
const KEYRING_ACCOUNT: &str = "desktop-app";
const MAX_BODY: usize = 128 * 1024 * 1024;
const LOGIN_WAIT: Duration = Duration::from_secs(300);
const CALLBACK_PAGE: &str = "<!doctype html><meta charset=utf-8><title>Tradewind</title><p>Signed in. You can close this tab and return to Tradewind.</p>";

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

/// Price data and addon zips come from DigitalOcean Spaces, backups from presigned URLs.
fn checked_url(url: &str) -> Result<Url> {
    let url = Url::parse(url)?;
    ensure!(
        url.scheme() == "https"
            && url.username().is_empty()
            && url.password().is_none()
            && url.port().is_none(),
        msg!(InvalidDownloadUrl)
    );
    let host = url.host_str().context(msg!(MissingDownloadHost))?;
    ensure!(
        ["digitaloceanspaces.com", "tradeskillmaster.com"]
            .iter()
            .any(|domain| host == *domain
                || host
                    .strip_suffix(domain)
                    .is_some_and(|rest| rest.ends_with('.'))),
        msg!(DownloadHostRejected, host = host)
    );
    Ok(url)
}
fn random_token() -> Result<String> {
    let mut bytes = [0u8; 32];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}
/// Serves the browser's redirect to `/auth/callback` and returns its authorization code.
fn await_callback(listener: TcpListener, state: &str) -> Result<String> {
    listener.set_nonblocking(true)?;
    let deadline = Instant::now() + LOGIN_WAIT;
    while Instant::now() < deadline {
        let mut stream = match listener.accept() {
            Ok((stream, _)) => stream,
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(100));
                continue;
            }
            Err(e) => return Err(e.into()),
        };
        stream.set_nonblocking(false)?;
        stream.set_read_timeout(Some(Duration::from_secs(5)))?;
        let mut line = String::new();
        // Browsers open speculative connections that never send a request.
        if BufReader::new(&stream).read_line(&mut line).is_err() {
            continue;
        }
        let target = line.split_whitespace().nth(1).unwrap_or("/");
        let Ok(url) = Url::parse(&format!("http://localhost{target}")) else {
            continue;
        };
        let (status, page) = if url.path() == "/auth/callback" {
            ("200 OK", CALLBACK_PAGE)
        } else {
            ("404 Not Found", "")
        };
        let _ = write!(
            stream,
            "HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nConnection: close\r\nContent-Length: {}\r\n\r\n{page}",
            page.len()
        );
        if url.path() != "/auth/callback" {
            continue;
        }
        let param = |name: &str| {
            url.query_pairs()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value.into_owned())
        };
        ensure!(
            param("state").as_deref() == Some(state),
            msg!(LoginRejected)
        );
        return param("code").context(msg!(LoginRejected));
    }
    bail!(msg!(LoginTimeout))
}
/// Claims of a JWT access token. The signature is the server's business, not ours.
fn claims(token: &str) -> Result<Value> {
    let payload = token.split('.').nth(1).context(msg!(InvalidSession))?;
    Ok(serde_json::from_slice(&URL_SAFE_NO_PAD.decode(payload)?)?)
}

/// One Engine.IO packet of a long-polling response, as far as the sync cares.
#[derive(Debug, PartialEq)]
pub enum Packet {
    Ping,
    /// Socket.IO connection refused, e.g. for a rejected token.
    Refused,
    Event(String, Value),
}
/// Packets of one polling response are joined by the record separator `\x1e`.
pub fn packets(body: &str) -> Vec<Packet> {
    body.split('\x1e')
        .filter_map(|packet| {
            if packet == "2" {
                return Some(Packet::Ping);
            }
            if packet.starts_with("44") {
                return Some(Packet::Refused);
            }
            let Value::Array(mut event) = serde_json::from_str(packet.strip_prefix("42")?).ok()?
            else {
                return None;
            };
            if event.len() < 2 {
                return None;
            }
            let payload = event.swap_remove(1);
            Some(Packet::Event(event[0].as_str()?.to_owned(), payload))
        })
        .collect()
}
/// What the socket pushes on connect: price strings, addon versions and the news.
#[derive(Debug, Default)]
pub struct Feed {
    pub realms: Vec<Value>,
    pub regions: Vec<Value>,
    pub addons: Vec<Value>,
    pub app_info: Value,
    addon_seen: bool,
}
impl Feed {
    pub fn absorb(&mut self, name: &str, payload: Value) {
        match name {
            "realm_auctiondb" | "realm_shopping" => self.realms.push(payload),
            "region_auctiondb" => self.regions.push(payload),
            "addon" => {
                self.addon_seen = true;
                if payload["isComplete"].as_bool() == Some(true) {
                    self.addons.clear();
                }
                self.addons
                    .extend(payload["data"].as_array().cloned().unwrap_or_default());
            }
            "app_info" => self.app_info = payload,
            _ => (),
        }
    }
    fn complete(&self) -> bool {
        self.addon_seen && !self.app_info.is_null()
    }
}

pub struct Api {
    client: Client,
    access: String,
    refresh: String,
    claims: Value,
    expires: i64,
    remember: bool,
}
impl Api {
    pub fn new() -> Result<Self> {
        let client = Client::builder()
            .user_agent(concat!("Tradewind/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(Duration::from_secs(15))
            .timeout(Duration::from_secs(120))
            .redirect(reqwest::redirect::Policy::none())
            .build()?;
        Ok(Self {
            client,
            access: String::new(),
            refresh: String::new(),
            claims: Value::Null,
            expires: 0,
            remember: false,
        })
    }
    pub fn logged_in(&self) -> bool {
        !self.access.is_empty()
    }
    pub fn premium(&self) -> bool {
        self.claims["realm_access"]["roles"]
            .as_array()
            .is_some_and(|roles| {
                roles
                    .iter()
                    .any(|r| matches!(r.as_str(), Some("premium-gold" | "premium-platinum")))
            })
    }
    pub fn user_id(&self) -> Result<&str> {
        self.claims["sub"].as_str().context(msg!(InvalidSession))
    }
    /// `release` plus the channels this account may use.
    pub fn channels(&self) -> Vec<String> {
        let mut channels = vec!["release".to_owned()];
        for role in self.claims["resource_access"]["app-addon-version"]["roles"]
            .as_array()
            .into_iter()
            .flatten()
        {
            if let Some(name) = role.as_str().and_then(|r| r.strip_prefix("channel/"))
                && !channels.iter().any(|c| c == name)
            {
                channels.push(name.to_owned());
            }
        }
        channels
    }
    async fn token(&self, form: &[(&str, &str)], operation: Text, rejected: Text) -> Result<Value> {
        let response = self
            .client
            .post(format!("{AUTH}/token"))
            .form(form)
            .send()
            .await
            .map_err(|e| transport_error(operation, e))?;
        if matches!(response.status().as_u16(), 400 | 401 | 403) {
            bail!(rejected);
        }
        self.read_json(response.error_for_status().map_err(|e| e.without_url())?)
            .await
    }
    /// Browser sign-in: authorization code with PKCE, received on a one-shot loopback listener.
    pub async fn login(&mut self, remember: bool) -> Result<Option<Text>> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        // Keycloak accepts any port, but only the host `localhost` and this exact path.
        let redirect = format!(
            "http://localhost:{}/auth/callback",
            listener.local_addr()?.port()
        );
        let (verifier, state) = (random_token()?, random_token()?);
        let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(&verifier));
        let url = Url::parse_with_params(
            &format!("{AUTH}/auth"),
            [
                ("client_id", CLIENT_ID),
                ("redirect_uri", &redirect),
                ("state", &state),
                ("response_mode", "query"),
                ("response_type", "code"),
                ("scope", SCOPE),
                ("code_challenge", &challenge),
                ("code_challenge_method", "S256"),
            ],
        )?;
        tokio::task::spawn_blocking(move || desktop::open(url.as_str())).await??;
        let code = tokio::task::spawn_blocking(move || await_callback(listener, &state)).await??;
        let token = self
            .token(
                &[
                    ("grant_type", "authorization_code"),
                    ("client_id", CLIENT_ID),
                    ("code", &code),
                    ("redirect_uri", &redirect),
                    ("code_verifier", &verifier),
                ],
                msg!(Authentication),
                msg!(LoginRejected),
            )
            .await?;
        self.remember = remember;
        self.apply_token(token)?;
        let token = self.refresh.clone();
        Ok(if remember {
            tokio::task::spawn_blocking(move || keyring::store(KEYRING_ACCOUNT, &token))
                .await?
                .err()
                .map(|_| msg!(KeyringLoginWarning))
        } else {
            // Clear a refresh token from an earlier remembered login.
            let _ = tokio::task::spawn_blocking(|| keyring::clear(KEYRING_ACCOUNT)).await;
            None
        })
    }
    fn apply_token(&mut self, token: Value) -> Result<()> {
        let access = token["access_token"]
            .as_str()
            .context(msg!(MissingAccessToken))?;
        self.claims = claims(access)?;
        self.access = access.to_owned();
        if let Some(refresh) = token["refresh_token"].as_str() {
            self.refresh = refresh.to_owned();
        }
        ensure!(!self.refresh.is_empty(), msg!(MissingRefreshToken));
        self.expires = Utc::now().timestamp() + token["expires_in"].as_i64().unwrap_or(300) - 30;
        Ok(())
    }
    pub async fn resume(&mut self) -> Result<()> {
        self.refresh = tokio::task::spawn_blocking(|| keyring::lookup(KEYRING_ACCOUNT)).await??;
        self.remember = true;
        self.renew().await
    }
    async fn renew(&mut self) -> Result<()> {
        let token = self
            .token(
                &[
                    ("grant_type", "refresh_token"),
                    ("client_id", CLIENT_ID),
                    ("refresh_token", &self.refresh),
                ],
                msg!(RefreshSession),
                msg!(SessionExpired),
            )
            .await
            .inspect_err(|_| self.access.clear())?;
        self.apply_token(token)?;
        if self.remember {
            let token = self.refresh.clone();
            tokio::task::spawn_blocking(move || keyring::store(KEYRING_ACCOUNT, &token)).await??;
        }
        Ok(())
    }
    pub async fn ensure_session(&mut self) -> Result<()> {
        ensure!(self.logged_in(), msg!(SignInFirst));
        if Utc::now().timestamp() >= self.expires {
            self.renew().await?;
        }
        Ok(())
    }
    pub async fn logout(&mut self) -> Result<()> {
        let refresh = std::mem::take(&mut self.refresh);
        self.access.clear();
        self.claims = Value::Null;
        self.expires = 0;
        if !refresh.is_empty() {
            // Best effort: the local sign-out must work offline too.
            let _ = self
                .client
                .post(format!("{AUTH}/revoke"))
                .form(&[
                    ("client_id", CLIENT_ID),
                    ("token", refresh.as_str()),
                    ("token_type_hint", "refresh_token"),
                ])
                .timeout(Duration::from_secs(5))
                .send()
                .await;
        }
        if std::mem::take(&mut self.remember) {
            tokio::task::spawn_blocking(|| keyring::clear(KEYRING_ACCOUNT)).await??;
        }
        Ok(())
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
    async fn json(&self, method: Method, url: &str, body: Option<Value>) -> Result<Value> {
        let mut request = self.client.request(method, url).bearer_auth(&self.access);
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request
            .send()
            .await
            .map_err(|e| transport_error("TSM-API", e))?;
        let status = response.status();
        if status.is_success() {
            let bytes = self.bytes_limited(response).await?;
            return Ok(if bytes.is_empty() {
                Value::Null
            } else {
                serde_json::from_slice(&bytes)?
            });
        }
        if status.as_u16() == 401 {
            bail!(msg!(SessionRejected));
        }
        // The server explains refusals such as the subscription limit. Cap it: it is remote input.
        let reason = self.read_json(response).await.ok().and_then(|v| {
            v["message"]
                .as_str()
                .map(|m| m.chars().take(200).collect::<String>())
        });
        match reason {
            Some(reason) => bail!(msg!(ServiceError, reason = reason)),
            None if status.as_u16() == 403 => bail!(msg!(SessionRejected)),
            None => bail!(msg!(HttpError)),
        }
    }
    pub async fn get(&self, url: &str) -> Result<Value> {
        self.json(Method::GET, url, None).await
    }
    pub async fn post(&self, url: &str, body: Value) -> Result<Value> {
        self.json(Method::POST, url, Some(body)).await
    }
    pub async fn delete(&self, url: &str) -> Result<Value> {
        self.json(Method::DELETE, url, None).await
    }
    /// Connects to the price socket, collects what it pushes and disconnects again.
    pub async fn feed(&self) -> Result<Feed> {
        tokio::time::timeout(Duration::from_secs(20), self.poll_feed())
            .await
            .map_err(|_| anyhow::anyhow!(msg!(TransportTimeout)))?
    }
    async fn socket(&self, request: reqwest::RequestBuilder) -> Result<String> {
        // The socket wants the raw token, without "Bearer ".
        let response = request
            .header(header::AUTHORIZATION, &self.access)
            .send()
            .await
            .map_err(|e| transport_error("TSM-Socket", e))?;
        if matches!(response.status().as_u16(), 401 | 403) {
            bail!(msg!(SessionRejected));
        }
        let response = response.error_for_status().map_err(|e| e.without_url())?;
        Ok(String::from_utf8(self.bytes_limited(response).await?)?)
    }
    async fn poll_feed(&self) -> Result<Feed> {
        let url = |sid: &str| format!("{SOCKET}?EIO=4&transport=polling{sid}");
        let open = self.socket(self.client.get(url(""))).await?;
        let open: Value = serde_json::from_str(
            open.split('\x1e')
                .next()
                .and_then(|p| p.strip_prefix('0'))
                .context(msg!(InvalidResponse))?,
        )?;
        let sid = open["sid"].as_str().context(msg!(InvalidResponse))?;
        ensure!(
            sid.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_'),
            msg!(InvalidResponse)
        );
        let url = url(&format!("&sid={sid}"));
        let send = |packet: &'static str| {
            self.socket(
                self.client
                    .post(&url)
                    .header(header::CONTENT_TYPE, "text/plain;charset=UTF-8")
                    .body(packet),
            )
        };
        // Socket.IO connect to the default namespace.
        send("40").await?;
        let mut feed = Feed::default();
        while !feed.complete() {
            for packet in packets(&self.socket(self.client.get(&url)).await?) {
                match packet {
                    Packet::Event(name, payload) => feed.absorb(&name, payload),
                    Packet::Ping => {
                        send("3").await?;
                    }
                    Packet::Refused => bail!(msg!(SessionRejected)),
                }
            }
        }
        Ok(feed)
    }
    /// Raw body of a presigned or CDN URL, without credentials.
    pub async fn fetch(&self, url: &str) -> Result<Vec<u8>> {
        let response = self
            .client
            .get(checked_url(url)?)
            .send()
            .await
            .map_err(|e| transport_error("TSM-Download", e))?
            .error_for_status()
            .map_err(|e| e.without_url())?;
        self.bytes_limited(response).await
    }
    pub async fn upload(&self, url: &str, bytes: Vec<u8>) -> Result<()> {
        self.client
            .put(checked_url(url)?)
            .body(bytes)
            .send()
            .await
            .map_err(|e| transport_error("TSM-Upload", e))?
            .error_for_status()
            .map_err(|e| e.without_url())?;
        Ok(())
    }
    pub async fn download(&self, url: &str) -> Result<String> {
        let bytes = self.fetch(url).await?;
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
}

#[cfg(test)]
mod fixtures {
    use super::*;
    use serde_json::json;
    #[test]
    fn engine_io_packets_are_split_decoded_and_pings_noticed() {
        let body = include_str!("../../tests/fixtures/socket-poll.txt");
        let found = packets(body);
        assert_eq!(found[0], Packet::Ping);
        assert_eq!(found.len(), 6);
        assert_eq!(
            packets("2\x1e42[\"app_info\",{\"news\":[]}]\x1e40\x1e44{}").len(),
            3
        );
        assert_eq!(
            packets("42[\"addon\",{\"isComplete\":true}]"),
            vec![Packet::Event("addon".into(), json!({"isComplete":true}))]
        );
        assert!(packets("42not json\x1e42[\"x\"]\x1e0{}").is_empty());
        let mut feed = Feed::default();
        for packet in found {
            if let Packet::Event(name, payload) = packet {
                assert!(!feed.complete());
                feed.absorb(&name, payload);
            }
        }
        assert!(feed.complete());
        assert_eq!(feed.realms.len(), 2);
        assert_eq!(feed.regions.len(), 1);
        assert_eq!(feed.addons.len(), 2);
        assert_eq!(feed.app_info["news"][0]["timestamp"], 1793577133);
    }
    #[test]
    fn claims_give_user_premium_and_channels() {
        let fixture = include_str!("../../tests/fixtures/claims.json");
        let token = format!("e30.{}.sig", URL_SAFE_NO_PAD.encode(fixture));
        let mut api = Api::new().unwrap();
        api.claims = claims(&token).unwrap();
        assert_eq!(
            api.user_id().unwrap(),
            "00000000-0000-4000-8000-000000000000"
        );
        assert!(!api.premium());
        assert_eq!(api.channels(), vec!["release"]);
        api.claims["realm_access"]["roles"] = json!(["premium-gold"]);
        api.claims["resource_access"]["app-addon-version"]["roles"] =
            json!(["channel/release", "channel/beta"]);
        assert!(api.premium());
        assert_eq!(api.channels(), vec!["release", "beta"]);
        assert!(claims("no-dots").is_err());
    }
    #[test]
    fn downloads_are_limited_to_https_spaces_and_tsm_hosts_without_leaking_signed_urls() {
        for url in [
            "https://tradeskillmaster.nyc3.digitaloceanspaces.com/pricing/a.txt?X-Amz-Signature=fixture-only",
            "https://tradeskillmaster.nyc3.cdn.digitaloceanspaces.com/addon-versions/a.zip",
            "https://data.tradeskillmaster.com/a.txt",
        ] {
            assert!(checked_url(url).is_ok(), "{url}");
        }
        for url in [
            "http://tradeskillmaster.nyc3.digitaloceanspaces.com/a.txt",
            "https://evildigitaloceanspaces.com/a.txt?X-Amz-Signature=fixture-only",
            "https://digitaloceanspaces.com.evil.example/a.txt?X-Amz-Signature=fixture-only",
            "https://app-cdn.tradeskillmaster.xyz/a.txt",
            "https://user:pw@a.digitaloceanspaces.com/a.txt",
            "https://a.digitaloceanspaces.com:8443/a.txt",
        ] {
            let error = checked_url(url)
                .err()
                .map(|e| e.to_string())
                .unwrap_or_default();
            assert!(!error.is_empty(), "accepted {url}");
            assert!(!error.contains("fixture-only"));
        }
    }
}
