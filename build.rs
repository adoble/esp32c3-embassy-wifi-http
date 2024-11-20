fn main() {
    // Uncomments this ljne leads to a linkage failure.
    // Note that this (and other flags) are already set in .cargo/config.toml
    //println!("cargo:rustc-link-arg-bins=-Tlinkall.x");
}
