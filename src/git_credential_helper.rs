//! Narrow, non-persistent Git credential-helper protocol for selected project credentials.
//!
//! The executable must call `dispatch` before starting the GUI. The helper reads
//! only an OS-stored secret reference and returns it for one matching HTTPS remote.
use release_desk_core::port::SecretStorePort as _;
use release_desk_store::credentials::OsSecretStore;
use std::io::{self, BufRead, Write};

#[derive(Debug)]
struct Scope {
    key: String,
    host: String,
    path: String,
    username: String,
}

impl Scope {
    fn from_env() -> Option<Self> {
        let scope = Self {
            key: std::env::var("REPO_MANAGER_CREDENTIAL_REF").ok()?,
            host: std::env::var("REPO_MANAGER_CREDENTIAL_HOST").ok()?,
            path: std::env::var("REPO_MANAGER_CREDENTIAL_PATH").ok()?,
            username: std::env::var("REPO_MANAGER_CREDENTIAL_USERNAME").ok()?,
        };
        let supported = matches!(
            (scope.host.as_str(), scope.username.as_str()),
            ("github.com", "x-access-token") | ("gitlab.com", "oauth2")
        );
        if !supported
            || [&scope.key, &scope.path]
                .iter()
                .any(|value| value.is_empty() || value.chars().any(char::is_control))
        {
            return None;
        }
        Some(scope)
    }
}

/// If invoked as the Git credential helper, handle the request and return an exit code.
/// Otherwise return `None`, allowing normal application startup.
pub fn dispatch() -> Option<i32> {
    let mut args = std::env::args();
    args.next();
    if args.next().as_deref() != Some("--git-credential-helper") {
        return None;
    }
    let operation = args.next().unwrap_or_default();
    if args.next().is_some() {
        return Some(1);
    }
    if operation == "store" || operation == "erase" {
        return Some(0);
    }
    let Some(scope) = Scope::from_env() else {
        return Some(1);
    };
    let stdin = io::stdin();
    let stdout = io::stdout();
    let store = OsSecretStore::new();
    let result = respond(&operation, &scope, stdin.lock(), stdout.lock(), |key| {
        store
            .get_secret(key)
            .ok()
            .flatten()
            .or_else(|| std::env::var("GITHUB_TOKEN").ok())
            .or_else(|| std::env::var("GH_TOKEN").ok())
    });
    Some(if result.is_ok() { 0 } else { 1 })
}

fn respond<R: BufRead, W: Write>(
    operation: &str,
    scope: &Scope,
    mut input: R,
    mut output: W,
    lookup: impl FnOnce(&str) -> Option<String>,
) -> io::Result<()> {
    if operation != "get" {
        return Err(io::Error::other("unsupported credential operation"));
    }
    let mut protocol = None;
    let mut host = None;
    let mut path = None;
    let mut username = None;
    let mut total = 0usize;
    let mut line = String::new();
    loop {
        line.clear();
        let read = input.read_line(&mut line)?;
        total += read;
        if total > 16_384 {
            return Err(io::Error::other("credential request too large"));
        }
        if read == 0 || line == "\n" || line == "\r\n" {
            break;
        }
        let line = line.trim_end_matches(['\r', '\n']);
        let Some((key, value)) = line.split_once('=') else {
            return Err(io::Error::other("invalid credential request"));
        };
        let slot = match key {
            "protocol" => &mut protocol,
            "host" => &mut host,
            "path" => &mut path,
            "username" => &mut username,
            _ => continue,
        };
        if slot.replace(value.to_string()).is_some() {
            return Err(io::Error::other("duplicate credential attribute"));
        }
    }
    let path_matches = match path.as_deref() {
        Some(p) => {
            p == scope.path.as_str()
                || p.trim_end_matches(".git") == scope.path.trim_end_matches(".git")
        }
        None => false,
    };
    if protocol.as_deref() != Some("https")
        || host.as_deref() != Some(scope.host.as_str())
        || !path_matches
        || username
            .as_deref()
            .is_some_and(|value| value != scope.username)
    {
        return Err(io::Error::other("credential request outside project scope"));
    }
    let secret = lookup(&scope.key)
        .filter(|secret| !secret.is_empty() && !secret.chars().any(char::is_control))
        .ok_or_else(|| io::Error::other("project credential unavailable"))?;
    writeln!(output, "username={}\npassword={}\n", scope.username, secret)?;
    output.flush()
}

#[cfg(test)]
mod tests {
    use super::*;
    fn scope() -> Scope {
        Scope {
            key: "key-ref".into(),
            host: "github.com".into(),
            path: "owner/repo.git".into(),
            username: "x-access-token".into(),
        }
    }
    #[test]
    fn returns_secret_only_for_exact_context() {
        let mut output = Vec::new();
        respond(
            "get",
            &scope(),
            b"protocol=https\nhost=github.com\npath=owner/repo.git\n\n".as_slice(),
            &mut output,
            |key| (key == "key-ref").then(|| "test-secret".into()),
        )
        .unwrap();
        assert_eq!(output, b"username=x-access-token\npassword=test-secret\n\n");
        for request in [
            "protocol=http\nhost=github.com\npath=owner/repo.git\n\n",
            "protocol=https\nhost=github.com.evil\npath=owner/repo.git\n\n",
            "protocol=https\nhost=github.com\npath=other/repo.git\n\n",
            "protocol=https\nhost=github.com\npath=owner/repo.git\nusername=other\n\n",
        ] {
            let mut result = Vec::new();
            assert!(
                respond("get", &scope(), request.as_bytes(), &mut result, |_| Some(
                    "test-secret".into()
                ))
                .is_err()
            );
            assert!(result.is_empty());
        }
    }
    #[test]
    fn missing_or_unsafe_secret_is_not_returned() {
        for secret in [None, Some("".into()), Some("secret\nother=value".into())] {
            let mut result = Vec::new();
            assert!(
                respond(
                    "get",
                    &scope(),
                    b"protocol=https\nhost=github.com\npath=owner/repo.git\n\n".as_slice(),
                    &mut result,
                    |_| secret
                )
                .is_err()
            );
            assert!(result.is_empty());
        }
    }
}
