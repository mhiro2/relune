#!/usr/bin/env bash
# install.sh — Download and install the relune binary from GitHub Releases.
#
# Environment variables:
#   VERSION   — Relune version to install ("latest" or a semver like "0.7.0").
#   GH_TOKEN  — GitHub token for API requests (optional, avoids rate limits).

set -euo pipefail

REPO="mhiro2/relune"

# ---------------------------------------------------------------------------
# Resolve OS / Arch to GoReleaser naming conventions
# ---------------------------------------------------------------------------
case "${RUNNER_OS}" in
  Linux)  os="linux"  ;;
  macOS)  os="darwin" ;;
  *)
    echo "::error::Unsupported runner OS: ${RUNNER_OS}"
    exit 1
    ;;
esac

case "${RUNNER_ARCH}" in
  X64)   arch="amd64" ;;
  ARM64) arch="arm64" ;;
  *)
    echo "::error::Unsupported runner architecture: ${RUNNER_ARCH}"
    exit 1
    ;;
esac

# ---------------------------------------------------------------------------
# Resolve version
# ---------------------------------------------------------------------------
if [[ "${VERSION}" == "latest" ]]; then
  api_url="https://api.github.com/repos/${REPO}/releases/latest"
  auth_header=()
  if [[ -n "${GH_TOKEN:-}" ]]; then
    auth_header=(-H "Authorization: token ${GH_TOKEN}")
  fi
  VERSION=$(curl -fsSL "${auth_header[@]}" "${api_url}" | grep '"tag_name"' | sed -E 's/.*"v?([^"]+)".*/\1/')
  if [[ -z "${VERSION}" ]]; then
    echo "::error::Failed to resolve latest relune version"
    exit 1
  fi
  echo "Resolved latest version: ${VERSION}"
fi

# Strip leading 'v' if present
VERSION="${VERSION#v}"

# ---------------------------------------------------------------------------
# Download and install
# ---------------------------------------------------------------------------
archive="relune_${VERSION}_${os}_${arch}.tar.gz"
url="https://github.com/${REPO}/releases/download/v${VERSION}/${archive}"

install_dir="${RUNNER_TOOL_CACHE}/relune/${VERSION}/${os}-${arch}"
mkdir -p "${install_dir}"

echo "Downloading ${url} ..."
curl -fsSL "${url}" -o "${install_dir}/${archive}"
tar -xzf "${install_dir}/${archive}" -C "${install_dir}"
rm -f "${install_dir}/${archive}"

chmod +x "${install_dir}/relune"
echo "${install_dir}" >> "${GITHUB_PATH}"

echo "Installed relune ${VERSION} to ${install_dir}"
