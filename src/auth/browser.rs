use std::{net::SocketAddr, time::Duration};

use rand::Rng;
use serde::Deserialize;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};
use zeroize::Zeroizing;

use super::{AuthError, DeveloperTokenProvider, SecretToken};

const AUTHORIZATION_TIMEOUT: Duration = Duration::from_secs(5 * 60);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_REQUEST_SIZE: usize = 64 * 1024;
const MAX_TOKEN_SIZE: usize = 32 * 1024;

pub struct BrowserAuthorization;

impl BrowserAuthorization {
    pub async fn authorize<P: DeveloperTokenProvider>(
        provider: &P,
    ) -> Result<SecretToken, AuthError> {
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .map_err(AuthError::BrowserServer)?;
        let address = listener.local_addr().map_err(AuthError::BrowserServer)?;
        let origin = format!("http://127.0.0.1:{}", address.port());
        let nonce = random_nonce();
        let developer_token = provider.token(Some(&origin))?;
        let authorization_path = format!("/authorize/{nonce}");
        let callback_path = format!("/callback/{nonce}");
        let url = format!("{origin}{authorization_path}");
        let page = authorization_page(&developer_token, &callback_path, &nonce)?;

        println!("Open this local authorization URL if the browser does not open:\n{url}");
        open_browser(&url);

        tokio::time::timeout(
            AUTHORIZATION_TIMEOUT,
            wait_for_token(
                listener,
                address,
                &origin,
                &authorization_path,
                &callback_path,
                &nonce,
                &page,
            ),
        )
        .await
        .map_err(|_| AuthError::BrowserTimeout)?
    }
}

fn random_nonce() -> String {
    let mut bytes = [0_u8; 32];
    rand::rng().fill(&mut bytes);
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn authorization_page(
    developer_token: &SecretToken,
    callback_path: &str,
    nonce: &str,
) -> Result<Zeroizing<String>, AuthError> {
    let token_json = Zeroizing::new(serde_json::to_string(developer_token.expose()).map_err(
        |_| AuthError::BrowserAuthorization("could not prepare authorization".to_owned()),
    )?);
    let callback_json = serde_json::to_string(callback_path).map_err(|_| {
        AuthError::BrowserAuthorization("could not prepare authorization".to_owned())
    })?;
    let nonce_json = serde_json::to_string(nonce).map_err(|_| {
        AuthError::BrowserAuthorization("could not prepare authorization".to_owned())
    })?;
    let page = AUTHORIZATION_HTML
        .replace("__CALLBACK_PATH__", &callback_json)
        .replace("__NONCE__", &nonce_json);
    Ok(Zeroizing::new(
        page.replace("__DEVELOPER_TOKEN__", token_json.as_str()),
    ))
}

const AUTHORIZATION_HTML: &str = r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width,initial-scale=1">
  <meta name="referrer" content="no-referrer">
  <meta http-equiv="Content-Security-Policy" content="default-src 'none'; script-src 'self' https://js-cdn.music.apple.com 'unsafe-inline'; connect-src https: 'self'; frame-src https://*.apple.com; style-src 'unsafe-inline'; img-src data: https:; frame-ancestors 'none'">
  <title>apple-music-tui authorization</title>
  <style>body{font:16px system-ui;max-width:42rem;margin:4rem auto;padding:0 1rem;color:#202124}button{font:inherit;padding:.7rem 1rem}#status{margin-top:1rem}</style>
</head>
<body>
  <h1>Authorize apple-music-tui</h1>
  <p>This local page uses Apple's MusicKit JS consent screen. The resulting Music User Token is sent only to the loopback helper and stored in macOS Keychain.</p>
  <button id="authorize" type="button">Authorize Apple Music</button>
  <p id="status" role="status">Ready.</p>
  <script src="https://js-cdn.music.apple.com/musickit/v3/musickit.js"></script>
  <script>
    const developerToken = __DEVELOPER_TOKEN__;
    const callbackPath = __CALLBACK_PATH__;
    const nonce = __NONCE__;
    const status = document.getElementById('status');
    document.getElementById('authorize').addEventListener('click', async () => {
      try {
        status.textContent = 'Waiting for Apple Music authorization…';
        await MusicKit.configure({developerToken, app: {name: 'apple-music-tui', build: '0.1.0'}});
        const music = MusicKit.getInstance();
        const result = await music.authorize();
        const musicUserToken = result || music.musicUserToken;
        if (!musicUserToken) throw new Error('MusicKit returned no user token');
        const response = await fetch(callbackPath, {
          method: 'POST',
          headers: {'Content-Type': 'application/json', 'X-Apple-Music-TUI-Nonce': nonce},
          body: JSON.stringify({musicUserToken})
        });
        if (!response.ok) throw new Error('The local helper rejected the callback');
        status.textContent = 'Authorization complete. You may close this window.';
      } catch (_) {
        status.textContent = 'Authorization failed. Return to the terminal for guidance.';
        try {
          await fetch(callbackPath, {
            method: 'POST',
            headers: {'Content-Type': 'application/json', 'X-Apple-Music-TUI-Nonce': nonce},
            body: JSON.stringify({authorizationFailed: true})
          });
        } catch (_) {}
      }
    });
  </script>
</body>
</html>"#;

#[cfg(target_os = "macos")]
fn open_browser(url: &str) {
    if let Err(error) = std::process::Command::new("/usr/bin/open").arg(url).spawn() {
        tracing::debug!(%error, "could not launch the default browser");
    }
}

#[cfg(not(target_os = "macos"))]
fn open_browser(_url: &str) {}

async fn wait_for_token(
    listener: TcpListener,
    address: SocketAddr,
    origin: &str,
    authorization_path: &str,
    callback_path: &str,
    nonce: &str,
    page: &str,
) -> Result<SecretToken, AuthError> {
    loop {
        let (stream, peer) = listener.accept().await.map_err(AuthError::BrowserServer)?;
        if !peer.ip().is_loopback() {
            continue;
        }
        let request = tokio::time::timeout(
            REQUEST_TIMEOUT,
            handle_request(
                stream,
                address,
                origin,
                authorization_path,
                callback_path,
                nonce,
                page,
            ),
        )
        .await;
        match request {
            Err(_) => continue,
            Ok(result) => match result {
                Ok(Some(token)) => return Ok(token),
                Ok(None) | Err(AuthError::BrowserAuthorization(_)) => continue,
                Err(AuthError::BrowserRejected) => return Err(AuthError::BrowserRejected),
                Err(error) => return Err(error),
            },
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct CallbackPayload {
    music_user_token: Option<String>,
    #[serde(default)]
    authorization_failed: bool,
}

async fn handle_request(
    mut stream: TcpStream,
    address: SocketAddr,
    origin: &str,
    authorization_path: &str,
    callback_path: &str,
    nonce: &str,
    page: &str,
) -> Result<Option<SecretToken>, AuthError> {
    let request = read_request(&mut stream).await?;
    let expected_host = format!("127.0.0.1:{}", address.port());
    let host_is_valid = request.header("host") == Some(expected_host.as_str());

    if request.method == "GET" && request.path == authorization_path && host_is_valid {
        write_response(
            &mut stream,
            200,
            "text/html; charset=utf-8",
            page.as_bytes(),
        )
        .await?;
        return Ok(None);
    }

    if is_valid_callback(&request, callback_path, &expected_host, origin, nonce) {
        let payload: CallbackPayload = match serde_json::from_slice(&request.body) {
            Ok(payload) => payload,
            Err(_) => {
                write_response(&mut stream, 400, "text/plain", b"Invalid callback").await?;
                return Ok(None);
            }
        };
        if payload.authorization_failed && payload.music_user_token.is_none() {
            write_response(&mut stream, 200, "text/plain", b"Authorization ended").await?;
            return Err(AuthError::BrowserRejected);
        }
        let Some(user_token) = payload.music_user_token else {
            write_response(&mut stream, 400, "text/plain", b"Invalid callback").await?;
            return Ok(None);
        };
        if user_token.trim().is_empty() || user_token.len() > MAX_TOKEN_SIZE {
            write_response(&mut stream, 400, "text/plain", b"Invalid callback").await?;
            return Ok(None);
        }
        write_response(&mut stream, 200, "text/plain", b"Authorization complete").await?;
        return Ok(Some(SecretToken::new(user_token)));
    }

    write_response(&mut stream, 404, "text/plain", b"Not found").await?;
    Ok(None)
}

fn is_valid_callback(
    request: &HttpRequest,
    callback_path: &str,
    expected_host: &str,
    origin: &str,
    nonce: &str,
) -> bool {
    request.method == "POST"
        && request.path == callback_path
        && request.header("host") == Some(expected_host)
        && request.header("origin") == Some(origin)
        && request.header("x-apple-music-tui-nonce") == Some(nonce)
        && request
            .header("content-type")
            .is_some_and(|value| value.eq_ignore_ascii_case("application/json"))
        && request.body.len() <= MAX_TOKEN_SIZE
}

struct HttpRequest {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl HttpRequest {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(candidate, _)| candidate == name)
            .map(|(_, value)| value.as_str())
    }
}

async fn read_request(stream: &mut TcpStream) -> Result<HttpRequest, AuthError> {
    let mut buffer = Vec::with_capacity(4096);
    let header_end = loop {
        if buffer.len() >= MAX_REQUEST_SIZE {
            return Err(AuthError::BrowserAuthorization(
                "local callback request was too large".to_owned(),
            ));
        }
        let read = stream
            .read_buf(&mut buffer)
            .await
            .map_err(AuthError::BrowserServer)?;
        if read == 0 {
            return Err(AuthError::BrowserAuthorization(
                "local callback closed before completing".to_owned(),
            ));
        }
        if let Some(position) = buffer.windows(4).position(|window| window == b"\r\n\r\n") {
            break position + 4;
        }
    };

    let head = std::str::from_utf8(&buffer[..header_end]).map_err(|_| {
        AuthError::BrowserAuthorization("local callback request was malformed".to_owned())
    })?;
    let mut lines = head.split("\r\n");
    let mut request_line = lines
        .next()
        .ok_or_else(|| AuthError::BrowserAuthorization("missing request line".to_owned()))?
        .split_whitespace();
    let method = request_line.next().unwrap_or_default().to_owned();
    let path = request_line.next().unwrap_or_default().to_owned();
    if request_line.next() != Some("HTTP/1.1") || request_line.next().is_some() {
        return Err(AuthError::BrowserAuthorization(
            "local callback request line was malformed".to_owned(),
        ));
    }
    let headers = lines
        .filter(|line| !line.is_empty())
        .map(|line| {
            let (name, value) = line.split_once(':').ok_or_else(|| {
                AuthError::BrowserAuthorization("local callback header was malformed".to_owned())
            })?;
            Ok((name.trim().to_ascii_lowercase(), value.trim().to_owned()))
        })
        .collect::<Result<Vec<_>, AuthError>>()?;
    let content_length = headers
        .iter()
        .find(|(name, _)| name == "content-length")
        .map(|(_, value)| value.parse::<usize>())
        .transpose()
        .map_err(|_| AuthError::BrowserAuthorization("invalid callback length".to_owned()))?
        .unwrap_or_default();
    if content_length > MAX_TOKEN_SIZE || header_end + content_length > MAX_REQUEST_SIZE {
        return Err(AuthError::BrowserAuthorization(
            "local callback request was too large".to_owned(),
        ));
    }
    while buffer.len() < header_end + content_length {
        let read = stream
            .read_buf(&mut buffer)
            .await
            .map_err(AuthError::BrowserServer)?;
        if read == 0 {
            return Err(AuthError::BrowserAuthorization(
                "local callback body was incomplete".to_owned(),
            ));
        }
    }
    Ok(HttpRequest {
        method,
        path,
        headers,
        body: buffer[header_end..header_end + content_length].to_vec(),
    })
}

async fn write_response(
    stream: &mut TcpStream,
    status: u16,
    content_type: &str,
    body: &[u8],
) -> Result<(), AuthError> {
    let reason = if status == 200 {
        "OK"
    } else if status == 400 {
        "Bad Request"
    } else {
        "Not Found"
    };
    let headers = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\nReferrer-Policy: no-referrer\r\nX-Content-Type-Options: nosniff\r\n\r\n",
        body.len()
    );
    stream
        .write_all(headers.as_bytes())
        .await
        .map_err(AuthError::BrowserServer)?;
    stream
        .write_all(body)
        .await
        .map_err(AuthError::BrowserServer)
}

#[cfg(test)]
mod tests {
    use super::{HttpRequest, authorization_page, is_valid_callback, random_nonce};
    use crate::auth::SecretToken;

    #[test]
    fn nonce_has_256_bits_encoded_as_hex() {
        let first = random_nonce();
        let second = random_nonce();
        assert_eq!(first.len(), 64);
        assert!(first.bytes().all(|byte| byte.is_ascii_hexdigit()));
        assert_ne!(first, second);
    }

    #[test]
    fn helper_page_does_not_use_browser_persistence() {
        let page = authorization_page(
            &SecretToken::new("test-token".to_owned()),
            "/callback/nonce",
            "nonce",
        )
        .expect("page");
        assert!(!page.contains("localStorage"));
        assert!(page.contains("MusicKit.getInstance"));
        assert!(page.contains("X-Apple-Music-TUI-Nonce"));
    }

    #[test]
    fn callback_requires_exact_origin_host_path_and_nonce() {
        let mut request = HttpRequest {
            method: "POST".to_owned(),
            path: "/callback/nonce".to_owned(),
            headers: vec![
                ("host".to_owned(), "127.0.0.1:4321".to_owned()),
                ("origin".to_owned(), "http://127.0.0.1:4321".to_owned()),
                ("x-apple-music-tui-nonce".to_owned(), "nonce".to_owned()),
                ("content-type".to_owned(), "application/json".to_owned()),
            ],
            body: br#"{"musicUserToken":"test"}"#.to_vec(),
        };
        assert!(is_valid_callback(
            &request,
            "/callback/nonce",
            "127.0.0.1:4321",
            "http://127.0.0.1:4321",
            "nonce"
        ));

        request.headers[1].1 = "http://attacker.invalid".to_owned();
        assert!(!is_valid_callback(
            &request,
            "/callback/nonce",
            "127.0.0.1:4321",
            "http://127.0.0.1:4321",
            "nonce"
        ));
    }
}
