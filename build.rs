use std::{env, process::Command};

fn main() {
    println!(
        "cargo:rustc-env=AUDIOSERVE_LONG_VERSION={}",
        get_long_version()
    );
    println!("cargo:rustc-env=AUDIOSERVE_FEATURES={}", get_features());
}

fn get_long_version() -> String {
    let ver = env::var("CARGO_PKG_VERSION").expect("cargo version is missing");
    let commit = get_commit();
    format!("{} #{}", ver, commit)
}

const FEATURE_PREFIX: &str = "CARGO_FEATURE_";

fn get_features() -> String {
    env::vars()
        .filter_map(|(v, _)| v.strip_prefix(FEATURE_PREFIX).map(|s| s.to_string()))
        .map(|f| f.to_ascii_lowercase().replace('_', "-"))
        .collect::<Vec<_>>()
        .join(", ")
}

fn get_commit() -> String {
    Command::new("git")
        .args(&["rev-parse", "HEAD"])
        .output()
        .map(|mut o| {
            o.stdout.truncate(7);
            String::from_utf8(o.stdout).expect("git output must be utf-8")
        })
        .unwrap_or_else(|e| {
            eprintln!("Error running git: {}", e);
            "?".to_string()
        })
}
