// API client module: contains a small blocking HTTP client that talks to
// the project's API gateway. It is intentionally small and synchronous
// to keep the learning curve low for beginners.

use anyhow::{Context, Result};
use reqwest::blocking::{Client, multipart};
use reqwest::header::{HeaderMap, HeaderValue, AUTHORIZATION};
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::path::PathBuf;

/// Simple API client that holds a reqwest blocking client, the base URL
/// of the API gateway and an optional JWT token for authenticated calls.
#[derive(Clone)]
pub struct ApiClient {
    client: Client,
    base_url: String,
    token: Option<String>,
}

/// Data shape used to register a user. Fields mirror the backend
/// expectations (see BusinessLogic service).
#[derive(Serialize, Deserialize, Debug)]
pub struct RegisterRequest {
    pub nombre_completo: String,
    pub edad: i32,
    pub rol: String,
    pub identificacion: String,
    pub correo: String,
    pub contrasena: String,
    pub acepta_tratamiento_datos: bool,
}

/// Login request payload.
#[derive(Serialize, Deserialize, Debug)]
pub struct AuthRequest {
    pub correo: String,
    pub contrasena: String,
}

/// Expected response from the login endpoint. We keep `user_id` as a
/// serde_json::Value because the backend returns an int but keeping it
/// flexible avoids parsing issues in the prototype.
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
    /// `API_GATEWAY_URL` or fallback to `http://localhost:8000`.
    pub fn from_env() -> Result<Self> {
    let base_url = std::env::var("API_GATEWAY_URL").unwrap_or_else(|_| "http://localhost:3001".into());
        let client = Client::builder()
            .build()
            .context("Failed to build HTTP client")?;
        Ok(ApiClient {
            client,
            base_url,
            token: None,
        })
    }

    /// Store a JWT token for subsequent authenticated requests.
    pub fn set_token(&mut self, token: &str) {
        self.token = Some(token.to_string());
    }

    /// Returns whether a token is present in the client.
    pub fn has_token(&self) -> bool {
        self.token.is_some()
    }

    /// Helper to build the Authorization header map when a token is set.
    fn auth_headers(&self) -> HeaderMap {
        let mut headers = HeaderMap::new();
        if let Some(t) = &self.token {
            let val = format!("Bearer {}", t);
            headers.insert(AUTHORIZATION, HeaderValue::from_str(&val).unwrap());
        }
        headers
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
    /// path `/upload/profile` is used here as a prototype convenience.
    /// The function adds the Authorization header if a token is present.
    pub fn upload_profile_picture(&self, file_path: &PathBuf) -> Result<String> {
        let url = format!("{}/upload/profile", &self.base_url);

        // Open file and create a multipart part. We set a default filename
        // and `image/jpeg` as the mime type for the prototype; a real app
        // would detect the mime type from the file extension.
        let file = File::open(file_path).context("Failed to open image file")?;
        let file_name = file_path.file_name().and_then(|s| s.to_str()).unwrap_or("image.jpg");

        let part = multipart::Part::reader(file).file_name(file_name.to_string()).mime_str("image/jpeg").unwrap();
        let form = multipart::Form::new().part("file", part);

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
