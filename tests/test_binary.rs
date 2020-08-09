use anyhow::Result;
use escargot::CargoBuild;
use reqwest::{blocking::Client, StatusCode};
use serde_json::Value;

const BASE_URL: &str = "http://localhost:3000";

fn make_url(path: &str) -> String {
    BASE_URL.to_string() + path
}

#[test]
#[ignore]
fn test_binary() -> Result<()> {
    let bin = CargoBuild::new()
        .bin("audioserve")
        .features("transcoding-cache partially-static")
        .run()?;

    eprintln!("Binary is at {:?}", bin.path());

    let mut cmd = bin.command();
    cmd.args(&[
        "--no-authentication",
        "--listen",
        "127.0.0.1:3000",
        "test_data",
    ])
    .env("RUST_LOG", "audioserve=debug");
    let mut proc = cmd.spawn()?;

    eprintln!("Process is running with id {}", proc.id());

    let client = Client::new();

    let resp;
    let mut retries = 5;

    loop {
        let r = client.get(&make_url("")).send();
        match r {
            Ok(r) => {
                resp = r;
                break;
            }
            Err(e) if retries > 0 => eprint!("Error connecting audioserve {}", e),
            Err(e) if retries == 0 => panic!("Cannot connect to server, error: {}", e),
            Err(_) => unreachable!(),
        }

        retries -= 1;
        std::thread::sleep(std::time::Duration::from_secs(5 - retries))
    }

    assert_eq!(StatusCode::OK, resp.status());
    let mime = resp.headers().get("Content-Type").unwrap();
    assert_eq!("text/html", mime.to_str()?);

    let collections: Value = client.get(&make_url("/collections")).send()?.json()?;
    if let Value::Object(o) = collections {
        let cols = o.get("names").unwrap();
        if let Value::Array(v) = cols {
            assert_eq!(1, v.len());
            if let Value::String(s) = &v[0] {
                assert_eq!("test_data", s)
            } else {
                panic!("names[0] is not string");
            }
        } else {
            panic!("names are not array");
        }
    } else {
        panic!("Collections are not JSON object")
    }

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
