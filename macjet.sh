#!/usr/bin/env bash
#
# ╔═══════════════════════════════════════════════════╗
# ║  🔥 MACJET V3                                ║
# ║  The Ultimate Mac Developer Dashboard            ║
# ║                                                   ║
# ║  Answers: "Why does my Mac sound like a turbine?" ║
# ╚═══════════════════════════════════════════════════╝
#
# Usage:
#   ./macjet.sh              Launch (non-sudo, gracefully degraded)
#   sudo ./macjet.sh         Launch with full energy/thermal data
#   ./macjet.sh --doctor     Run diagnostics
#   ./macjet.sh --update     Update dependencies
#   ./macjet.sh --mcp        Launch MCP server for AI agents
#   ./macjet.sh --help       Show help
#

set -euo pipefail

# ─── Configuration ───────────────────────────────────
MACJET_HOME="${HOME}/.macjet"
VENV_DIR="${MACJET_HOME}/venv"
BIN_DIR="${MACJET_HOME}/bin"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
APP_DIR="${SCRIPT_DIR}/macjet"
PYTHON_MIN="3.10"

# ─── Colors ──────────────────────────────────────────
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
PURPLE='\033[0;35m'
CYAN='\033[0;36m'
BOLD='\033[1m'
DIM='\033[2m'
NC='\033[0m'

# ─── Helpers ─────────────────────────────────────────
log_info()  { echo -e "  ${BLUE}ℹ${NC}  $1"; }
log_ok()    { echo -e "  ${GREEN}✅${NC} $1"; }
log_warn()  { echo -e "  ${YELLOW}⚠${NC}  $1"; }
log_err()   { echo -e "  ${RED}❌${NC} $1"; }
log_step()  { echo -e "  ${PURPLE}▸${NC}  $1"; }

print_banner() {
    echo ""
    echo -e "${BOLD}${RED}  ╔══════════════════════════════════════════════╗${NC}"
    echo -e "${BOLD}${RED}  ║${NC}  🔥 ${BOLD}MACJET V3${NC}                           ${BOLD}${RED}║${NC}"
    echo -e "${BOLD}${RED}  ║${NC}  ${DIM}The Ultimate Mac Developer Dashboard${NC}       ${BOLD}${RED}║${NC}"
    echo -e "${BOLD}${RED}  ╚══════════════════════════════════════════════╝${NC}"
    echo ""
}

# ─── Python Detection ────────────────────────────────
find_python() {
    # Try python3 first, then python
    for cmd in python3 python; do
        if command -v "$cmd" &>/dev/null; then
            local version
            version=$("$cmd" -c "import sys; print(f'{sys.version_info.major}.{sys.version_info.minor}')" 2>/dev/null)
            if [[ -n "$version" ]]; then
                local major minor
                major=$(echo "$version" | cut -d. -f1)
                minor=$(echo "$version" | cut -d. -f2)
                if [[ "$major" -ge 3 && "$minor" -ge 10 ]]; then
                    echo "$cmd"
                    return 0
                fi
            fi
        fi
    done

    # Try Homebrew Python
    if command -v /opt/homebrew/bin/python3 &>/dev/null; then
        echo "/opt/homebrew/bin/python3"
        return 0
    fi

    return 1
}

# ─── Virtual Environment Setup ───────────────────────
setup_venv() {
    local python_cmd="$1"

    if [[ -f "${VENV_DIR}/bin/python" ]]; then
        # Check if venv python is still valid
        if "${VENV_DIR}/bin/python" -c "import sys" &>/dev/null; then
            return 0
        else
            log_warn "Existing venv is broken, recreating..."
            rm -rf "$VENV_DIR"
        fi
    fi

    log_step "Creating virtual environment..."
    "$python_cmd" -m venv "$VENV_DIR"

    log_step "Installing dependencies..."
    "${VENV_DIR}/bin/pip" install --quiet --upgrade pip
    "${VENV_DIR}/bin/pip" install --quiet \
        "textual>=8.0.0" \
        "psutil>=7.0.0" \
        "textual-plotext>=1.0.0" \
        "websockets>=13.0"

    log_ok "Dependencies installed"
}

# ─── Swift Helper Build ──────────────────────────────
build_swift_helper() {
    mkdir -p "$BIN_DIR"

    local helper_src="${APP_DIR}/native/macjet-helper.swift"
    local helper_bin="${BIN_DIR}/macjet-helper"

    if [[ ! -f "$helper_src" ]]; then
        log_warn "Swift helper source not found, skipping"
        return 0
    fi

    # Only rebuild if source is newer than binary
    if [[ -f "$helper_bin" && "$helper_bin" -nt "$helper_src" ]]; then
        return 0
    fi

    if command -v swiftc &>/dev/null; then
        log_step "Compiling Swift helper..."
        if swiftc -O \
            -framework AppKit \
            -framework ApplicationServices \
            -o "$helper_bin" \
            "$helper_src" 2>/dev/null; then
            log_ok "Swift helper compiled"
        else
            log_warn "Swift compilation failed (window context will use AppleScript)"
        fi
    else
        log_warn "Swift compiler not found (window context will use AppleScript)"
    fi
}

# ─── Doctor Mode ─────────────────────────────────────
run_doctor() {
    print_banner
    echo -e "  ${BOLD}Diagnostics Report${NC}"
    echo -e "  ─────────────────────────────────────────"
    echo ""

    # Python
    local python_cmd
    if python_cmd=$(find_python); then
        local ver
        ver=$("$python_cmd" --version 2>&1)
        log_ok "Python: $ver"
    else
        log_err "Python 3.10+ not found"
    fi

    # Venv
    if [[ -f "${VENV_DIR}/bin/python" ]]; then
        log_ok "Virtual environment: ${VENV_DIR}"

        # Check packages
        for pkg in textual psutil; do
            if "${VENV_DIR}/bin/python" -c "import $pkg; print(f'  {$pkg.__name__} {$pkg.__version__}')" 2>/dev/null; then
                log_ok "$pkg installed"
            else
                log_err "$pkg not installed"
            fi
        done
    else
        log_warn "Virtual environment not set up yet"
    fi

    # Swift
    if command -v swiftc &>/dev/null; then
        local swift_ver
        swift_ver=$(swift --version 2>&1 | head -1)
        log_ok "Swift: $swift_ver"
    else
        log_warn "Swift compiler not found"
    fi

    # Swift helper
    if [[ -x "${BIN_DIR}/macjet-helper" ]]; then
        log_ok "Swift helper: compiled"
        if "${BIN_DIR}/macjet-helper" --test 2>/dev/null | grep -q '"status"'; then
            log_ok "Swift helper: working"
        else
            log_warn "Swift helper: test failed"
        fi
    else
        log_warn "Swift helper: not compiled"
    fi

    # Accessibility
    echo ""
    log_info "Accessibility: check System Preferences → Privacy & Security → Accessibility"
    log_info "Grant access to Terminal/iTerm2 for window title detection"

    # Sudo capabilities
    echo ""
    if [[ $EUID -eq 0 ]]; then
        log_ok "Running as root: full energy/thermal data available"
    else
        log_warn "Not running as root: energy/thermal data unavailable"
        log_info "Use 'sudo ./macjet.sh' for full features"
    fi

    # powermetrics
    if [[ -x /usr/bin/powermetrics ]]; then
        log_ok "powermetrics: available"
    else
        log_err "powermetrics: not found"
    fi

    # Terminal capabilities
    echo ""
    log_info "Terminal: $TERM"
    if [[ -n "${COLORTERM:-}" ]]; then
        log_ok "True color: supported ($COLORTERM)"
    else
        log_warn "True color: unknown (set COLORTERM=truecolor for best results)"
    fi

    echo ""
    echo -e "  ${DIM}────────────────────────────────────────${NC}"
    echo ""
}

# ─── Update Mode ─────────────────────────────────────
run_update() {
    print_banner
    log_step "Updating dependencies..."

    if [[ ! -f "${VENV_DIR}/bin/pip" ]]; then
        log_err "Virtual environment not found. Run macjet.sh first."
        exit 1
    fi

    "${VENV_DIR}/bin/pip" install --quiet --upgrade \
        "textual>=8.0.0" \
        "psutil>=7.0.0" \
        "textual-plotext>=1.0.0" \
        "websockets>=13.0"

    log_ok "Dependencies updated"

    # Rebuild Swift helper
    build_swift_helper

    log_ok "Update complete"
}

# ─── Help ────────────────────────────────────────────
show_help() {
    print_banner
    echo -e "  ${BOLD}Usage:${NC}"
    echo -e "    ./macjet.sh              ${DIM}Launch dashboard${NC}"
    echo -e "    sudo ./macjet.sh         ${DIM}Launch with full energy data${NC}"
    echo -e "    ./macjet.sh --doctor     ${DIM}Run diagnostics${NC}"
    echo -e "    ./macjet.sh --update     ${DIM}Update dependencies${NC}"
    echo -e "    ./macjet.sh --mcp        ${DIM}Launch MCP server for AI agents${NC}"
    echo -e "    ./macjet.sh --help       ${DIM}Show this help${NC}"
    echo ""
    echo -e "  ${BOLD}Keyboard Shortcuts (in dashboard):${NC}"
    echo -e "    ${CYAN}s${NC}  Cycle sort (cpu/mem/name/pid)"
    echo -e "    ${CYAN}g${NC}  Cycle grouping (app/tree/flat)"
    echo -e "    ${CYAN}/${NC}  Filter processes"
    echo -e "    ${CYAN}k${NC}  Kill selected process"
    echo -e "    ${CYAN}p${NC}  CPU profile (sample)"
    echo -e "    ${CYAN}n${NC}  Network inspector (nettop)"
    echo -e "    ${CYAN}?${NC}  Full help and keymap"
    echo -e "    ${CYAN}q${NC}  Quit"
    echo ""
}

# ─── Main ────────────────────────────────────────────
main() {
    # Parse arguments
    case "${1:-}" in
        --doctor)
            run_doctor
            exit 0
            ;;
        --update)
            run_update
            exit 0
            ;;
        --mcp)
            # Ensure venv exists
            if [[ ! -d "${VENV_DIR}" ]]; then
                echo -e "${YELLOW}Setting up virtual environment...${NC}"
                python3 -m venv "${VENV_DIR}"
            fi
            source "${VENV_DIR}/bin/activate"
            # Install MCP deps if needed
            pip install -q "mcp[cli]>=1.0.0" "pydantic>=2.0" "psutil>=7.0.0" "websockets>=13.0" 2>/dev/null
            echo -e "${GREEN}🔌 Starting MacJet MCP server (stdio)...${NC}" >&2
            exec "${VENV_DIR}/bin/python" "${SCRIPT_DIR}/macjet_mcp.py"
            ;;
        --help|-h)
            show_help
            exit 0
            ;;
    esac

    print_banner

    # Find Python
    local python_cmd
    if ! python_cmd=$(find_python); then
        log_err "Python 3.10+ is required but not found"
        log_info "Install via: brew install python@3.12"
        exit 1
    fi

    log_ok "Python: $("$python_cmd" --version 2>&1)"

    # Setup venv + install deps
    mkdir -p "$MACJET_HOME"
    setup_venv "$python_cmd"

    # Build Swift helper (best effort)
    build_swift_helper

    # Sudo status
    echo ""
    if [[ $EUID -eq 0 ]]; then
        log_ok "Running as root — full energy/thermal monitoring enabled"
    else
        log_warn "Running without sudo — energy/thermal data unavailable"
        log_info "Use ${BOLD}sudo ./macjet.sh${NC} for full features"
    fi

    echo ""
    log_step "Launching MacJet..."
    echo ""

    # Launch the app
    cd "$SCRIPT_DIR"
    exec "${VENV_DIR}/bin/python" -m macjet
}

# Trap for clean exit
trap 'echo ""; echo -e "  ${DIM}MacJet stopped.${NC}"; exit 0' INT TERM

main "$@"
