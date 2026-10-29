//! On macOS, embeds packaging/macos/Info.plist in eco's own `__info_plist`
//! section: the daemon launchd runs is the process macOS asks the user about
//! for the microphone and the audio it plays, and it reads the reason there.

fn main() {
    let plist = concat!(env!("CARGO_MANIFEST_DIR"), "/packaging/macos/Info.plist");
    println!("cargo:rerun-if-changed={plist}");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        println!("cargo:rustc-link-arg-bin=eco=-Wl,-sectcreate,__TEXT,__info_plist,{plist}");
    }
}
