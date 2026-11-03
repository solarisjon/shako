# Homebrew Distribution

shako is distributed via a Homebrew tap at [solarisjon/homebrew-shako](https://github.com/solarisjon/homebrew-shako).

## Installation

```bash
brew tap solarisjon/shako
brew install shako
```

Or in one step:

```bash
brew install solarisjon/shako/shako
```

## Making shako Your Default Shell

After installing via brew:

```bash
echo "$(brew --prefix)/bin/shako" | sudo tee -a /etc/shells
chsh -s "$(brew --prefix)/bin/shako"
```

## Updating

```bash
brew update
brew upgrade shako
```

## Release Infrastructure

### How Releases Work

1. Tag a new version: `git tag v1.x.y && git push --tags`
2. The [release workflow](../.github/workflows/release.yml) fires automatically:
   - Builds binaries for macOS (Apple Silicon + Intel) and Linux (x86_64 + ARM64 musl)
   - Creates a GitHub Release with the binaries and SHA256 checksums
   - Dispatches an `update-formula` event to the `homebrew-shako` tap repo
3. The tap repo updates the formula with new version + SHAs

### Setting Up the Tap Repo

Create a new GitHub repo named `homebrew-shako` under the `solarisjon` org/account. Then:

1. Copy `Formula/shako.rb` from this repo as the starting formula
2. Create `.github/workflows/update-formula.yml` in the tap repo (see below)
3. Generate a PAT with `repo` scope on the tap repo, add it as `HOMEBREW_TAP_TOKEN` secret in **this** repo's settings

#### Tap repo update workflow (`.github/workflows/update-formula.yml`)

```yaml
name: Update Formula

on:
  repository_dispatch:
    types: [update-formula]

jobs:
  update:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - name: Update formula
        env:
          VERSION: ${{ github.event.client_payload.version }}
          ARM64_SHA: ${{ github.event.client_payload.arm64_sha }}
          X86_SHA: ${{ github.event.client_payload.x86_sha }}
        run: |
          sed -i "s/version \".*\"/version \"${VERSION#v}\"/" Formula/shako.rb
          sed -i "/aarch64-apple-darwin/{n;s/sha256 \".*\"/sha256 \"${ARM64_SHA}\"/}" Formula/shako.rb
          sed -i "/x86_64-apple-darwin/{n;s/sha256 \".*\"/sha256 \"${X86_SHA}\"/}" Formula/shako.rb

      - name: Commit and push
        run: |
          git config user.name "github-actions[bot]"
          git config user.email "github-actions[bot]@users.noreply.github.com"
          git add Formula/shako.rb
          git commit -m "chore: update formula to ${VERSION}"
          git push
        env:
          VERSION: ${{ github.event.client_payload.version }}
```

### Required GitHub Secrets

| Secret | Where | Purpose |
|--------|-------|---------|
| `HOMEBREW_TAP_TOKEN` | `solarisjon/shako` repo | PAT to dispatch events to the tap repo |
| `GITHUB_TOKEN` | auto-provided | Create releases |

### Building Locally

To test a local release build before tagging:

```bash
cargo build --release
./target/release/shako --version
```

### Supported Platforms

| Platform | Target Triple | Notes |
|----------|--------------|-------|
| macOS Apple Silicon | `aarch64-apple-darwin` | Primary target |
| macOS Intel | `x86_64-apple-darwin` | |
| Linux x86_64 | `x86_64-unknown-linux-musl` | Static binary |
| Linux ARM64 | `aarch64-unknown-linux-musl` | Static binary |
