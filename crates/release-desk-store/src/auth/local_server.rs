//! Local HTTP redirect server for capturing OAuth browser callbacks.

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

/// Result received on the local OAuth redirect listener.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OAuthRedirectOutcome {
    Success { code: String },
    Error { message: String },
}

/// A lightweight background local HTTP redirect server for OAuth callbacks.
pub struct LocalOAuthServer {
    port: u16,
    running: Arc<AtomicBool>,
    outcome: Arc<Mutex<Option<OAuthRedirectOutcome>>>,
    handle: Option<JoinHandle<()>>,
}

impl LocalOAuthServer {
    /// Bind and start listening on `127.0.0.1:<port>` non-blockingly.
    pub fn start(port: u16) -> Result<Self, std::io::Error> {
        let listener = TcpListener::bind(("127.0.0.1", port))?;
        listener.set_nonblocking(true)?;

        let running = Arc::new(AtomicBool::new(true));
        let outcome = Arc::new(Mutex::new(None));

        let r_flag = running.clone();
        let out_ref = outcome.clone();

        let handle = thread::spawn(move || {
            let mut elapsed = Duration::ZERO;
            let tick = Duration::from_millis(80);
            let timeout = Duration::from_secs(180); // 3 minute timeout

            while r_flag.load(Ordering::Relaxed) && elapsed < timeout {
                match listener.accept() {
                    Ok((mut stream, _addr)) => {
                        handle_connection(&mut stream, &out_ref);
                        r_flag.store(false, Ordering::Relaxed);
                        break;
                    }
                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(tick);
                        elapsed += tick;
                    }
                    Err(_) => {
                        break;
                    }
                }
            }
        });

        Ok(Self {
            port,
            running,
            outcome,
            handle: Some(handle),
        })
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn is_listening(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }

    pub fn poll_outcome(&self) -> Option<OAuthRedirectOutcome> {
        self.outcome.lock().ok().and_then(|guard| guard.clone())
    }

    pub fn stop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

impl Drop for LocalOAuthServer {
    fn drop(&mut self) {
        self.stop();
    }
}

fn handle_connection(
    stream: &mut std::net::TcpStream,
    out_ref: &Arc<Mutex<Option<OAuthRedirectOutcome>>>,
) {
    let mut buffer = [0u8; 4096];
    let bytes_read = stream.read(&mut buffer).unwrap_or(0);
    let request = String::from_utf8_lossy(&buffer[..bytes_read]);

    let mut outcome = None;

    if let Some(first_line) = request.lines().next() {
        if let Some(path_and_query) = first_line.split_whitespace().nth(1) {
            if let Some(query) = path_and_query.split('?').nth(1) {
                let mut code = None;
                let mut error = None;

                for param in query.split('&') {
                    if let Some(val) = param.strip_prefix("code=") {
                        code = Some(val.to_string());
                    } else if let Some(val) = param.strip_prefix("token=") {
                        code = Some(val.to_string());
                    } else if let Some(val) = param.strip_prefix("access_token=") {
                        code = Some(val.to_string());
                    } else if let Some(val) = param.strip_prefix("error=") {
                        error = Some(val.to_string());
                    }
                }

                if let Some(c) = code {
                    outcome = Some(OAuthRedirectOutcome::Success { code: c });
                } else if let Some(e) = error {
                    outcome = Some(OAuthRedirectOutcome::Error { message: e });
                }
            }
        }
    }

    let is_success = matches!(outcome, Some(OAuthRedirectOutcome::Success { .. }));
    if let Ok(mut guard) = out_ref.lock() {
        *guard = outcome;
    }

    let html_body = if is_success {
        r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <title>Release Desk • Authentication Successful</title>
  <style>
    body { font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, Helvetica, Arial, sans-serif; background-color: #0b0f19; color: #f1f5f9; display: flex; align-items: center; justify-content: center; height: 100vh; margin: 0; }
    .card { background: #1e293b; border: 1px solid #334155; border-radius: 14px; padding: 48px; text-align: center; max-width: 440px; box-shadow: 0 25px 50px -12px rgba(0, 0, 0, 0.7); }
    .icon { font-size: 52px; margin-bottom: 20px; }
    h1 { font-size: 22px; font-weight: 600; margin: 0 0 12px; color: #38bdf8; }
    p { font-size: 15px; color: #94a3b8; line-height: 1.5; margin: 0 0 24px; }
    .badge { display: inline-block; padding: 8px 16px; background: rgba(56, 189, 248, 0.15); color: #38bdf8; border: 1px solid rgba(56, 189, 248, 0.3); border-radius: 9999px; font-size: 13px; font-weight: 600; }
  </style>
</head>
<body>
  <div class="card">
    <div class="icon">✨</div>
    <h1>Authentication Successful!</h1>
    <p>Release Desk has captured your authorization token. You can close this browser tab and return to the application.</p>
    <span class="badge">Return to Release Desk</span>
  </div>
</body>
</html>"#
    } else {
        r#"<!DOCTYPE html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <title>Release Desk • Authentication Failed</title>
  <style>
    body { font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, Helvetica, Arial, sans-serif; background-color: #0b0f19; color: #f1f5f9; display: flex; align-items: center; justify-content: center; height: 100vh; margin: 0; }
    .card { background: #1e293b; border: 1px solid #ef4444; border-radius: 14px; padding: 48px; text-align: center; max-width: 440px; }
    .icon { font-size: 52px; margin-bottom: 20px; }
    h1 { font-size: 22px; font-weight: 600; margin: 0 0 12px; color: #ef4444; }
    p { font-size: 15px; color: #94a3b8; line-height: 1.5; margin: 0 0 24px; }
  </style>
</head>
<body>
  <div class="card">
    <div class="icon">⚠️</div>
    <h1>Authentication Failed</h1>
    <p>No valid authorization code was returned. Please return to Release Desk and try again.</p>
  </div>
</body>
</html>"#
    };

    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        html_body.len(),
        html_body
    );

    let _ = stream.write_all(response.as_bytes());
    let _ = stream.flush();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_local_oauth_server_lifecycle() {
        let mut server = LocalOAuthServer::start(14567).expect("must bind port");
        assert!(server.is_listening());
        assert_eq!(server.poll_outcome(), None);
        server.stop();
        assert!(!server.is_listening());
    }
}
