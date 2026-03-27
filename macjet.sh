#!/usr/bin/env bash

# ANSI Escape Codes for beautiful formatting
RESET="\033[0m"
BOLD="\033[1m"
DIM="\033[2m"
BLUE="\033[34m"
GREEN="\033[32m"
YELLOW="\033[33m"
CYAN="\033[36m"
RED="\033[31m"

echo -e "\n${BOLD}${BLUE}🚀 MacJet is now 100% Rust natively compiled! (v2.0.1)${RESET}\n"

# ---------------------------------------------------------
# Critical Dependency Check: Rust (Cargo)
# ---------------------------------------------------------
if ! command -v cargo &> /dev/null; then
    echo -e "${RED}❌ Fatal Error: Rust (cargo) is not installed.${RESET}"
    echo -e "   MacJet v2 is written completely in Rust and requires Cargo to compile."
    echo -e "   Please install it via: ${CYAN}curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh${RESET}\n"
    exit 1
fi

# ---------------------------------------------------------
# Optional Developer Dependency Checks
# ---------------------------------------------------------
MISSING_OPTIONAL=0

# Check Homebrew
if ! command -v brew &> /dev/null; then
    echo -e "${YELLOW}⚠️  Homebrew is not installed.${RESET}"
    echo -e "   We recommend Homebrew for installing developer tools on macOS."
    echo -e "   Install it via: ${CYAN}/bin/bash -c \"\$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)\"${RESET}"
    echo ""
    MISSING_OPTIONAL=1
fi

# Check VHS (for generating GIFs/screenshots)
if ! command -v vhs &> /dev/null; then
    echo -e "${YELLOW}🎥 VHS is not installed.${RESET}"
    echo -e "   Want to automatically generate your own UI screenshots or HD demo GIFs locally?"
    if command -v brew &> /dev/null; then
        echo -e "   Install it easily via: ${CYAN}brew install vhs${RESET}"
    else
        echo -e "   Install Homebrew first, then run: ${CYAN}brew install vhs${RESET}"
    fi
    echo ""
    MISSING_OPTIONAL=1
fi

# ---------------------------------------------------------
# Countdown (Give them time to read)
# ---------------------------------------------------------
if [ $MISSING_OPTIONAL -eq 1 ]; then
    echo -e "${DIM}Note: MacJet will continue starting anyway. You don't need these to run the actual app.${RESET}"
    echo -e -n "\nStarting MacJet in "
    for i in {4..1}; do
        echo -e -n "${BOLD}${CYAN}$i... ${RESET}"
        sleep 1
    done
    echo -e "\n"
else
    echo -e "${GREEN}✅ All optional developer dependencies (like vhs) are installed.${RESET}"
    echo -e "${DIM}Starting MacJet...${RESET}\n"
    sleep 1
fi

# ---------------------------------------------------------
# Launch
# ---------------------------------------------------------
echo -e "${DIM}Tip: For the fastest execution moving forward, bypass this wrapper and use Cargo directly:${RESET}"
echo -e "${CYAN}cargo run --release -- \$@${RESET}\n"

cargo run --release -- "$@"
