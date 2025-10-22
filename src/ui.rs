// UI layer
// -------
// This module implements the interactive command-line interface for
// NeumoDiagnostics. It uses `dialoguer` for prompts and `indicatif`
// for simple progress spinners. The UI is organized around a single
// blocking menu loop (`main_menu`) which delegates network work to the
// `ApiClient` in `api.rs`.
//
// Important implementation notes:
// - Network calls are performed using the blocking `reqwest::blocking`
//   client inside `ApiClient`. To keep spinners animated on Windows
//   (cmd.exe) and avoid blocking the main thread, the CLI spawns a
//   short-lived background thread for each blocking call and polls the
//   result via an `mpsc` channel while ticking the spinner on the main
//   thread.
// - Token persistence helpers in `ApiClient` read/write two files
//   next to the project's `Cargo.toml`: `.neumodiag_token` (raw JWT)
//   and `.neumodiag_token.meta` (JSON with fields like `persist` and
//   `clean_exit`). The CLI reads the meta on startup to decide whether
//   to auto-restore a session.
// - All UI strings are in Spanish for this prototype and the menus are
//   intentionally minimal and keyboard-driven (arrow keys + Enter).

use crate::api::{ApiClient, RegisterRequest, AuthRequest};
use anyhow::Result;
use dialoguer::{Input, Select, Password};
use indicatif::{ProgressBar, ProgressStyle, ProgressDrawTarget};
use std::path::PathBuf;
use std::time::{Duration, Instant};
use std::thread;
use base64::engine::general_purpose::STANDARD as base64_standard;
use base64::Engine as _;

// Optional file dialog support
use rfd::FileDialog;

// small helper to clear previous terminal lines; used to hide the
// initial "Continuar/Cancelar" prompt when the user chooses to continue.
fn clear_previous_lines(mut n: u16) {
    use std::io::stdout;
    use crossterm::{execute, cursor::MoveUp, terminal::{Clear, ClearType}, cursor::MoveToColumn};
    let mut out = stdout();
    // safety: loop a bounded number of times; ignore errors — clearing is best-effort
    while n > 0 {
        let _ = execute!(out, MoveUp(1), MoveToColumn(0), Clear(ClearType::CurrentLine));
        n -= 1;
    }
}

// Shared header width used by the banner and separators so they match.
const HEADER_WIDTH: usize = 80;
// Minimum spinner display time in milliseconds so short operations still
// show a visible spinner for the user.
const MIN_SPINNER_MS: u64 = 1500;

fn print_header() {
    let width = HEADER_WIDTH;
    let line = "=".repeat(width);
    let title = "NeumoDiagnostics - Interfaz de línea de comandos";
    // center title
    let padding = if width > title.len() { (width - title.len()) / 2 } else { 0 };
    let centered = format!("{:padding$}{}{:padding$}", "", title, "", padding = padding);
    println!("{}", line);
    println!("{}", centered);
    println!("{}", line);
}

fn print_separator() {
    // Use the same width as the header so separators align visually.
    let sep = "=".repeat(HEADER_WIDTH);
    println!("{}", sep);
}

/// Print a titled section with a centered title and a separator line below it.
fn print_section(title: &str) {
    // center the title according to header width
    let width = HEADER_WIDTH;
    let padding = if width > title.len() { (width - title.len()) / 2 } else { 0 };
    let centered = format!("{:padding$}{}{:padding$}", "", title, "", padding = padding);
    println!("{}", centered);
    print_separator();
}

/// Main interactive menu. Receives an `ApiClient` instance and runs a
/// simple select loop until the user chooses "Exit".
///
/// Note: `Select::interact()` is keyboard-driven: you can use arrow keys
/// and Enter to choose an option.
pub fn main_menu(mut api: ApiClient) -> Result<()> {
    // Attempt auto-login only when a persisted token exists and the
    // token meta indicates the previous session exited cleanly.
    if let Ok(Some(meta)) = api.load_token_meta() {
        // meta example: {"persist": true, "clean_exit": true}
        if meta.get("clean_exit").and_then(|v| v.as_bool()).unwrap_or(false) {
            if let Ok(Some(t)) = api.load_token_from_project() {
                let tok = t.trim().to_string();
                api.set_token(&tok);
                // Try to decode token payload and extract nombre_completo for nicer message
                println!();
                print_separator();
                if let Some(name) = extract_name_from_jwt(&tok) {
                    let title = format!("Bienvenido de vuelta: {}", name);
                    print_section(&title);
                } else {
                    print_section("Sesión restaurada automáticamente desde la sesión guardada.");
                }
            }
        }
    }

    // Mark this run as not clean yet; only set to true when exiting via the menu.
    // Doing this here (after reading previous meta) ensures an unclean shutdown
    // leaves clean_exit=false so the next run will not auto-login.
    let _ = api.set_clean_exit_meta(false);

    loop {
        print_header();
        // Build menu items; show upload only when a token is present.
        let mut items = Vec::new();
        let is_logged = api.has_token();
        if is_logged {
            items.push("Subir foto de perfil");
            items.push("Cerrar sesión");
        } else {
            items.push("Registrarse");
            items.push("Iniciar sesión");
        }
        items.push("Salir");

        let selection = Select::new().items(&items).default(0).interact()?;
        let choice = items[selection];

        match choice {
            "Registrarse" => {
                // Show a titled section for registration
                print_section("NeumoDiagnostics - Registro");
                // Allow user to cancel registration and return to the main menu
                if let Err(e) = handle_register(&api) {
                    // If the handler returned an error, surface it; otherwise continue
                    println!("Error en el flujo de registro: {}", e);
                }
                print_separator();
            }
            "Iniciar sesión" => {
                // Show a titled section for login
                print_section("NeumoDiagnostics - Iniciar sesión");
                // handle_login returns Ok(Some(token)) on success, Ok(None) when cancelled or failed
                if let Some(token) = handle_login(&api)? {
                    api.set_token(&token);
                    // Preguntar si se recuerda la sesión (Sí/No en español)
                    let remember_idx = Select::new()
                        .with_prompt("¿Recordar esta sesión en este equipo?")
                        .items(&["Sí", "No"]) 
                        .default(1)
                        .interact()?;
                    let remember = remember_idx == 0;
                    if remember {
                        api.persist_token_to_project(&token, true)?;
                    } else {
                        api.persist_token_to_project(&token, false)?;
                    }
                    println!("Sesión iniciada.");
                }
            }
            "Cerrar sesión" => {
                api.clear_token();
                // Always clear persisted token on explicit logout so the next run will not restore.
                api.clear_persisted_token_in_project();
                println!("Sesión cerrada.");
            }
            "Subir foto de perfil" => {
                // Show a titled section for uploading
                print_section("NeumoDiagnostics - Subir foto de perfil");
                if !api.has_token() {
                    println!("Debe iniciar sesión antes de subir una foto de perfil.");
                    continue;
                }

                // Provide an explicit cancel option so the user can return to the menu
                let pick_methods = vec!["Seleccionar archivo (GUI)", "Ingresar ruta manualmente", "Cancelar"];
                let pick = pick_methods[Select::new().items(&pick_methods).default(0).interact()?];

                if pick == "Cancelar" {
                    println!("Operación cancelada. Volviendo al menú.");
                    continue;
                }

                let pb_opt: Option<PathBuf> = if pick == "Seleccionar archivo (GUI)" {
                    match FileDialog::new().add_filter("Imagen", &["jpg", "jpeg", "png"]).pick_file() {
                        Some(p) => Some(p),
                        None => {
                            println!("No se seleccionó un archivo o el diálogo no está disponible.");
                            None
                        }
                    }
                } else {
                    let raw_path: String = Input::new().with_prompt("Ruta del archivo de imagen").interact_text()?;
                    let trimmed = raw_path.trim();
                    if trimmed.is_empty() {
                        println!("Ruta vacía: operación cancelada.");
                        None
                    } else {
                        let path = trimmed.trim_matches('"').trim_matches('\'').to_string();
                        Some(PathBuf::from(path))
                    }
                };

                if pb_opt.is_none() {
                    continue;
                }
                let pb = pb_opt.unwrap();

                use std::sync::mpsc::{channel, TryRecvError};
                let spinner = ProgressBar::new_spinner();
                spinner.set_style(ProgressStyle::with_template("{spinner} {msg}").unwrap());
                spinner.set_draw_target(ProgressDrawTarget::stderr());
                spinner.set_message("Subiendo la imagen...");

                // Run the blocking upload in a background thread and poll for the result
                let (tx, rx) = channel();
                let api_cloned = api.clone();
                let pb_clone = pb.clone();
                std::thread::spawn(move || {
                    let r = api_cloned.upload_profile_picture(&pb_clone);
                    let _ = tx.send(r);
                });

                // Poll for the result while ticking the spinner and ensure minimum display time
                let start = Instant::now();
                loop {
                    match rx.try_recv() {
                        Ok(res) => {
                            // if result arrived too quickly, keep spinning until min time
                            while start.elapsed().as_millis() < MIN_SPINNER_MS as u128 {
                                spinner.tick();
                                thread::sleep(Duration::from_millis(80));
                            }
                            spinner.finish_and_clear();
                            match res {
                                Ok(_) => println!("Imagen de perfil cargada exitosamente."),
                                Err(e) => println!("Fallo la subida: {}", e),
                            }
                            break;
                        }
                        Err(TryRecvError::Empty) => {
                            spinner.tick();
                            thread::sleep(Duration::from_millis(80));
                        }
                        Err(_) => {
                            spinner.finish_and_clear();
                            println!("Fallo interno: no se pudo obtener el resultado de la subida.");
                            break;
                        }
                    }
                }
            }
            "Salir" => {
                let _ = api.set_clean_exit_meta(true);
                println!("Saliendo...");
                break
            }
            _ => {}
        }
        println!("");
    }
    Ok(())
}

/// Collect input fields for registration and call `ApiClient::register`.
fn handle_register(api: &ApiClient) -> Result<()> {
    // Allow immediate cancel of the registration flow
    let start_idx = Select::new()
        .with_prompt("¿Desea continuar con el registro o cancelar?")
        .items(&["Continuar", "Cancelar"]) 
        .default(0)
        .interact()?;
    if start_idx == 1 {
        println!("Registro cancelado. Volviendo al menú.");
        return Ok(());
    }
    // If the user chose to continue, clean up the prompt lines so the
    // terminal doesn't keep showing the temporary selector. 6 lines is
    // a conservative clearance for the prompt + selector display.
    clear_previous_lines(1);

    // `Input::interact_text()` prompts the user for input and returns it.
    let nombre: String = Input::new().with_prompt("Nombre completo").interact_text()?;
    let edad: i32 = Input::new().with_prompt("Edad").interact_text()?;
    // Show role choices with capitalized first letter
    let rol_choices = vec!["Doctor", "Paciente"];
    let rol_idx = Select::new().with_prompt("Rol").items(&rol_choices).default(1).interact()?;
    let rol = rol_choices[rol_idx].to_lowercase();
    let identificacion: String = Input::new().with_prompt("Identificación").interact_text()?;
    let correo: String = Input::new().with_prompt("Correo electrónico").interact_text()?;
    // `Password` hides input in terminal for passwords. Request confirmation.
    // If the passwords don't match, allow the user to retry entering only
    // the passwords or cancel the registration — do not force restarting
    // the whole form.
    let contrasena: String = loop {
        let p = Password::new().with_prompt("Contraseña").interact()?;
        let pc = Password::new().with_prompt("Confirmar contraseña").interact()?;
        if p == pc {
            break p;
        }
        println!("Las contraseñas no coinciden.");
        let retry = Select::new()
            .with_prompt("¿Desea reintentar la contraseña o cancelar el registro?")
            .items(&["Reintentar", "Cancelar"]) 
            .default(0)
            .interact()?;
        if retry == 1 {
            println!("Registro cancelado. Volviendo al menú.");
            return Ok(());
        }
        // otherwise loop and ask for passwords again
    };
    // Keep the consent choice visible and persistent. Use Spanish Sí/No selection
    let acepta_idx = Select::new()
        .with_prompt("¿Acepta el tratamiento de datos?")
        .items(&["Sí", "No"]) 
        .default(1)
        .interact()?;
    let acepta = acepta_idx == 0;

    print_separator();
    print_section("NeumoDiagnostics - Resumen de registro");
    println!("Nombre: {}", nombre);
    println!("Edad: {}", edad);
    println!("Rol: {}", rol_choices[rol_idx]);
    println!("Identificación: {}", identificacion);
    println!("Correo: {}", correo);
    println!("Acepta tratamiento de datos: {}", if acepta { "Sí" } else { "No" });

    let req = RegisterRequest {
        nombre_completo: nombre,
        edad,
        rol,
        identificacion,
        correo,
        contrasena,
        acepta_tratamiento_datos: acepta,
    };

    // Final confirmation before registering — show data and ask Sí/No
    print_separator();
    println!("¿Confirmar registro con los datos mostrados? ");
    let confirm_idx = Select::new().items(&["Sí", "No"]).default(0).interact()?;
    if confirm_idx == 0 {
        // show spinner for UX, then call the API
        use std::sync::mpsc::{channel, TryRecvError};

        let spinner = ProgressBar::new_spinner();
        spinner.set_style(ProgressStyle::with_template("{spinner} {msg}").unwrap());
        spinner.set_draw_target(ProgressDrawTarget::stderr());
        spinner.set_message("Registrando...");

        let (tx, rx) = channel();
        let api_cloned = api.clone();
        let req_clone = req.clone();
        std::thread::spawn(move || {
            let r = api_cloned.register(&req_clone);
            let _ = tx.send(r);
        });

        let start = Instant::now();
        loop {
            match rx.try_recv() {
                Ok(res) => {
                    while start.elapsed().as_millis() < MIN_SPINNER_MS as u128 {
                        spinner.tick();
                        thread::sleep(Duration::from_millis(80));
                    }
                    spinner.finish_and_clear();
                    match res {
                        Ok(_) => println!("Registrado exitosamente, por favor inicie sesión."),
                        Err(e) => println!("Fallo el registro: {}", e),
                    }
                    break;
                }
                Err(TryRecvError::Empty) => {
                    spinner.tick();
                    thread::sleep(Duration::from_millis(80));
                }
                Err(_) => {
                    spinner.finish_and_clear();
                    println!("Fallo interno: no se pudo obtener el resultado del registro.");
                    break;
                }
            }
        }
    } else {
        println!("Registro cancelado. Revise sus datos e intente de nuevo.");
    }
    Ok(())
}

/// Collect credentials and perform login, returning the JWT token if OK.
fn handle_login(api: &ApiClient) -> Result<Option<String>> {
    // Allow immediate cancel of the login flow
    let start_idx = Select::new()
        .with_prompt("¿Desea continuar con el inicio de sesión o cancelar?")
        .items(&["Continuar", "Cancelar"]) 
        .default(0)
        .interact()?;
    if start_idx == 1 {
        println!("Inicio de sesión cancelado. Volviendo al menú.");
        return Ok(None);
    }
    // Hide the initial selector when continuing so the form appears cleanly.
    clear_previous_lines(1);

    let correo: String = Input::new().with_prompt("Correo electrónico").interact_text()?;
    let contrasena: String = Password::new().with_prompt("Contraseña").interact()?;
    let req = AuthRequest { correo, contrasena };

    use std::sync::mpsc::{channel, TryRecvError};

    let spinner = ProgressBar::new_spinner();
    spinner.set_style(ProgressStyle::with_template("{spinner} {msg}").unwrap());
    spinner.set_draw_target(ProgressDrawTarget::stderr());
    spinner.set_message("Iniciando sesión...");

    let (tx, rx) = channel();
    let api_cloned = api.clone();
    let req_clone = req.clone();
    std::thread::spawn(move || {
        let r = api_cloned.login(&req_clone);
        let _ = tx.send(r);
    });

    let start = Instant::now();
    loop {
        match rx.try_recv() {
            Ok(res) => {
                while start.elapsed().as_millis() < MIN_SPINNER_MS as u128 {
                    spinner.tick();
                    thread::sleep(Duration::from_millis(80));
                }
                spinner.finish_and_clear();
                match res {
                    Ok(resp) => return Ok(Some(resp.token)),
                    Err(e) => {
                        let err_text = e.to_string();
                        let lower = err_text.to_lowercase();
                        if lower.contains("bcrypt") || lower.contains("hashedpassword") || lower.contains("usuario no encontrado") || lower.contains("no rows") || lower.contains("invalid") || lower.contains("bad request") {
                            println!("Credenciales inválidas: correo o contraseña incorrectos.");
                        } else {
                            println!("Fallo al iniciar sesión: {}", e);
                        }
                        return Ok(None);
                    }
                }
            }
            Err(TryRecvError::Empty) => {
                spinner.tick();
                thread::sleep(Duration::from_millis(80));
            }
            Err(_) => {
                spinner.finish_and_clear();
                println!("Fallo interno: no se pudo obtener el resultado del inicio de sesión.");
                return Ok(None);
            }
        }
    }
}

// Token persistence is handled by helpers in `ApiClient` which persist
// the token next to the `Cargo.toml` (project folder) and manage a small
// meta JSON file. See `ApiClient::persist_token_to_project` and
// `ApiClient::load_token_from_project`.

// Try to extract "nombre_completo" from a JWT token without verifying signature.
// This is only for display purposes when restoring a session.
fn extract_name_from_jwt(token: &str) -> Option<String> {
    // JWT is three base64url parts separated by '.'; we want the payload (2nd part)
    let parts: Vec<&str> = token.split('.').collect();
    if parts.len() != 3 {
        return None;
    }
    let payload_b64 = parts[1];
    // base64 in JWT is URL-safe without padding; standard engine accepts padded base64,
    // try to add padding if necessary.
    let mut s = payload_b64.replace('-', "+").replace('_', "/");
    while s.len() % 4 != 0 { s.push('='); }
    let decoded = base64_standard.decode(&s).ok()?;
    let json: serde_json::Value = serde_json::from_slice(&decoded).ok()?;
    json.get("nombre_completo").and_then(|v| v.as_str()).map(|s| s.to_string())
}
