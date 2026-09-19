class TuiTest < Formula
  desc "Control, inspect, test, and record terminal sessions"
  homepage "https://github.com/microsoft/tui-test"
  version "0.1.0-beta.4"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/microsoft/tui-test/releases/download/0.1.0-beta.4/tui-test-aarch64-apple-darwin.tar.gz"
      sha256 "dd105ad15813aa7391c8cbb17ccd895d965f2914a057b57c1f4f49b419229160"
    end

    on_intel do
      url "https://github.com/microsoft/tui-test/releases/download/0.1.0-beta.4/tui-test-x86_64-apple-darwin.tar.gz"
      sha256 "181d72de564f29b60482607d52f4d6c0e97b3b3102844984168cf112bc30984a"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/microsoft/tui-test/releases/download/0.1.0-beta.4/tui-test-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "d93e06aaa60c7e5a1b7dab3f15dbb06f484d95ffdb6b2cc44ffb9f08f556f8da"
    end

    on_intel do
      url "https://github.com/microsoft/tui-test/releases/download/0.1.0-beta.4/tui-test-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "18579937967cc1f5880a4ae1c89fe98ea5eec5c0e6448259cfc5da83880120f3"
    end
  end

  def install
    bin.install "tui-test"
  end

  test do
    system bin/"tui-test", "--version"
  end
end
