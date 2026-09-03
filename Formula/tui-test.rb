class TuiTest < Formula
  desc "Control, inspect, test, and record terminal sessions"
  homepage "https://github.com/microsoft/tui-test"
  version "0.1.0-beta.2"
  license "MIT"

  on_macos do
    on_arm do
      url "https://github.com/microsoft/tui-test/releases/download/0.1.0-beta.2/tui-test-aarch64-apple-darwin.tar.gz"
      sha256 "a5a49eec33b8f977f3f1a782f0f8d81f0978a14e0407a6a5f15619d6ccd3c89c"
    end

    on_intel do
      url "https://github.com/microsoft/tui-test/releases/download/0.1.0-beta.2/tui-test-x86_64-apple-darwin.tar.gz"
      sha256 "16143bf7cbbc29ecf9493cea15843a13cbba56f9952e530396091de96e33c59b"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/microsoft/tui-test/releases/download/0.1.0-beta.2/tui-test-aarch64-unknown-linux-gnu.tar.gz"
      sha256 "f6cf1624cd722c36cc834b524fd50b493f66f8c4fb4a604b713fbc08e189ae3c"
    end

    on_intel do
      url "https://github.com/microsoft/tui-test/releases/download/0.1.0-beta.2/tui-test-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "e54ff48914e1d1e8250c7b9b34bd7a1e5915f802fc7492ce832554fcebf570b5"
    end
  end

  def install
    bin.install "tui-test"
  end

  test do
    system bin/"tui-test", "--version"
  end
end
