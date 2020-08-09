use anyhow::Result;
use escargot::CargoBuild;
use reqwest::{blocking::Client, StatusCode};
use serde_json::Value;

const BASE_URL: &str = "http://localhost:3000";

fn make_url(path: &str) -> String {
    BASE_URL.to_string() + path
}

#[test]
fn test_binary() -> Result<()> {
    let bin = CargoBuild::new()
        .bin("audioserve")
        .features("transcoding-cache partially-static")
        .run()?;

    eprintln!("Binary is at {:?}", bin.path());

    let mut cmd = bin.command();
    cmd.args(&["--no-authentication", "test_data"])
        .env("RUST_LOG", "audioserve=debug");
    let mut proc = cmd.spawn()?;

    eprintln!("Process is running with id {}", proc.id());

    let client = Client::new();

    let resp = client.get(&make_url("")).send()?;

    assert_eq!(StatusCode::OK, resp.status());
    let mime = resp.headers().get("Content-Type").unwrap();
    assert_eq!("text/html", mime.to_str()?);

    let _collections: Value = client.get(&make_url("/collections")).send()?.json()?;

    #[cfg(not(unix))]
    proc.kill();

    #[cfg(unix)]
    {
        use nix::{sys::signal, unistd::Pid};
        signal::kill(Pid::from_raw(proc.id() as i32), signal::SIGINT)
            .unwrap_or_else(|e| eprintln!("Cannot kill process {}, error {}", proc.id(), e));
    }
    let status = proc.wait().unwrap();
    eprintln!("Exit status is {:?}", status.code());
    Ok(())
}
