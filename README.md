NeumoDiagnostics CLI (prototype)

Features implemented in this prototype:
- Register (POST /register)
- Login (POST /auth)
- Upload Profile Picture (POST /upload/profile)

How to build:
- Install Rust toolchain (rustup)
- cd cmd-front-end/cli
- cargo build --release

How to run:
- Set API_GATEWAY_URL env var if gateway is not at http://localhost:8000
- cargo run --release

Notes:
- Token is persisted at ~/.neumodiag_token after successful login.
- This prototype uses blocking reqwest for simplicity.
- Upload endpoint path `/upload/profile` is a convenience route; adapt if gateway exposes different path.
