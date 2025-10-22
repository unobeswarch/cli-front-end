// API client module
// -----------------
// This module provides a compact, blocking HTTP client used by the
// CLI to talk to the project's backend (auth-be / API gateway).
//
// Design goals / notes:
// - Keep the API surface small and easy to follow (blocking reqwest
//   client). This simplifies the CLI flow and avoids async boilerplate.
// - Provide helpers for persisting a JWT token into the project folder
//   so the CLI can 'remember' a session between runs. Meta JSON tracks
//   whether the token should persist and whether the previous exit was
//   clean (used to avoid auto-login after crashes/force closes).
// - Expose simple methods for register, login and upload that return
//   `anyhow::Result` with helpful context messages on failure.

use anyhow::{Context, Result};
use reqwest::blocking::{Client, multipart};
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION};
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::path::PathBuf;
use std::io::{Read, Write};
use serde_json::json;

/// Simple API client
///
/// This struct centralizes HTTP calls, stores the base URL used for
/// requests and an optional JWT token (set after login). It derives
/// `Clone` so the client can be cheaply cloned and used from background
/// threads (we keep a `reqwest::blocking::Client` inside which is cheap
/// to clone).
#[derive(Clone)]
pub struct ApiClient {
    // Underlying reqwest blocking client used for synchronous requests
    client: Client,
    // Base URL for API gateway (defaults to http://localhost:8081)
    base_url: String,
    // Optional JWT token used for authenticated endpoints
    token: Option<String>,
}

/// RegisterRequest
///
/// Shape sent to the backend's `/register` endpoint. Field names follow
/// the backend's expected payload so they can be serialized directly
/// using serde. `Clone` is derived to allow moving the request into a
/// background thread in the CLI while the main thread keeps the UI
/// responsive.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct RegisterRequest {
    pub nombre_completo: String,
    pub edad: i32,
    pub rol: String,
    pub identificacion: String,
    pub correo: String,
    pub contrasena: String,
    pub acepta_tratamiento_datos: bool,
}

/// AuthRequest
///
/// Payload sent to the `/auth` endpoint. Also `Clone` so the CLI can
/// send it from a background thread while the spinner continues in the
/// main thread.
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct AuthRequest {
    pub correo: String,
    pub contrasena: String,
}

/// AuthResponse
///
/// The CLI expects the auth endpoint to reply with a JSON object
/// containing at least a `token` (JWT) and a friendly `nombre` used
/// for UI greetings. Other fields mirror the backend response and are
/// kept generic where appropriate (e.g., `user_id` as Value).
#[derive(Serialize, Deserialize, Debug)]
pub struct AuthResponse {
    pub nombre: String,
    pub token: String,
    pub rol: String,
    pub user_id: serde_json::Value,
    pub correo: String,
}

impl ApiClient {
    /// Create an ApiClient configured from the environment variable
    /// `API_GATEWAY_URL` or fallback to `http://localhost:8080`.
    pub fn from_env() -> Result<Self> {
        let base_url = std::env::var("API_GATEWAY_URL").unwrap_or_else(|_| "http://localhost:8080".into());
        let client = Client::builder()
            .build()
            .context("Failed to build HTTP client")?;
        Ok(ApiClient {
            client,
            base_url,
            token: None,
        })
    }

    // Notes:
    // - The client is built once and reused. `reqwest::blocking::Client`
    //   holds connection pools and other internal caches which are
    //   beneficial even for a CLI.
    // - `API_GATEWAY_URL` environment variable allows pointing the CLI
    //   to a different backend (e.g., a locally running auth-be vs a
    //   gateway proxy).

    /// Store a JWT token for subsequent authenticated requests.
    pub fn set_token(&mut self, token: &str) {
        self.token = Some(token.to_string());
    }

    /// Clear any stored token (logout).
    pub fn clear_token(&mut self) {
        self.token = None;
    }

    /// Returns whether a token is present in the client.
    pub fn has_token(&self) -> bool {
        self.token.is_some()
    }

    /// Build authorization headers when a token is present.
    fn auth_headers(&self) -> HeaderMap {
        let mut headers = HeaderMap::new();
        if let Some(t) = &self.token {
            // Build a standard `Authorization: Bearer <token>` header.
            // We `unwrap()` here because the formatted string is always
            // valid for a header value; if this ever changes a proper
            // error path should be added.
            let val = format!("Bearer {}", t);
            headers.insert(AUTHORIZATION, HeaderValue::from_str(&val).unwrap());
        }
        headers
    }

    /// Persist token and metadata into the project folder (cli-front-end).
    /// This writes two files next to Cargo.toml: `.neumodiag_token` and
    /// `.neumodiag_token.meta` which contains JSON like {"persist":true,"clean_exit":false}
    pub fn persist_token_to_project(&self, token: &str, persist: bool) -> Result<()> {
        let proj_dir = find_project_dir()?;

        let token_path = proj_dir.join(".neumodiag_token");
        let meta_path = proj_dir.join(".neumodiag_token.meta");

        // Write token
        let mut f = File::create(&token_path).context("creating token file")?;
        f.write_all(token.as_bytes()).context("writing token file")?;

        // Write meta
        // meta stores whether the user asked to persist the token and
        // whether the program exited cleanly in the previous run. The
        // CLI sets `clean_exit` to `true` only when the user exits via
        // the menu — this avoids auto-login after crashes.
        let meta = json!({"persist": persist, "clean_exit": false});
        let mut m = File::create(&meta_path).context("creating token meta file")?;
        m.write_all(meta.to_string().as_bytes()).context("writing token meta file")?;
        Ok(())
    }

    /// Load token only if present in project folder. Returns Ok(None) when
    /// no token is available. Note: does not automatically set ApiClient.token
    /// so the caller can decide whether to honor auto-login rules.
    pub fn load_token_from_project(&self) -> Result<Option<String>> {
        let proj_dir = find_project_dir()?;
        let token_path = proj_dir.join(".neumodiag_token");
        if !token_path.exists() {
            return Ok(None);
        }
        let mut s = String::new();
        let mut f = File::open(&token_path).context("opening token file")?;
        // Read the raw token. Note: some editors or tools may add a
        // trailing newline when saving files. The caller typically
        // trims whitespace before use (see ui.rs) to be robust.
        f.read_to_string(&mut s).context("reading token file")?;
        Ok(Some(s))
    }

    /// Read meta JSON if present. Returns None when no meta file exists.
    pub fn load_token_meta(&self) -> Result<Option<serde_json::Value>> {
        let proj_dir = find_project_dir()?;
        let meta_path = proj_dir.join(".neumodiag_token.meta");
        if !meta_path.exists() {
            return Ok(None);
        }
        let s = std::fs::read_to_string(&meta_path).context("reading meta file")?;
        let v: serde_json::Value = serde_json::from_str(&s).context("parsing meta json")?;
        Ok(Some(v))
    }

    /// Update meta.clean_exit flag to the provided value. Creates meta if missing.
    pub fn set_clean_exit_meta(&self, clean: bool) -> Result<()> {
        let proj_dir = find_project_dir()?;
        let meta_path = proj_dir.join(".neumodiag_token.meta");
        let mut meta = if meta_path.exists() {
            let s = std::fs::read_to_string(&meta_path).unwrap_or_else(|_| "{}".into());
            // Merge with existing meta when possible. If the meta file is
            // malformed we fall back to an empty object to avoid panics.
            serde_json::from_str(&s).unwrap_or_else(|_| json!({}))
        } else {
            json!({})
        };
        meta["clean_exit"] = json!(clean);
        let mut m = File::create(&meta_path).context("creating meta file")?;
        m.write_all(meta.to_string().as_bytes()).context("writing meta file")?;
        Ok(())
    }

    /// Clear persisted token and meta files in the project folder.
    pub fn clear_persisted_token_in_project(&self) {
        let proj_dir = find_project_dir().unwrap_or_else(|_| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
        let token_path = proj_dir.join(".neumodiag_token");
        let meta_path = proj_dir.join(".neumodiag_token.meta");
        let _ = std::fs::remove_file(token_path);
        let _ = std::fs::remove_file(meta_path);
    }

    /// Register a user by POSTing to /register. Returns a simple String
    /// on success, or an error with the server response body on failure.
    pub fn register(&self, req: &RegisterRequest) -> Result<String> {
        let url = format!("{}/register", &self.base_url);
        let res = self.client.post(&url)
            .json(req)
            .send()
            .context("Failed to send register request")?;
        if !res.status().is_success() {
            let status = res.status();
            let txt = res.text().unwrap_or_else(|_| "".into());
            anyhow::bail!("Register failed: {} - {}", status, txt);
        }
        Ok("Registered".into())
    }

    /// Perform login and parse the expected AuthResponse JSON.
    pub fn login(&self, req: &AuthRequest) -> Result<AuthResponse> {
        let url = format!("{}/auth", &self.base_url);
        let res = self.client.post(&url)
            .json(req)
            .send()
            .context("Failed to send auth request")?;
        if !res.status().is_success() {
            let status = res.status();
            let txt = res.text().unwrap_or_else(|_| "".into());
            anyhow::bail!("Login failed: {} - {}", status, txt);
        }
        let resp: AuthResponse = res.json().context("Parsing auth response json")?;
        Ok(resp)
    }

    /// Upload a profile picture using multipart/form-data. The backend
    /// path `/upload` is used here and the multipart field is `foto`.
    /// The function adds the Authorization header if a token is present.
    pub fn upload_profile_picture(&self, file_path: &PathBuf) -> Result<String> {
        // auth-be exposes the upload handler at /upload and expects the
        // multipart field to be named "foto".
        let url = format!("{}/upload", &self.base_url);

        // Open file and create a multipart part. We set a default filename
        // and `image/jpeg` as the mime type for the prototype; a real app
        // would detect the mime type from the file extension.
        let file = File::open(file_path).context("Failed to open image file")?;
        let file_name = file_path.file_name().and_then(|s| s.to_str()).unwrap_or("image.jpg");

        let part = multipart::Part::reader(file).file_name(file_name.to_string()).mime_str("image/jpeg").unwrap();
        // Use field name "foto" to match auth-be's HandlerGuardarFotoPerfil
        let form = multipart::Form::new().part("foto", part);

        let mut req = self.client.post(&url).multipart(form);
        // Add auth header if present
        if let Some(_) = &self.token {
            req = req.headers(self.auth_headers());
        }

        let res = req.send().context("Failed to send upload request")?;
        if !res.status().is_success() {
            let status = res.status();
            let txt = res.text().unwrap_or_else(|_| "".into());
            anyhow::bail!("Upload failed: {} - {}", status, txt);
        }
        Ok("Upload OK".into())
    }
}

/// Try to locate the project directory by checking CARGO_MANIFEST_DIR, then
/// walking up from the current executable location looking for Cargo.toml.
fn find_project_dir() -> Result<PathBuf> {
    if let Ok(s) = std::env::var("CARGO_MANIFEST_DIR") {
        return Ok(PathBuf::from(s));
    }

    if let Ok(exe) = std::env::current_exe() {
        if let Some(mut dir) = exe.parent() {
            loop {
                // Walk upwards from the executable location looking for a
                // `Cargo.toml` file. This heuristic finds the project root
                // in common development layouts (cargo run, target/debug
                // exe, etc.). If nothing is found, we fall back to the
                // current working directory below.
                let candidate = dir.join("Cargo.toml");
                if candidate.exists() {
                    return Ok(dir.to_path_buf());
                }
                // Move up one directory and repeat. Stop when we reach
                // the filesystem root (no parent).
                if let Some(p) = dir.parent() {
                    dir = p;
                } else {
                    break;
                }
            }
        }
    }

    Ok(std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
}
