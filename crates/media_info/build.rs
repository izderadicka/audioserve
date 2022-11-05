fn main() {
    const FFMPEG_VERSION: &str = "ffmpeg-4.3.1"; //"ffmpeg-5.0.1";
    let ffi_src = format!("src/ffi_{}.rs", FFMPEG_VERSION);
    let ffi_target =
        std::path::Path::new(&std::env::var("OUT_DIR").unwrap()).join("current_ffi.rs");
    std::fs::copy(ffi_src, ffi_target).unwrap();
    #[cfg(any(feature = "static", feature = "partially-static"))]
    {
        use std::env;
        use std::path;
        use std::process;

        let out_dir = env::var("OUT_DIR").unwrap();
        let ffmpeg_dir = path::Path::new(&out_dir).join(FFMPEG_VERSION);
        if !ffmpeg_dir.exists() {
            use std::fs::File;
            let fflog = File::create(path::Path::new(&out_dir).join("ffmpeg-compilation.log"))
                .expect("cannot create log file");
            let rc = process::Command::new("./build_ffmpeg.sh")
                .arg(&out_dir)
                .env("FFMPEG_VERSION", FFMPEG_VERSION)
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
            out_dir, FFMPEG_VERSION
        );
        println!(
            "cargo:rustc-link-search=native={}/{}/libavutil",
            out_dir, FFMPEG_VERSION
        );
        println!(
            "cargo:rustc-link-search=native={}/{}/libavcodec",
            out_dir, FFMPEG_VERSION
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
