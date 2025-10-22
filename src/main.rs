// Binary entrypoint
// ------------------
// Keep `main` tiny: construct dependencies and start the interactive
// menu implemented in `ui::main_menu`. Returning `anyhow::Result` lets
// us use the `?` operator for concise error propagation in this small
// prototype.

use neumodiag_cli::{ui::main_menu, api::ApiClient};

fn main() -> anyhow::Result<()> {
    // Build an ApiClient. It reads `API_GATEWAY_URL` from the
    // environment (if present) or falls back to http://localhost:8081.
    // This lets you point the CLI at a different backend without
    // recompiling.
    let api = ApiClient::from_env()?;

    // Run the main interactive menu. This function blocks until the
    // user chooses to exit; it owns the UI loop and delegates network
    // actions to `ApiClient`.
    main_menu(api)?;
    Ok(())
}
