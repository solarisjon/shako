# typed: false
# frozen_string_literal: true

# Formula for the shako shell.
# This file is auto-updated by the release workflow.
# For the canonical tap repo, see: https://github.com/solarisjon/homebrew-shako
class Shako < Formula
  desc "Fish-inspired shell with transparent AI command translation"
  homepage "https://github.com/solarisjon/shako"
  version "1.0.0"
  license "Apache-2.0"

  on_macos do
    on_arm do
      url "https://github.com/solarisjon/shako/releases/download/v#{version}/shako-v#{version}-aarch64-apple-darwin.tar.gz"
      sha256 "PLACEHOLDER_ARM64_SHA256"
    end

    on_intel do
      url "https://github.com/solarisjon/shako/releases/download/v#{version}/shako-v#{version}-x86_64-apple-darwin.tar.gz"
      sha256 "PLACEHOLDER_X86_SHA256"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/solarisjon/shako/releases/download/v#{version}/shako-v#{version}-aarch64-unknown-linux-musl.tar.gz"
      sha256 "PLACEHOLDER_LINUX_ARM64_SHA256"
    end

    on_intel do
      url "https://github.com/solarisjon/shako/releases/download/v#{version}/shako-v#{version}-x86_64-unknown-linux-musl.tar.gz"
      sha256 "PLACEHOLDER_LINUX_X86_SHA256"
    end
  end

  def install
    bin.install "shako"
  end

  def post_install
    # Remind users to add shako to /etc/shells if they want it as their login shell
    ohai "To use shako as your login shell:"
    ohai "  echo '#{HOMEBREW_PREFIX}/bin/shako' | sudo tee -a /etc/shells"
    ohai "  chsh -s #{HOMEBREW_PREFIX}/bin/shako"
  end

  def caveats
    <<~EOS
      shako requires an LLM API key for AI features. On first run, shako will
      launch an interactive setup wizard to configure your provider.

      Supported providers:
        - Anthropic Claude  (ANTHROPIC_API_KEY)
        - OpenAI            (OPENAI_API_KEY)
        - Any OpenAI-compatible endpoint

      To make shako your default login shell:
        echo '#{opt_bin}/shako' | sudo tee -a /etc/shells
        chsh -s #{opt_bin}/shako

      Configuration is stored in ~/.config/shako/config.toml
    EOS
  end

  test do
    # Verify the binary runs and reports the right version
    assert_match "shako #{version}", shell_output("#{bin}/shako --version")
    # Verify non-interactive mode works (exits cleanly after running a command)
    assert_equal 0, system(bin/"shako", "-c", "echo ok")
  end
end
