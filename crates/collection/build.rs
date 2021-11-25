fn main() {
    #[cfg(feature="static")]
    {
        println!("cargo:rustc-link-search=native=/usr/lib");
        println!("cargo:rustc-link-search=native=/lib");
        println!("cargo:rustc-link-lib=static=clang");
        println!("cargo:rustc-link-lib=static=icui18n");
        println!("cargo:rustc-link-lib=static=z");
    }
}