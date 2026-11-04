#[test]
fn public_mcp_contract() {
    for backend in ["turso", "surrealdb"] {
        let status = std::process::Command::new("python3")
            .arg("tests/process.py")
            .arg(env!("CARGO_BIN_EXE_tuck"))
            .env("TUCK_BACKEND", backend)
            .env(
                "TUCK_TEST_HOOKS",
                if cfg!(feature = "test-hooks") {
                    "1"
                } else {
                    "0"
                },
            )
            .status()
            .expect("python3 must be installed");
        assert!(status.success(), "{backend} public MCP contract failed");
    }
}
