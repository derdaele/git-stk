#!/bin/bash
set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Configuration
REPO="derdaele/git-stk"
BINARY_NAME="git-stk"

echo -e "${GREEN}Installing git-stk...${NC}"

# Detect OS and architecture
OS="$(uname -s)"
ARCH="$(uname -m)"

case "$OS" in
    Linux*)
        OS_TYPE="linux"
        ;;
    Darwin*)
        OS_TYPE="macos"
        ;;
    MINGW*|MSYS*|CYGWIN*)
        OS_TYPE="windows"
        BINARY_NAME="git-stk.exe"
        ;;
    *)
        echo -e "${RED}Unsupported operating system: $OS${NC}"
        exit 1
        ;;
esac

case "$ARCH" in
    x86_64|amd64)
        ARCH_TYPE="x86_64"
        ;;
    arm64|aarch64)
        ARCH_TYPE="aarch64"
        ;;
    *)
        echo -e "${RED}Unsupported architecture: $ARCH${NC}"
        exit 1
        ;;
esac

# Fetch the latest release information
echo "Fetching latest release..."
LATEST_RELEASE=$(curl -s "https://api.github.com/repos/$REPO/releases/latest")
VERSION=$(echo "$LATEST_RELEASE" | grep '"tag_name":' | sed -E 's/.*"([^"]+)".*/\1/')

if [ -z "$VERSION" ]; then
    echo -e "${RED}Failed to fetch the latest release version${NC}"
    exit 1
fi

echo "Latest version: $VERSION"

# Construct download URL based on OS and architecture
if [ "$OS_TYPE" = "windows" ]; then
    ASSET_NAME="git-stk-${VERSION}-${ARCH_TYPE}-pc-windows-msvc.zip"
else
    ASSET_NAME="git-stk-${VERSION}-${ARCH_TYPE}-unknown-${OS_TYPE}-gnu.tar.gz"
fi

DOWNLOAD_URL="https://github.com/$REPO/releases/download/$VERSION/$ASSET_NAME"

# Create temporary directory
TMP_DIR=$(mktemp -d)
trap "rm -rf $TMP_DIR" EXIT

echo "Downloading $ASSET_NAME..."
if ! curl -L -o "$TMP_DIR/$ASSET_NAME" "$DOWNLOAD_URL" 2>/dev/null; then
    echo -e "${YELLOW}Warning: Could not download pre-built binary${NC}"
    echo -e "${YELLOW}Please visit https://github.com/$REPO/releases/latest to download manually${NC}"
    exit 1
fi

# Extract the archive
echo "Extracting..."
cd "$TMP_DIR"
if [ "$OS_TYPE" = "windows" ]; then
    unzip -q "$ASSET_NAME"
else
    tar -xzf "$ASSET_NAME"
fi

# Determine installation directory
if [ -w "/usr/local/bin" ]; then
    INSTALL_DIR="/usr/local/bin"
elif [ -w "$HOME/.local/bin" ]; then
    INSTALL_DIR="$HOME/.local/bin"
    mkdir -p "$INSTALL_DIR"
else
    INSTALL_DIR="$HOME/.local/bin"
    mkdir -p "$INSTALL_DIR"
fi

# Install the binary
echo "Installing to $INSTALL_DIR..."
if [ -w "$INSTALL_DIR" ]; then
    mv "$BINARY_NAME" "$INSTALL_DIR/"
    chmod +x "$INSTALL_DIR/$BINARY_NAME"
else
    sudo mv "$BINARY_NAME" "$INSTALL_DIR/"
    sudo chmod +x "$INSTALL_DIR/$BINARY_NAME"
fi

# Verify installation
if command -v git-stk >/dev/null 2>&1; then
    echo -e "${GREEN}Successfully installed git-stk $VERSION!${NC}"
    echo ""
    echo "Run 'git-stk --help' to get started"
else
    echo -e "${YELLOW}git-stk was installed to $INSTALL_DIR${NC}"
    echo -e "${YELLOW}Make sure $INSTALL_DIR is in your PATH${NC}"
    echo ""
    echo "Add this to your shell profile:"
    echo "  export PATH=\"\$PATH:$INSTALL_DIR\""
fi
