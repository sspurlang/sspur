fn main() {
    // Chained fixups let dyld bind the binary's pointers as their pages are touched instead of
    // rewriting all of __DATA_CONST at launch, which keeps `sspur run`'s cached path small.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        println!("cargo:rustc-link-arg-bin=sspur=-Wl,-fixup_chains");
    }
}
