const FFMPEG_VERSION_4: &str = "ffmpeg-4.3.1";
const FFMPEG_VERSION_5: &str = "ffmpeg-5.0.1";
const FFMPEG_VERSION_6: &str = "ffmpeg-6.0";

macro_rules! warn {
    ($fmt:literal $(, $arg:expr)* ) => {
        println!(concat!("cargo:warning=", $fmt), $($arg)*)
    };
}

fn parse_main_version(version: &str) -> Option<u32> {
    let mut parts = version.split('.');
    parts.next().and_then(|first| first.parse().ok())
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    let ffmpeg_version = if cfg!(feature = "static") || cfg!(feature = "partially-static") {
        FFMPEG_VERSION_6
    } else {
        match pkg_config::probe_library("libavformat") {
            Ok(lib) => {
                if let Some(version) = parse_main_version(&lib.version) {
                    if version > 60 {
                        warn!("libavformat is too new - will try latest ffi, but may not work");
                        FFMPEG_VERSION_6
                    } else if version == 60 {
                        FFMPEG_VERSION_6
                    } else if version == 59 {
                        FFMPEG_VERSION_5
                    } else if version == 58 {
                        FFMPEG_VERSION_4
                    } else {
                        panic!("libavformat version is too old {}", lib.version);
                    }
                } else {
                    panic!("Invalid version of libavformat: {}", lib.version);
                }
            }
            Err(e) => {
                warn!("Cannot find libavformat: {}", e);
                FFMPEG_VERSION_4
            }
        }
    };
    let ffi_src = format!("src/ffi_{}.rs", ffmpeg_version);
    let ffi_target =
        std::path::Path::new(&std::env::var("OUT_DIR").unwrap()).join("current_ffi.rs");
    std::fs::copy(ffi_src, ffi_target).unwrap();
    #[cfg(any(feature = "static", feature = "partially-static"))]
    {
        use std::env;
        use std::path;
        use std::process;

        let out_dir = env::var("OUT_DIR").unwrap();
        let ffmpeg_dir = path::Path::new(&out_dir).join(ffmpeg_version);
        if !ffmpeg_dir.exists() {
            use std::fs::File;
            let fflog = File::create(path::Path::new(&out_dir).join("ffmpeg-compilation.log"))
                .expect("cannot create log file");
            let rc = process::Command::new("./build_ffmpeg.sh")
                .arg(&out_dir)
                .env("FFMPEG_VERSION", ffmpeg_version)
                .stdout(fflog.try_clone().expect("Cannot clone file"))
                .stderr(fflog)
                .status()
                .expect("cannot run ffmpeg build script");
            if !rc.success() {
                panic!(
                    "ffmpeg build script failed with {:?}, check log at {}",
                    rc.code(),
                    &out_dir
                );
            }
        }

        println!("cargo:rustc-link-lib=static=avformat");
        println!("cargo:rustc-link-lib=static=avutil");
        println!("cargo:rustc-link-lib=static=avcodec");
        println!(
            "cargo:rustc-link-search=native={}/{}/libavformat",
            out_dir, ffmpeg_version
        );
        println!(
            "cargo:rustc-link-search=native={}/{}/libavutil",
            out_dir, ffmpeg_version
        );
        println!(
            "cargo:rustc-link-search=native={}/{}/libavcodec",
            out_dir, ffmpeg_version
        );
    }

    #[cfg(feature = "static")]
    {
        println!("cargo:rustc-link-lib=static=z");
        println!("cargo:rustc-link-lib=static=bz2");
        println!("cargo:rustc-link-search=native=/usr/lib");
        println!("cargo:rustc-link-search=native=/lib");
    }

    #[cfg(feature = "partially-static")]
    {
        println!("cargo:rustc-link-lib=z");
        println!("cargo:rustc-link-lib=bz2");
    }

    #[cfg(all(not(feature = "static"), not(feature = "partially-static")))]
    {
        println!("cargo:rustc-link-lib=avformat");
        println!("cargo:rustc-link-lib=avutil");
        println!("cargo:rustc-link-lib=avcodec");
    }
}
