//! Links the game into the program on iOS: libDunk.a, from the folder RETRO_DUNK_LIB names
//! (where scripts/meadow-for.sh put it), with what Meadow's runtime asks of the system.
fn main() {
    println!("cargo:rerun-if-env-changed=RETRO_DUNK_LIB");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("ios") {
        return;
    }
    let folder = std::env::var("RETRO_DUNK_LIB").expect("RETRO_DUNK_LIB names the folder libDunk.a is in");
    println!("cargo:rerun-if-changed={folder}/libDunk.a");
    println!("cargo:rustc-link-search=native={folder}");
    println!("cargo:rustc-link-lib=static=Dunk");
    println!("cargo:rustc-link-lib=iconv");
    println!("cargo:rustc-link-lib=framework=Security");
    println!("cargo:rustc-link-lib=framework=CoreFoundation");
}