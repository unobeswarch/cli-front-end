// UI layer: provides a simple interactive menu using `dialoguer`.
// The functions are small and synchronous to make the flow easy to follow.

use crate::api::{ApiClient, RegisterRequest, AuthRequest};
use anyhow::Result;
use dialoguer::{Input, Select, Confirm, Password};
use indicatif::{ProgressBar, ProgressStyle};
use std::path::PathBuf;
use std::time::Duration;
use std::thread;

/// Main interactive menu. Receives an `ApiClient` instance and runs a
/// simple select loop until the user chooses "Exit".
///
/// Note: `Select::interact()` is keyboard-driven: you can use arrow keys
/// and Enter to choose an option.
pub fn main_menu(mut api: ApiClient) -> Result<()> {
    loop {
        let items = vec!["Register", "Login", "Upload profile picture", "Exit"];
        // `Select` shows a keyboard-navigable list in the terminal.
        let selection = Select::new().items(&items).default(0).interact()?;
        match selection {
            0 => {
                // Registration flow collects fields and calls API client.
                handle_register(&api)?;
            }
            1 => {
                // Login flow returns an optional token. If token is present,
                // we set it in the client and persist to disk for future runs.
                if let Some(token) = handle_login(&api)? {
                    api.set_token(&token);
                    // persist token to disk
                    persist_token(&token)?;
                }
            }
            2 => {
                // Upload requires a token. We check a persisted token on disk
                // first (so user can reuse a previous session) and ensure the
                // client has it set.
                let token_opt = load_token().ok();
                if token_opt.is_none() && !api.has_token() {
                    println!("You should login first to attach profile picture.");
                    continue;
                }
                let path: String = Input::new().with_prompt("Image file path").interact_text()?;
                let pb = PathBuf::from(path);
                // indicatif's `ProgressSpinner` is used to show a simple
                // spinner while the upload is happening.
                let spinner = ProgressBar::new_spinner();
                spinner.set_style(ProgressStyle::with_template("{spinner} {msg}").unwrap());
                spinner.set_message("Uploading...");
                // simulate a small delay to make the spinner visible
                thread::sleep(Duration::from_millis(400));
                match api.upload_profile_picture(&pb) {
                    Ok(_) => println!("Upload successful"),
                    Err(e) => println!("Upload failed: {}", e),
                }
            }
            3 => break,
            _ => {}
        }
    }
    Ok(())
}

/// Collect input fields for registration and call `ApiClient::register`.
fn handle_register(api: &ApiClient) -> Result<()> {
    // `Input::interact_text()` prompts the user for input and returns it.
    let nombre: String = Input::new().with_prompt("Full name").interact_text()?;
    let edad: i32 = Input::new().with_prompt("Age").interact_text()?;
    let rol_choices = vec!["doctor", "paciente"];
    let rol = rol_choices[Select::new().items(&rol_choices).default(1).interact()?].to_string();
    let identificacion: String = Input::new().with_prompt("Identification").interact_text()?;
    let correo: String = Input::new().with_prompt("Email").interact_text()?;
    // `Password` hides input in terminal for passwords.
    let contrasena: String = Password::new().with_prompt("Password").interact()?;
    let acepta = Confirm::new().with_prompt("Accept data processing?").interact()?;

    let req = RegisterRequest {
        nombre_completo: nombre,
        edad,
        rol,
        identificacion,
        correo,
        contrasena,
        acepta_tratamiento_datos: acepta,
    };

    // show spinner for UX, then call the API
    let spinner = ProgressBar::new_spinner();
    spinner.set_style(ProgressStyle::with_template("{spinner} {msg}").unwrap());
    spinner.set_message("Registering...");
    // simulate a small wait for UX
    thread::sleep(Duration::from_millis(300));

    match api.register(&req) {
        Ok(_) => println!("Registered successfully, please login."),
        Err(e) => println!("Register failed: {}", e),
    }
    Ok(())
}

/// Collect credentials and perform login, returning the JWT token if OK.
fn handle_login(api: &ApiClient) -> Result<Option<String>> {
    let correo: String = Input::new().with_prompt("Email").interact_text()?;
    let contrasena: String = Password::new().with_prompt("Password").interact()?;
    let req = AuthRequest { correo, contrasena };

    let spinner = ProgressBar::new_spinner();
    spinner.set_style(ProgressStyle::with_template("{spinner} {msg}").unwrap());
    spinner.set_message("Logging in...");

    match api.login(&req) {
        Ok(resp) => {
            println!("Welcome {}!", resp.nombre);
            Ok(Some(resp.token))
        }
        Err(e) => {
            println!("Login failed: {}", e);
            Ok(None)
        }
    }
}

/// Persist token into a file in the user's home directory.
fn persist_token(token: &str) -> Result<()> {
    let dir = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    let path = dir.join(".neumodiag_token");
    std::fs::write(path, token)?;
    Ok(())
}

/// Load token from the user's home directory file.
fn load_token() -> Result<String> {
    let dir = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    let path = dir.join(".neumodiag_token");
    let data = std::fs::read_to_string(path)?;
    Ok(data)
}
