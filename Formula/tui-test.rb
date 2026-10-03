class TuiTest < Formula
  desc "Control, inspect, test, and record terminal sessions"
  homepage "https://github.com/microsoft/tui-test"
  version "0.1.0"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/microsoft/tui-test/releases/download/0.1.0/tui-test-aarch64-apple-darwin.tar.gz"
      sha256 "88e71dae1ebdead25f1a7d8a2618e39868a2fa8a4b14630d4c4cfa446af6caeb"
    end

    on_intel do
      url "https://github.com/microsoft/tui-test/releases/download/0.1.0/tui-test-x86_64-apple-darwin.tar.gz"
      sha256 "7d86bab1faa6f63a3d88f1088e636b521498016f7842e0373c6ece3801077665"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/microsoft/tui-test/releases/download/0.1.0/tui-test-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "0ca5ade3c404ea69434362d93b2827025f81fe0147751d7e0d15578844f6b683"
    end

    on_intel do
      url "https://github.com/microsoft/tui-test/releases/download/0.1.0/tui-test-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "c7096f55a1cdb58c46bc4bdb27f8b5e246f07a5324a904c5ccd1b0351df0466b"
    end
  end

  def install
    bin.install "tui-test"
  end

  test do
    system bin/"tui-test", "--version"
  end
end
