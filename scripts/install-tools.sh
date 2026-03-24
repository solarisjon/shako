#!/usr/bin/env bash
set -euo pipefail

# ── shako recommended tools installer ─────────────────────────────────
# Detects your package manager and installs the modern CLI tools that
# shako auto-detects and integrates.  Run with --all to skip prompts.

BOLD='\033[1m'
GREEN='\033[32m'
YELLOW='\033[33m'
CYAN='\033[1;36m'
DIM='\033[90m'
RESET='\033[0m'

# ── Tool definitions ──────────────────────────────────────────────────
# Format: binary|brew_name|apt_name|description|tier
TOOLS=(
  "starship|starship|starship|Cross-shell prompt with git/rust/node info|core"
  "eza|eza|eza|Modern ls with icons, git status, tree view|core"
  "bat|bat|bat|cat with syntax highlighting and line numbers|core"
  "fd|fd|fd-find|Faster find with simpler syntax|core"
  "rg|ripgrep|ripgrep|Faster grep that respects .gitignore|core"
  "zoxide|zoxide|zoxide|Smart cd that learns your habits (powers z/zi)|core"
  "fzf|fzf|fzf|Fuzzy finder for interactive selection (powers zi)|core"
  "dust|dust|dust|Visual disk usage (replaces du)|extra"
  "delta|git-delta|git-delta|Side-by-side diff with syntax highlighting|extra"
  "procs|procs|procs|Modern ps with color and search|extra"
  "sd|sd|sd|Simpler sed for find-and-replace|extra"
)

# ── Detect package manager ────────────────────────────────────────────
detect_pm() {
  if command -v brew &>/dev/null; then
    echo "brew"
  elif command -v apt &>/dev/null; then
    echo "apt"
  elif command -v dnf &>/dev/null; then
    echo "dnf"
  elif command -v pacman &>/dev/null; then
    echo "pacman"
  elif command -v apk &>/dev/null; then
    echo "apk"
  elif command -v pkg &>/dev/null; then
    echo "pkg"
  else
    echo ""
  fi
}

# Package name for a given tool + package manager
pkg_name() {
  local binary="$1" brew_name="$2" apt_name="$3" pm="$4"
  case "$pm" in
    brew)   echo "$brew_name" ;;
    apt)    echo "$apt_name" ;;
    dnf)    echo "$apt_name" ;;  # dnf names usually match apt
    pacman) echo "$brew_name" ;; # arch names usually match brew
    apk)    echo "$brew_name" ;;
    pkg)    echo "$brew_name" ;;
    *)      echo "$brew_name" ;;
  esac
}

install_cmd() {
  local pm="$1"
  case "$pm" in
    brew)   echo "brew install" ;;
    apt)    echo "sudo apt install -y" ;;
    dnf)    echo "sudo dnf install -y" ;;
    pacman) echo "sudo pacman -S --noconfirm" ;;
    apk)    echo "sudo apk add" ;;
    pkg)    echo "pkg install -y" ;;
    *)      echo "# install" ;;
  esac
}

# ── Parse args ────────────────────────────────────────────────────────
MODE="interactive"  # interactive | all | core | list
for arg in "$@"; do
  case "$arg" in
    --all)      MODE="all" ;;
    --core)     MODE="core" ;;
    --list)     MODE="list" ;;
    -h|--help)
      echo "Usage: install-tools.sh [OPTIONS]"
      echo ""
      echo "Install the modern CLI tools that shako integrates with."
      echo ""
      echo "Options:"
      echo "  --all     Install all missing tools without prompting"
      echo "  --core    Install only core tools without prompting"
      echo "  --list    Show tool status and exit"
      echo "  -h        Show this help"
      exit 0
      ;;
    *)
      echo "Unknown option: $arg (try --help)"
      exit 1
      ;;
  esac
done

# ── Main ──────────────────────────────────────────────────────────────
echo ""
echo -e " ${CYAN}shako${RESET} ${DIM}·${RESET} recommended tools installer"
echo ""

PM=$(detect_pm)
if [[ -z "$PM" ]]; then
  echo -e " ${YELLOW}No supported package manager found.${RESET}"
  echo -e " ${DIM}Supported: brew, apt, dnf, pacman, apk, pkg${RESET}"
  echo ""
  echo " Install these tools manually:"
  echo ""
  for entry in "${TOOLS[@]}"; do
    IFS='|' read -r binary _ _ desc tier <<< "$entry"
    if ! command -v "$binary" &>/dev/null; then
      printf "   %-12s %s\n" "$binary" "$desc"
    fi
  done
  echo ""
  exit 1
fi

INSTALL=$(install_cmd "$PM")

# ── Scan installed vs missing ─────────────────────────────────────────
missing_core=()
missing_extra=()
installed=()

for entry in "${TOOLS[@]}"; do
  IFS='|' read -r binary brew_name apt_name desc tier <<< "$entry"
  pkg=$(pkg_name "$binary" "$brew_name" "$apt_name" "$PM")

  if command -v "$binary" &>/dev/null; then
    installed+=("$binary")
    echo -e "   ${GREEN}✓${RESET} ${BOLD}${binary}${RESET}  ${DIM}${desc}${RESET}"
  else
    echo -e "   ${YELLOW}✗${RESET} ${BOLD}${binary}${RESET}  ${DIM}${desc}${RESET}"
    if [[ "$tier" == "core" ]]; then
      missing_core+=("$pkg")
    else
      missing_extra+=("$pkg")
    fi
  fi
done

echo ""

total_missing=$(( ${#missing_core[@]} + ${#missing_extra[@]} ))

if [[ "$total_missing" -eq 0 ]]; then
  echo -e " ${GREEN}✓ All recommended tools are installed!${RESET}"
  echo ""
  exit 0
fi

# ── List mode: just show status ───────────────────────────────────────
if [[ "$MODE" == "list" ]]; then
  if [[ ${#missing_core[@]} -gt 0 ]]; then
    echo -e " ${YELLOW}Core:${RESET}    $INSTALL ${missing_core[*]}"
  fi
  if [[ ${#missing_extra[@]} -gt 0 ]]; then
    echo -e " ${DIM}Extra:${RESET}   $INSTALL ${missing_extra[*]}"
  fi
  echo ""
  exit 0
fi

# ── Install functions ─────────────────────────────────────────────────
do_install() {
  local label="$1"
  shift
  local packages=("$@")

  if [[ ${#packages[@]} -eq 0 ]]; then
    return
  fi

  echo -e " ${CYAN}Installing ${label}:${RESET} ${packages[*]}"
  echo -e " ${DIM}→ $INSTALL ${packages[*]}${RESET}"
  echo ""

  # shellcheck disable=SC2086
  $INSTALL "${packages[@]}"
  echo ""
}

# ── Non-interactive modes ─────────────────────────────────────────────
if [[ "$MODE" == "all" ]]; then
  all_missing=("${missing_core[@]}" "${missing_extra[@]}")
  do_install "all tools" "${all_missing[@]}"

  # Post-install: init zoxide if just installed
  if command -v zoxide &>/dev/null; then
    echo -e " ${DIM}Tip: zoxide is ready — shako's z/zi builtins will use it automatically.${RESET}"
  fi
  if command -v starship &>/dev/null; then
    echo -e " ${DIM}Tip: starship is ready — shako integrates it automatically on next launch.${RESET}"
  fi
  echo ""
  echo -e " ${GREEN}✓ Done!${RESET} Restart shako to pick up the new tools."
  echo ""
  exit 0
fi

if [[ "$MODE" == "core" ]]; then
  if [[ ${#missing_core[@]} -gt 0 ]]; then
    do_install "core tools" "${missing_core[@]}"
  else
    echo -e " ${GREEN}✓ All core tools already installed.${RESET}"
  fi
  if [[ ${#missing_extra[@]} -gt 0 ]]; then
    echo -e " ${DIM}Skipped extras: ${missing_extra[*]}${RESET}"
    echo -e " ${DIM}Run with --all to install everything.${RESET}"
  fi
  echo ""
  echo -e " ${GREEN}✓ Done!${RESET} Restart shako to pick up the new tools."
  echo ""
  exit 0
fi

# ── Interactive mode ──────────────────────────────────────────────────
if [[ ${#missing_core[@]} -gt 0 ]]; then
  echo -e " ${YELLOW}${#missing_core[@]} core tools${RESET} missing: ${missing_core[*]}"
  echo -ne " Install core tools? ${DIM}[Y/n]${RESET} "
  read -r answer
  answer="${answer:-y}"
  if [[ "${answer,,}" =~ ^(y|yes)$ ]]; then
    do_install "core tools" "${missing_core[@]}"
  else
    echo -e " ${DIM}Skipped.${RESET}"
  fi
  echo ""
fi

if [[ ${#missing_extra[@]} -gt 0 ]]; then
  echo -e " ${DIM}${#missing_extra[@]} extra tools${RESET} available: ${missing_extra[*]}"
  echo -ne " Install extra tools? ${DIM}[y/N]${RESET} "
  read -r answer
  answer="${answer:-n}"
  if [[ "${answer,,}" =~ ^(y|yes)$ ]]; then
    do_install "extra tools" "${missing_extra[@]}"
  else
    echo -e " ${DIM}Skipped.${RESET}"
  fi
  echo ""
fi

# ── Post-install tips ─────────────────────────────────────────────────
echo -e " ${GREEN}✓ Done!${RESET} Restart shako to pick up the new tools."
echo ""
