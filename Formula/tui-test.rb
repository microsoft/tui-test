class TuiTest < Formula
  desc "Control, inspect, test, and record terminal sessions"
  homepage "https://github.com/microsoft/tui-test"
  version "0.1.0-beta.5"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/microsoft/tui-test/releases/download/0.1.0-beta.5/tui-test-aarch64-apple-darwin.tar.gz"
      sha256 "4ff9a72e891c9643ed1c126510def9a8f348aa0c20d535d7283ae831e762235a"
    end

    on_intel do
      url "https://github.com/microsoft/tui-test/releases/download/0.1.0-beta.5/tui-test-x86_64-apple-darwin.tar.gz"
      sha256 "8744d5b4a34c8bd58e85352c85b9fe5421cc01da8d67c7d8609cd8c8bd9f40ef"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/microsoft/tui-test/releases/download/0.1.0-beta.5/tui-test-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "557aa41e585205c0a1d7dfbe1786842e63360d9a4ccb2bd7096f7d4af275706b"
    end

    on_intel do
      url "https://github.com/microsoft/tui-test/releases/download/0.1.0-beta.5/tui-test-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "4788539cf313fe6d30b321de5bbe7ac8b5830c84b2aa6f6815335686da19dc83"
    end
  end

  def install
    bin.install "tui-test"
  end

  test do
    system bin/"tui-test", "--version"
  end
end
