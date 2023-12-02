#![allow(dead_code)]

use anyhow::{Error, Result};
use escargot::CargoBuild;
use headers::HeaderValue;
use reqwest::{blocking::Client, header::HeaderMap, StatusCode};
use serde_json::Value;
use std::io;

fn make_url(path: &str, port: u16) -> String {
    format!("http://localhost:{}", port) + path
}

fn extract_value<'a>(mut v: &'a Value, path: &str) -> Result<&'a Value> {
    let components = path.split('.');
    for mut part in components {
        let mut idx: Option<usize> = None;
        if part.ends_with("]") {
            let i = part
                .find("[")
                .ok_or_else(|| Error::msg("invalid array index"))?;
            let n = part[i + 1..part.len() - 1].parse()?;
            idx = Some(n);
            part = &part[..i];
        }
        if !part.is_empty() {
            if let Value::Object(o) = v {
                v = o
                    .get(part)
                    .ok_or_else(|| Error::msg(format!("Invalid object key {}", part)))?
            } else {
                return Err(Error::msg("value is not an object"));
            };
        }

        match (v, idx) {
            (Value::Array(arr), Some(idx)) => {
                v = arr.get(idx).ok_or_else(|| Error::msg("invalid index"))?
            }
            (_, Some(_)) => return Err(Error::msg("value is not an array")),
            (_, None) => (),
        }
    }

    Ok(v)
}

fn string_value(v: &Value) -> Result<&str> {
    if let Value::String(ref s) = v {
        Ok(s.as_str())
    } else {
        Err(Error::msg("value is not string"))
    }
}

fn int_value(v: &Value) -> Result<i64> {
    if let Value::Number(n) = v {
        n.as_i64().ok_or_else(|| Error::msg("not integer value"))
    } else {
        Err(Error::msg("value is not integer"))
    }
}

fn float_value(v: &Value) -> Result<f64> {
    if let Value::Number(n) = v {
        n.as_f64().ok_or_else(|| Error::msg("not float value"))
    } else {
        Err(Error::msg("value is not float"))
    }
}

fn array_len(v: &Value) -> Result<usize> {
    if let Value::Array(a) = v {
        Ok(a.len())
    } else {
        Err(Error::msg("value is not array"))
    }
}

macro_rules! assert_header {
    ($resp:ident, $key: expr, $val: expr) => {
        let v = $resp
            .headers()
            .get($key)
            .ok_or_else(|| Error::msg(format!("header {} not found", $key)))?;
        assert_eq!($val, v.to_str()?);
    };
}

#[test]
#[ignore]
fn test_binary() -> Result<()> {
    let tmp_dir = tempdir::TempDir::new("audioserve_bin_test")?;
    let bin = CargoBuild::new()
        .bin("audioserve")
        .features("transcoding-cache partially-static")
        .run()?;

    eprintln!("Binary is at {:?}", bin.path());

    let port: u16 = 3333;
    let listen_on = format!("127.0.0.1:{}", port);

    let mut cmd = bin.command();
    cmd.args(&[
        "--no-authentication",
        "--listen",
        listen_on.as_str(),
        "--data-dir",
        tmp_dir
            .path()
            .to_str()
            .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "Invalid temp path"))?,
        "test_data",
    ])
    .env("RUST_LOG", "audioserve=debug");
    let mut proc = cmd.spawn()?;

    eprintln!("Process is running with id {}", proc.id());

    let client = Client::new();

    let resp;
    let mut retries = 5;

    loop {
        let r = client.get(&make_url("", port)).send();
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
    assert_header!(resp, "Content-Type", "text/html");

    let jg = |path| -> Result<Value> {
        client
            .get(&make_url(path, port))
            .send()?
            .json::<Value>()
            .map_err(Error::new)
    };
    let collections = jg("/collections")?;
    assert_eq!(1, extract_value(&collections, "names").and_then(array_len)?);
    assert_eq!(
        "test_data",
        extract_value(&collections, "names[0]").and_then(string_value)?
    );

    let transcodings = jg("/transcodings")?;
    assert_eq!(
        64,
        extract_value(&transcodings, "high.bitrate").and_then(int_value)?
    );
    assert_eq!(
        "opus-in-ogg",
        extract_value(&transcodings, "high.name").and_then(string_value)?
    );

    let root_folder = jg("/folder/")?;
    assert_eq!(2, extract_value(&root_folder, "files").and_then(array_len)?);
    assert_eq!(
        2,
        extract_value(&root_folder, "subfolders").and_then(array_len)?
    );
    assert_eq!(
        "02-file.opus",
        extract_value(&root_folder, "files[0].name").and_then(string_value)?
    );
    assert_eq!(
        "audio/ogg",
        extract_value(&root_folder, "files[0].mime").and_then(string_value)?
    );
    assert_eq!(
        2,
        extract_value(&root_folder, "files[0].meta.duration").and_then(int_value)?
    );
    assert_eq!(
        48,
        extract_value(&root_folder, "files[0].meta.bitrate").and_then(int_value)?
    );

    assert_eq!(
        "audio/x-matroska",
        extract_value(&root_folder, "files[1].mime").and_then(string_value)?
    );

    let res = client
        .get(&make_url("/audio/03-file.mka?trans=l", port))
        .send()?;
    assert_eq!(StatusCode::OK, res.status());
    assert_header!(res, "Content-Type", "audio/ogg");
    assert_header!(res, "transfer-encoding", "chunked");
    assert_header!(res, "x-transcode", "codec=opus-in-ogg; bitrate=32");

    let mut range_headers = HeaderMap::new();
    range_headers.insert("Range", HeaderValue::from_str("bytes=0-100")?);
    let res = client
        .get(&make_url("/audio/02-file.opus?trans=0", port))
        .headers(range_headers)
        .send()?;
    assert_header!(res, "Content-Type", "audio/ogg");
    assert_eq!(StatusCode::PARTIAL_CONTENT, res.status());
    assert_header!(res, "content-range", "bytes 0-100/12480");
    assert_header!(res, "content-length", "101");

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
