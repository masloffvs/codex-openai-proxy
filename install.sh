#!/usr/bin/env bash
set -euo pipefail

# ─── Config ───────────────────────────────────────────────────────────────────
REPO="masloffvs/codex-openai-proxy"
BINARY_NAME="codex-openai-proxy"
INSTALL_DIR="/usr/local/bin"
SERVICE_NAME="codex-layer"
SERVICE_FILE="/etc/systemd/system/${SERVICE_NAME}.service"
PORT=18080
AUTH_PATH="$HOME/.codex/auth.json"

# ─── Colors ───────────────────────────────────────────────────────────────────
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m'

info()  { echo -e "${GREEN}[INFO]${NC}  $*"; }
warn()  { echo -e "${YELLOW}[WARN]${NC}  $*"; }
error() { echo -e "${RED}[ERROR]${NC} $*"; exit 1; }

# ─── Checks ──────────────────────────────────────────────────────────────────
info "Checking prerequisites..."

[[ $EUID -eq 0 ]] || error "This script must be run as root (use sudo ./install.sh)"

command -v systemctl >/dev/null 2>&1 || error "systemd is required but not found"

# ─── Detect install strategy ─────────────────────────────────────────────────
# If we're inside the repo (Cargo.toml exists), build from source.
# Otherwise, download the latest release from GitHub.
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
if [[ -f "$SCRIPT_DIR/Cargo.toml" ]]; then
    INSTALL_MODE="source"
    info "Detected local repo at $SCRIPT_DIR — will build from source"
else
    INSTALL_MODE="release"
    info "No local repo detected — will download latest release"
fi

# ─── Resolve auth path for the real user (not root) ──────────────────────────
if [[ -n "${SUDO_USER:-}" ]]; then
    REAL_HOME=$(getent passwd "$SUDO_USER" | cut -d: -f6)
    AUTH_PATH="$REAL_HOME/.codex/auth.json"
fi

# ─── Strategy: build from source ─────────────────────────────────────────────
install_from_source() {
    command -v cargo >/dev/null 2>&1 || error "cargo (Rust toolchain) is required to build from source. Install via https://rustup.rs"

    info "Building release binary..."
    cd "$SCRIPT_DIR"

    # Build as the real user to avoid permission issues with cargo cache
    if [[ -n "${SUDO_USER:-}" ]]; then
        sudo -u "$SUDO_USER" cargo build --release
    else
        cargo build --release
    fi

    local BUILT_BINARY="$SCRIPT_DIR/target/release/$BINARY_NAME"
    [[ -f "$BUILT_BINARY" ]] || error "Build succeeded but binary not found at $BUILT_BINARY"

    info "Installing binary to $INSTALL_DIR/$BINARY_NAME..."
    install -m 755 "$BUILT_BINARY" "$INSTALL_DIR/$BINARY_NAME"

    TAG="source ($(git -C "$SCRIPT_DIR" describe --tags --always 2>/dev/null || echo "local"))"
}

# ─── Strategy: download release ──────────────────────────────────────────────
install_from_release() {
    command -v curl >/dev/null 2>&1 || error "curl is required but not installed"
    command -v tar  >/dev/null 2>&1 || error "tar is required but not installed"
    command -v jq   >/dev/null 2>&1 || { warn "jq not found, installing..."; apt-get install -y jq || pacman -S --noconfirm jq || yum install -y jq || error "Failed to install jq"; }

    # Detect architecture
    local ARCH
    ARCH=$(uname -m)
    local ASSET_PATTERN
    case "$ARCH" in
        x86_64)  ASSET_PATTERN="x86_64-unknown-linux-gnu" ;;
        aarch64) ASSET_PATTERN="aarch64-unknown-linux-gnu" ;;
        *)       error "Unsupported architecture: $ARCH" ;;
    esac
    info "Detected architecture: $ARCH ($ASSET_PATTERN)"

    info "Fetching latest release from github.com/$REPO..."
    local RELEASE_JSON
    RELEASE_JSON=$(curl -fsSL "https://api.github.com/repos/$REPO/releases/latest") \
        || error "Failed to fetch release info. Check your network and that the repo has releases."

    TAG=$(echo "$RELEASE_JSON" | jq -r '.tag_name')
    local DOWNLOAD_URL
    DOWNLOAD_URL=$(echo "$RELEASE_JSON" | jq -r ".assets[] | select(.name | contains(\"$ASSET_PATTERN\")) | .browser_download_url")

    [[ -n "$TAG" && "$TAG" != "null" ]]                    || error "Could not determine latest release tag"
    [[ -n "$DOWNLOAD_URL" && "$DOWNLOAD_URL" != "null" ]]  || error "No release asset found for $ASSET_PATTERN in $TAG"

    info "Latest release: $TAG"
    info "Download URL: $DOWNLOAD_URL"

    local TMP_DIR
    TMP_DIR=$(mktemp -d)
    trap 'rm -rf "$TMP_DIR"' EXIT

    info "Downloading $TAG..."
    curl -fsSL "$DOWNLOAD_URL" -o "$TMP_DIR/release.tar.gz"

    info "Extracting..."
    tar xzf "$TMP_DIR/release.tar.gz" -C "$TMP_DIR"

    local BINARY_PATH
    BINARY_PATH=$(find "$TMP_DIR" -name "$BINARY_NAME" -type f | head -1)
    [[ -n "$BINARY_PATH" ]] || error "Binary '$BINARY_NAME' not found in the archive"

    info "Installing binary to $INSTALL_DIR/$BINARY_NAME..."
    install -m 755 "$BINARY_PATH" "$INSTALL_DIR/$BINARY_NAME"
}

# ─── Run the chosen strategy ─────────────────────────────────────────────────
TAG="unknown"
if [[ "$INSTALL_MODE" == "source" ]]; then
    install_from_source
else
    install_from_release
fi

# Verify it runs
"$INSTALL_DIR/$BINARY_NAME" --version && info "Binary installed successfully" \
    || error "Binary installed but failed to execute"

# ─── Check auth.json ─────────────────────────────────────────────────────────
if [[ -f "$AUTH_PATH" ]]; then
    info "Auth file found: $AUTH_PATH"
    # Validate it has the expected structure
    if jq -e '.tokens.access_token' "$AUTH_PATH" >/dev/null 2>&1; then
        info "Auth file contains access_token — OK"
    elif grep -q 'OPENAI_API_KEY' /etc/environment 2>/dev/null || [[ -n "${OPENAI_API_KEY:-}" ]]; then
        warn "Auth file exists but has no access_token. OPENAI_API_KEY will be used as fallback."
    else
        warn "Auth file exists but has no access_token and OPENAI_API_KEY is not set."
        warn "The proxy may fail to authenticate. Please check $AUTH_PATH"
    fi
else
    warn "Auth file not found at $AUTH_PATH"
    if [[ -n "${OPENAI_API_KEY:-}" ]]; then
        info "OPENAI_API_KEY environment variable is set — will use that"
    else
        warn "No auth.json and no OPENAI_API_KEY found."
        warn "Create $AUTH_PATH or set OPENAI_API_KEY before starting the service."
    fi
fi

# ─── Install systemd service ─────────────────────────────────────────────────
info "Installing systemd service..."

cat > "$SERVICE_FILE" <<EOF
[Unit]
Description=Codex Layer
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
ExecStart=$INSTALL_DIR/$BINARY_NAME --port $PORT --auth-path $AUTH_PATH
Restart=on-failure
RestartSec=5
Environment=RUST_LOG=info

# Security hardening
NoNewPrivileges=true
ProtectSystem=strict
ProtectHome=read-only
PrivateTmp=true
ProtectKernelTunables=true
ProtectControlGroups=true
ReadOnlyPaths=/

[Install]
WantedBy=multi-user.target
EOF

info "Reloading systemd daemon..."
systemctl daemon-reload

info "Enabling $SERVICE_NAME service..."
systemctl enable "$SERVICE_NAME"

# ─── Stop old instance if running, start new one ─────────────────────────────
if systemctl is-active --quiet "$SERVICE_NAME"; then
    info "Stopping existing $SERVICE_NAME..."
    systemctl stop "$SERVICE_NAME"
fi

info "Starting $SERVICE_NAME..."
systemctl start "$SERVICE_NAME"

sleep 2

if systemctl is-active --quiet "$SERVICE_NAME"; then
    info "Service is running!"
else
    warn "Service may have failed to start. Checking logs..."
    journalctl -u "$SERVICE_NAME" --no-pager -n 20
fi

# ─── Summary ──────────────────────────────────────────────────────────────────
echo ""
echo -e "${GREEN}════════════════════════════════════════════════════${NC}"
echo -e "${GREEN}  Installation complete!${NC}"
echo -e "${GREEN}════════════════════════════════════════════════════${NC}"
echo ""
echo "  Binary:   $INSTALL_DIR/$BINARY_NAME ($TAG)"
echo "  Service:  $SERVICE_NAME"
echo "  Port:     $PORT"
echo "  Auth:     $AUTH_PATH"
echo ""
echo "  Useful commands:"
echo "    systemctl status  $SERVICE_NAME"
echo "    systemctl restart $SERVICE_NAME"
echo "    journalctl -fu    $SERVICE_NAME"
echo ""
echo "  Test:"
echo "    curl http://localhost:$PORT/v1/models"
echo ""
