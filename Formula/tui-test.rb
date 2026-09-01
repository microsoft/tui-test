class TuiTest < Formula
  desc "Control, inspect, test, and record terminal sessions"
  homepage "https://github.com/microsoft/tui-test"
  version "0.1.0-beta.3"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/microsoft/tui-test/releases/download/0.1.0-beta.3/tui-test-aarch64-apple-darwin.tar.gz"
      sha256 "c1902b9388d5e6c48eb6675efeca2305afd4cd6c518455f93860441f28bdcd7d"
    end

    on_intel do
      url "https://github.com/microsoft/tui-test/releases/download/0.1.0-beta.3/tui-test-x86_64-apple-darwin.tar.gz"
      sha256 "f7208c9b14d5d6a3678afbf9685c83312fb46edebe4036eb5f080325e9e7986d"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/microsoft/tui-test/releases/download/0.1.0-beta.3/tui-test-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "31261346af3542cbf425b1f661743db629b8f9daa2edeb7544dd4db4bae19e7a"
    end

    on_intel do
      url "https://github.com/microsoft/tui-test/releases/download/0.1.0-beta.3/tui-test-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "35d0cfff1b3d6cfeba3abb0eeae7537c5722c36414106ca28b142e5e48235a37"
    end
  end

  def install
    bin.install "tui-test"
  end

  test do
    system bin/"tui-test", "--version"
  end
end
