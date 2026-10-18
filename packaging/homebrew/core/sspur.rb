# The formula to submit to Homebrew/homebrew-core after 1.0, so `brew install sspur` works. It
# builds from source, as homebrew-core requires; the tap (sspurlang/homebrew-sspur) ships the
# release binaries until then. Update url and sha256 to the release being submitted.
class Sspur < Formula
  desc "Programming language for AI agents to write, read and maintain"
  homepage "https://sspurlang.github.io/"
  url "https://github.com/sspurlang/sspur/archive/refs/tags/v1.0.0.tar.gz"
  sha256 "635e3ed801460aa519cdf683bcec4b1f3ff8ad29d1c423f0165d2e6a59544bad"
  license any_of: ["MIT", "Apache-2.0"]
  head "https://github.com/sspurlang/sspur.git", branch: "main"

  depends_on "rust" => :build
  # Native code is compiled by clang at run time; macOS ships it with the Command Line Tools.
  on_linux do
    depends_on "llvm"
  end

  def install
    system "cargo", "install", *std_cargo_args(path: "crates/sspur-cli")
  end

  test do
    (testpath/"hello.ssp").write <<~SSPUR
      fn main() -> Unit ! log
      = log("hello from sspur")

      test greets = 1 + 1 == 2
    SSPUR
    assert_match "hello from sspur", shell_output("#{bin}/sspur run --interp hello.ssp")
    assert_match "1 passed", shell_output("#{bin}/sspur test --interp hello.ssp")
    assert_match version.to_s, shell_output("#{bin}/sspur --version")
  end
end
