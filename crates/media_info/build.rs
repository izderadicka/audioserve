

fn main() {

    #[cfg(any(feature="static", feature="partially-static"))]
    {

        use std::env;
        use std::process;
        use std::path;
        
        let out_dir = env::var("OUT_DIR").unwrap();
        let ffmpeg_dir = path::Path::new(&out_dir).join("ffmpeg-4.1");
        if ! ffmpeg_dir.exists() {
            use std::fs::File;
            let fflog = File::create(path::Path::new(&out_dir).join("ffmpeg-compilation.log")).expect("cannot create log file");
            let rc =  process::Command::new("./build_ffmpeg.sh")
            .arg(&out_dir)
            .stdout(fflog)
            .status().expect("cannot run ffmpeg build script");
            if ! rc.success() {
                panic!("ffmpeg build script failed with {:?}, check log at {}", rc.code(), &out_dir);
            }

        }
        

        println!("cargo:rustc-link-lib=static=avformat");
        println!("cargo:rustc-link-lib=static=avutil");
        println!("cargo:rustc-link-lib=static=avcodec");
        println!("cargo:rustc-link-search=native={}/ffmpeg-4.1.5/libavformat", out_dir);
        println!("cargo:rustc-link-search=native={}/ffmpeg-4.1.5/libavutil", out_dir);
        println!("cargo:rustc-link-search=native={}/ffmpeg-4.1.5/libavcodec", out_dir);
       
    }

    #[cfg(feature="static")]
    {
        println!("cargo:rustc-link-lib=static=z");
        println!("cargo:rustc-link-lib=static=bz2");
        println!("cargo:rustc-link-search=native=/usr/lib");
        println!("cargo:rustc-link-search=native=/lib");
    }

    #[cfg(feature="partially-static")]
    {
        println!("cargo:rustc-link-lib=z");
        println!("cargo:rustc-link-lib=bz2"); 
    }

    #[cfg(all(not(feature="static"), not(feature="partially-static")))]
    {
        println!("cargo:rustc-link-lib=avformat");
        println!("cargo:rustc-link-lib=avutil");
        println!("cargo:rustc-link-lib=avcodec");
    }
}
