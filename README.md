NeumoDiagnostics CLI (prototype)

This is a small command-line prototype client for the NeumoDiagnostics authentication and profile APIs. 

Implemented features
- Register (POST /register)
- Login (POST /auth)
- Upload profile picture (POST /upload) — multipart field name: `foto`

Build
- Install Rust (rustup) and ensure `cargo` is on your PATH.
- From the `cli-front-end` folder run:

```cmd
cargo build --release
```

Run
- By default the CLI will target the auth backend at `http://localhost:8081`. To override the API base URL set the `API_GATEWAY_URL` environment variable.

Windows (cmd.exe) example:

```cmd
set API_GATEWAY_URL=http://localhost:8081
cargo run --release
```

Unix / PowerShell examples:

```powershell
$env:API_GATEWAY_URL = 'http://localhost:8081'
./cargo run --release
```

Token persistence and auto-login
- After a successful login the CLI can optionally remember your session token. The token is saved in the project folder next to the `Cargo.toml` file as two files:
	- `.neumodiag_token` — contains the raw JWT token (no encryption)
	- `.neumodiag_token.meta` — JSON metadata: { "persist": bool, "clean_exit": bool }

- Auto-login rules:
	- On startup the CLI attempts to auto-restore a saved session only when both:
		1) a token file exists, and
		2) the token metadata `clean_exit` is `true` (this indicates the previous run exited via the menu "Salir").
	- On explicit logout the token and metadata files are removed to prevent accidental auto-restore.

Security notes
- The token is stored in plain text in the project folder for convenience. This is convenient for local testing but not secure for production. Do not commit these files to version control.
- The token files are already added to `.gitignore` in this repo. If you prefer a different location (for example a hidden `.neumodiag/` folder), you can edit `src/api.rs` to change the storage path.

Endpoints and multipart uploads
- The CLI expects the auth backend to expose the following endpoints by default:
	- POST /register
	- POST /auth
	- POST /upload — multipart form upload with the file field named `foto`