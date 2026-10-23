class TuiTest < Formula
  desc "Control, inspect, test, and record terminal sessions"
  homepage "https://github.com/microsoft/tui-test"
  version "0.1.1"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/microsoft/tui-test/releases/download/0.1.1/tui-test-aarch64-apple-darwin.tar.gz"
      sha256 "a6819562a18031b212565f771d03fe9e5ec7d4f71b389394fd909cc66cee6b81"
    end

    on_intel do
      url "https://github.com/microsoft/tui-test/releases/download/0.1.1/tui-test-x86_64-apple-darwin.tar.gz"
      sha256 "298ae3d37549d4712a7795d388f4e871f3bf8b7863372c3441787870c747408d"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/microsoft/tui-test/releases/download/0.1.1/tui-test-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "99588dd2e296f9b2685d7fb71d68a3dde3b1fed8c0e8f1af4d215538e858bd09"
    end

    on_intel do
      url "https://github.com/microsoft/tui-test/releases/download/0.1.1/tui-test-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "ae1f32afb34c713284b0637ea41d855124fe0ac298defb38a4018cd952c158ca"
    end
  end

  def install
    bin.install "tui-test"
  end

  test do
    system bin/"tui-test", "--version"
  end
end
