class Tuiquake < Formula
  desc "View Japanese earthquake, early warning, and tsunami information in the terminal"
  homepage "https://github.com/hirokawa-beach/TuiQuake"
  url "https://github.com/hirokawa-beach/TuiQuake.git", tag: "v0.1.0"
  license "GPL-3.0-only"

  depends_on "rust" => :build

  def install
    system "cargo", "install", *std_cargo_args
    pkgshare.install "LICENSE", "THIRD_PARTY_NOTICES.txt"
  end

  test do
    assert_match "TuiQuake #{version}", shell_output("#{bin}/tuiquake --version")
  end
end
