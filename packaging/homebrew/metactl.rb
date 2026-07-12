# Install from a maintained tap after it is published:
#   brew install --HEAD pylit-ai/tap/metactl
# This source-build template intentionally tracks the repository head. A tap
# maintainer should replace `head` with a versioned source URL and SHA-256 for
# stable releases.
class Metactl < Formula
  desc "Reference kernel for agent configuration surfaces"
  homepage "https://github.com/pylit-ai/metactl"
  license "Apache-2.0"
  head "https://github.com/pylit-ai/metactl.git", branch: "main"

  depends_on "rust" => :build

  def install
    system "cargo", "install", *std_cargo_args(path: "crates/metactl")
  end

  test do
    assert_match "metactl", shell_output("#{bin}/metactl version")
  end
end
