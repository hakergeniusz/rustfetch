class Rustfetch < Formula
  desc "fastfetch- and hyfetch-inspired system information tool written in Rust"
  homepage "https://github.com/hakergeniusz/rustfetch"
  version "0.2.5" # Keep in sync with the release tag used in the URL below.
  license "MIT"

  # CI publishes a raw universal macOS binary (arm64 + x86_64) named
  # rustfetch-macos-universal at the release tag. Replace the placeholder
  # sha256 below with `sha256sum` of that asset after the release is published.
  url "https://github.com/hakergeniusz/rustfetch/releases/download/v#{version}/rustfetch-macos-universal"
  sha256 "REPLACE_WITH_SHA256_OF_THE_MACOS_UNIVERSAL_BINARY"

  # No Rust dependency: the release archive ships a prebuilt binary.
  # Use `depends_on "rust" => :build` only if this is switched to a source build.

  def install
    chmod "+x", "rustfetch" # Prebuilt archives do not always preserve the exec bit.
    bin.install "rustfetch-macos-universal" => "rustfetch"
  end

  test do
    assert_match(/rustfetch/, shell_output("#{bin}/rustfetch --version"))
  end
end
