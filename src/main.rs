// Entrypoint for the CLI application.
// - Keeps `main` small: create an API client and hand it to the UI loop.
// - Returns `anyhow::Result` to simplify error handling for the prototype.

use neumodiag_cli::{ui::main_menu, api::ApiClient};

fn main() -> anyhow::Result<()> {
    // Create API client configured by environment variable `API_GATEWAY_URL`
    // or default to http://localhost:8000. See `api::ApiClient::from_env`.
    let api = ApiClient::from_env()?;

    // Start the interactive menu. This call blocks until the user exits.
    main_menu(api)?;
    Ok(())
}
