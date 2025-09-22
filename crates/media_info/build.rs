const FFMPEG_VERSION_4: &str = "ffmpeg-4.4.6";
const FFMPEG_VERSION_5: &str = "ffmpeg-5.1.7";
const FFMPEG_VERSION_6: &str = "ffmpeg-6.1.1";
const FFMPEG_VERSION_7: &str = "ffmpeg-7.1.1";
const FFMPEG_VERSION_8: &str = "ffmpeg-8.0";

// macro_rules! warn {
//     ($fmt:literal $(, $arg:expr)* ) => {
//         println!(concat!("cargo:warning=", $fmt), $($arg)*)
//     };
// }

fn parse_main_version(version: &str) -> Option<u32> {
    let mut parts = version.split('.');
    parts.next().and_then(|first| first.parse().ok())
}

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    let ffmpeg_version = if cfg!(feature = "static") || cfg!(feature = "partially-static") {
        FFMPEG_VERSION_6
    } else {
        let pkg = pkg_config::Config::new()
            .print_system_cflags(true)
            .print_system_libs(true)
            .probe("libavformat");
        match pkg {
            Ok(lib) => {
                if let Some(version) = parse_main_version(&lib.version) {
                    if version > 62 {
                        panic!("libavformat is too new - need to update source with new ffi");
                    } else if version == 62 {
                        FFMPEG_VERSION_8
                    } else if version == 61 {
                        FFMPEG_VERSION_7
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
                if cfg!(windows) {
                    get_ffmpeg_version_windows()
                } else {
                    panic!("Cannot find libavformat: {}", e);
                }
            }
        }
    };

    generate_ffi(ffmpeg_version);
    let ffi_src = format!("src/ffi_{}.rs", ffmpeg_version);
    eprintln!("ffmpeg version: {}", ffmpeg_version);
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

fn generate_ffi(ffmpeg_version: &str) {
    let mut args = vec![format!("-I{ffmpeg_version}")];
    let out_file = format!("src/ffi_{ffmpeg_version}.rs");

    if cfg!(windows) {
        println!("cargo:rustc-link-search=C:/ffmpeg/lib");
        args.push("-IC:/ffmpeg/include".to_string());
    }

    println!("cargo:rustc-link-lib=avformat");
    println!("cargo:rustc-link-lib=avutil");
    println!("cargo:rustc-link-lib=avcodec");

    let _bindings = bindgen::Builder::default()
        .header("wrapper.h")
        // Tell cargo to invalidate the built crate whenever any of the
        // included header files changed.
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
        .generate_comments(false)
        .allowlist_type("AVFormatContext")
        .allowlist_type("AVDictionary")
        .allowlist_type("AVChapter")
        .allowlist_type("AVRational")
        .allowlist_function("av_dict_get")
        .allowlist_function("av_dict_count")
        .allowlist_function("av_log_set_level")
        .allowlist_function("av_register_all")
        .allowlist_function("avformat_version")
        .allowlist_function("avformat_alloc_context")
        .allowlist_function("avformat_open_input")
        .allowlist_function("avformat_find_stream_info")
        .allowlist_function("avformat_close_input")
        .allowlist_function("av_dump_format")
        .allowlist_var("AV_LOG_QUIET")
        .allowlist_var("AV_DICT_IGNORE_SUFFIX")
        .allowlist_var("AV_TIME_BASE")
        .clang_args(args)
        .generate()
        .expect("Unable to generate bindings")
        .write_to_file(out_file)
        .expect("Couldn't write bindings!");
}

fn get_ffmpeg_version_windows() -> &'static str {
    let paths = std::fs::read_dir("C:/ffmpeg/bin").unwrap();
    for path in paths {
        let path = path.unwrap().path();
        let file_name = path.file_name().unwrap().to_str().unwrap();
        if file_name.starts_with("avformat-") && file_name.ends_with(".dll") {
            let version = file_name
                .trim_start_matches("avformat-")
                .trim_end_matches(".dll")
                .parse::<u32>()
                .unwrap_or(0);
            if version > 62 {
                panic!("libavformat is too new - need to update source with new ffi");
            } else if version == 62 {
                return FFMPEG_VERSION_8;
            } else if version == 61 {
                return FFMPEG_VERSION_7;
            } else if version == 60 {
                return FFMPEG_VERSION_6;
            } else if version == 59 {
                return FFMPEG_VERSION_5;
            } else if version == 58 {
                return FFMPEG_VERSION_4;
            } else {
                panic!("unknown libavformat version {}", version);
            }
        }
    }
    panic!("Unknown ffmpeg location");
}
