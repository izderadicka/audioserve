fn main() {
    #[cfg(feature="static")]
    {
    println!("cargo:rustc-link-lib=static=ssl");
    println!("cargo:rustc-link-lib=static=crypto");

    }
}