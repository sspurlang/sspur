class Sspur < Formula
  desc "Programming language for AI agents to write, read and maintain"
  homepage "https://github.com/sspurlang/sspur"
  version "@VERSION@"
  license any_of: ["MIT", "Apache-2.0"]

  on_macos do
    on_arm do
      url "https://github.com/sspurlang/sspur/releases/download/v@VERSION@/sspur-v@VERSION@-aarch64-apple-darwin.tar.gz"
      sha256 "@SHA_AARCH64_APPLE_DARWIN@"
    end
    on_intel do
      url "https://github.com/sspurlang/sspur/releases/download/v@VERSION@/sspur-v@VERSION@-x86_64-apple-darwin.tar.gz"
      sha256 "@SHA_X86_64_APPLE_DARWIN@"
    end
  end

  on_linux do
    depends_on "llvm"

    on_arm do
      url "https://github.com/sspurlang/sspur/releases/download/v@VERSION@/sspur-v@VERSION@-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "@SHA_AARCH64_UNKNOWN_LINUX_GNU@"
    end
    on_intel do
      url "https://github.com/sspurlang/sspur/releases/download/v@VERSION@/sspur-v@VERSION@-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "@SHA_X86_64_UNKNOWN_LINUX_GNU@"
    end
  end

  depends_on "z3" => :recommended

  def install
    bin.install "sspur"
    doc.install "README.md", "CHANGELOG.md"
  end

  def caveats
    <<~EOS
      SSPUR compiles to native code through clang. On macOS it comes with
      the Xcode Command Line Tools (xcode-select --install).
    EOS
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/sspur --version")
    (testpath/"t.ssp").write <<~SSP
      fn add(a: Int, b: Int) -> Int
      = a + b

      test adds = add(2, 3) == 5
    SSP
    assert_match "1 passed, 0 failed", shell_output("#{bin}/sspur test --interp #{testpath}/t.ssp")
  end
end
