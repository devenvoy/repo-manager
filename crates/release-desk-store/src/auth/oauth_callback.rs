use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

const CALLBACK_TIMEOUT: Duration = Duration::from_secs(180);

#[derive(Clone, Debug, Eq, PartialEq)]
enum CallbackResult {
    Code(String),
    Error(String),
}

pub(super) struct LoopbackCallback {
    port: u16,
    outcome: Arc<Mutex<Option<CallbackResult>>>,
    running: Arc<AtomicBool>,
    cancelled: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl LoopbackCallback {
    pub(super) fn start(
        host: &str,
        callback_path: String,
        port: Option<u16>,
        expected_state: String,
        cancelled: Arc<AtomicBool>,
    ) -> Result<Self, String> {
        let listener = TcpListener::bind((host, port.unwrap_or(0)))
            .map_err(|error| format!("Could not open the OAuth callback port: {error}"))?;
        listener
            .set_nonblocking(true)
            .map_err(|error| format!("Could not configure OAuth callback listener: {error}"))?;
        let port = listener
            .local_addr()
            .map_err(|error| format!("Could not read OAuth callback port: {error}"))?
            .port();
        let outcome = Arc::new(Mutex::new(None));
        let running = Arc::new(AtomicBool::new(true));
        let thread_outcome = Arc::clone(&outcome);
        let thread_running = Arc::clone(&running);
        let thread_cancelled = Arc::clone(&cancelled);
        let handle = thread::spawn(move || {
            let started = Instant::now();
            while thread_running.load(Ordering::Relaxed)
                && !thread_cancelled.load(Ordering::Relaxed)
                && started.elapsed() < CALLBACK_TIMEOUT
            {
                match listener.accept() {
                    Ok((mut stream, _)) => {
                        let result = handle_callback(&mut stream, &expected_state, &callback_path);
                        if let Ok(mut slot) = thread_outcome.lock() {
                            *slot = Some(result);
                        }
                        thread_running.store(false, Ordering::Relaxed);
                        break;
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                        thread::sleep(Duration::from_millis(50));
                    }
                    Err(error) => {
                        if let Ok(mut slot) = thread_outcome.lock() {
                            *slot = Some(CallbackResult::Error(format!(
                                "OAuth callback failed: {error}"
                            )));
                        }
                        thread_running.store(false, Ordering::Relaxed);
                    }
                }
            }
        });
        Ok(Self {
            port,
            outcome,
            running,
            cancelled,
            handle: Some(handle),
        })
    }

    pub(super) fn port(&self) -> u16 {
        self.port
    }

    pub(super) fn wait_for_code(mut self) -> Result<String, String> {
        let deadline = Instant::now() + CALLBACK_TIMEOUT;
        while Instant::now() < deadline {
            if self.cancelled.load(Ordering::Relaxed) {
                self.stop();
                return Err("OAuth authorization was cancelled.".into());
            }
            if let Some(result) = self.outcome.lock().ok().and_then(|outcome| outcome.clone()) {
                self.stop();
                return match result {
                    CallbackResult::Code(code) => Ok(code),
                    CallbackResult::Error(error) => Err(error),
                };
            }
            if !self.running.load(Ordering::Relaxed) {
                break;
            }
            thread::sleep(Duration::from_millis(50));
        }
        self.stop();
        Err("OAuth authorization timed out or was cancelled.".into())
    }

    fn stop(&mut self) {
        self.running.store(false, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for LoopbackCallback {
    fn drop(&mut self) {
        self.stop();
    }
}

fn handle_callback(
    stream: &mut std::net::TcpStream,
    expected_state: &str,
    callback_path: &str,
) -> CallbackResult {
    let mut buffer = [0_u8; 4096];
    let bytes_read = stream.read(&mut buffer).unwrap_or(0);
    let request = String::from_utf8_lossy(&buffer[..bytes_read]);
    let Some((method, target)) = request.lines().next().and_then(|line| {
        let mut parts = line.split_whitespace();
        Some((parts.next()?, parts.next()?))
    }) else {
        respond(stream, false);
        return CallbackResult::Error("OAuth callback request was malformed.".into());
    };
    if method != "GET" {
        respond(stream, false);
        return CallbackResult::Error("OAuth callback method did not match.".into());
    }
    let Ok(url) = reqwest::Url::parse(&format!("http://localhost{target}")) else {
        respond(stream, false);
        return CallbackResult::Error("OAuth callback URL was malformed.".into());
    };
    if url.path() != callback_path {
        respond(stream, false);
        return CallbackResult::Error("OAuth callback path did not match.".into());
    }
    let mut code = None;
    let mut state = None;
    let mut error = None;
    for (key, value) in url.query_pairs() {
        match key.as_ref() {
            "code" => code = Some(value.into_owned()),
            "state" => state = Some(value.into_owned()),
            "error" => error = Some(value.into_owned()),
            _ => {}
        }
    }
    let result = match (code, state) {
        (Some(code), Some(state)) if !code.is_empty() && state == expected_state => {
            CallbackResult::Code(code)
        }
        (Some(_), Some(_)) => CallbackResult::Error("OAuth callback state did not match.".into()),
        (_, _) if error.is_some() => {
            CallbackResult::Error("Provider denied OAuth authorization.".into())
        }
        _ => CallbackResult::Error("OAuth callback did not include a code and state.".into()),
    };
    respond(stream, matches!(result, CallbackResult::Code(_)));
    result
}

fn respond(stream: &mut std::net::TcpStream, success: bool) {
    let (title, message) = if success {
        (
            "Sign-in complete",
            "Return to Release Desk to finish connecting your account.",
        )
    } else {
        ("Sign-in failed", "Return to Release Desk and try again.")
    };
    let body = format!("<!doctype html><title>{title}</title><h1>{title}</h1><p>{message}</p>");
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.flush();
}
