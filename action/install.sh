#!/usr/bin/env bash
# install.sh — Download and install the relune binary from GitHub Releases.
#
# Environment variables:
#   VERSION   — Relune version to install ("latest" or a semver like "0.10.0").
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
# Download, verify, and install
# ---------------------------------------------------------------------------
archive="relune_${VERSION}_${os}_${arch}.tar.gz"
base_url="https://github.com/${REPO}/releases/download/v${VERSION}"

install_dir="${RUNNER_TOOL_CACHE}/relune/${VERSION}/${os}-${arch}"
mkdir -p "${install_dir}"

tmp=$(mktemp -d)
trap 'rm -rf "${tmp}"' EXIT

echo "Downloading ${base_url}/${archive} ..."
curl -fsSL "${base_url}/${archive}" -o "${tmp}/${archive}"
curl -fsSL "${base_url}/checksums.txt" -o "${tmp}/checksums.txt"

matches=$(awk -v a="${archive}" '$2 == a {print $1}' "${tmp}/checksums.txt")
match_count=$(printf '%s' "${matches}" | grep -c . || true)
if [[ "${match_count}" -eq 0 ]]; then
  echo "::error::Archive ${archive} not found in checksums.txt"
  exit 1
fi
if [[ "${match_count}" -gt 1 ]]; then
  echo "::error::Multiple checksum entries for ${archive} in checksums.txt"
  exit 1
fi
expected="${matches}"

if command -v sha256sum >/dev/null 2>&1; then
  actual=$(sha256sum "${tmp}/${archive}" | awk '{print $1}')
else
  actual=$(shasum -a 256 "${tmp}/${archive}" | awk '{print $1}')
fi

if [[ "${expected}" != "${actual}" ]]; then
  echo "::error::Checksum mismatch for ${archive}: expected ${expected}, got ${actual}"
  exit 1
fi
echo "Checksum verified: ${actual}"

tar -xzf "${tmp}/${archive}" -C "${install_dir}"
if [[ ! -f "${install_dir}/relune" ]]; then
  echo "::error::Archive ${archive} did not contain a 'relune' binary"
  exit 1
fi
chmod +x "${install_dir}/relune"
echo "${install_dir}" >> "${GITHUB_PATH}"

version_output=$("${install_dir}/relune" --version)
echo "${version_output}"
if [[ "${version_output}" != *"${VERSION}"* ]]; then
  echo "::error::Installed binary reports '${version_output}' but expected version ${VERSION}"
  exit 1
fi

echo "Installed relune ${VERSION} to ${install_dir}"
