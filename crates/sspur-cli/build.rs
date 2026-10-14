fn main() {
    // Chained fixups let dyld bind the binary's pointers as their pages are touched instead of
    // rewriting all of __DATA_CONST at launch, which keeps `sspur run`'s cached path small.
    // Apple Silicon only: every macOS that runs on it loads this format; older Intel releases may not.
    let arm_mac = std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") && std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() == Ok("aarch64");
    if arm_mac {
        println!("cargo:rustc-link-arg-bin=sspur=-Wl,-fixup_chains");
    }
}
