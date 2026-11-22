class Rustfetch < Formula
  desc "fastfetch- and hyfetch-inspired system information tool written in Rust"
  homepage "https://github.com/hakergeniusz/rustfetch"
  version "0.2.6" # Keep in sync with the release tag used in the URL below.
  license "MIT"

  # CI publishes a raw universal macOS binary (arm64 + x86_64) named
  # rustfetch-macos-universal at the release tag. Replace the placeholder
  # sha256 below with `sha256sum` of that asset after the release is published.
  url "https://github.com/hakergeniusz/rustfetch/releases/download/v#{version}/rustfetch-macos-universal"
  sha256 "631205f1c888e53fdece59bbd6dd7f30e7f07b0f841fe0187a63896fec8e14f7"

  # No Rust dependency: the release archive ships a prebuilt binary.
  # Use `depends_on "rust" => :build` only if this is switched to a source build.

  def install
    chmod "+x", "rustfetch-macos-universal" # Prebuilt assets do not preserve the exec bit.
    bin.install "rustfetch-macos-universal" => "rustfetch"
  end

  test do
    assert_match(/rustfetch/, shell_output("#{bin}/rustfetch --version"))
  end
end
