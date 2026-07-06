#!/usr/bin/env bash
set -euo pipefail

# install.sh — GitHub Release の最新バイナリを取得して配置する（本店 macOS / 支店 Linux 両対応）。
#
# 使い方:
#   ./install.sh            # 最新版を取得（既に最新ならスキップ）
#   ./install.sh --force    # バージョンに関わらず再取得
#   DSN_INSTALL_DIR=/opt/bin ./install.sh   # 配置先を変更（既定 ~/.local/bin）
#
# 依存: curl のみ（public リポジトリなので gh 認証不要）。定期実行（systemd timer / launchd）
# から冪等に呼べる。バージョンが最新と一致していればダウンロードしない。

REPO="densuke/discord-send-notifier-rs"
BIN_NAME="discord-send-notifier"
DEST_DIR="${DSN_INSTALL_DIR:-${HOME}/.local/bin}"
DEST="${DEST_DIR}/${BIN_NAME}"

# OS/arch から Release アセット名を決める。
detect_asset() {
    local os arch
    os="$(uname -s)"
    arch="$(uname -m)"
    case "${os}/${arch}" in
        Linux/x86_64) echo "discord-send-notifier-x86_64-linux-musl" ;;
        Darwin/arm64 | Darwin/aarch64) echo "discord-send-notifier-aarch64-macos" ;;
        *)
            echo "ERROR: 未対応のプラットフォーム: ${os}/${arch}" >&2
            return 1
            ;;
    esac
}

# GitHub API から最新リリースの tag 名を取得（jq 不要）。取得不能なら空。
latest_tag() {
    curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" 2>/dev/null \
        | grep -m1 '"tag_name"' \
        | sed -E 's/.*"tag_name": *"([^"]+)".*/\1/'
}

main() {
    local force="no"
    [[ "${1:-}" == "--force" ]] && force="yes"

    local asset tag
    asset="$(detect_asset)"
    tag="$(latest_tag || true)"

    # 更新要否判定: 既存があり、--version が最新 tag と一致すればスキップ。
    if [[ "${force}" == "no" && -x "${DEST}" && -n "${tag}" ]]; then
        local current
        current="$("${DEST}" --version 2>/dev/null | awk '{print $2}')"
        if [[ -n "${current}" && "v${current}" == "${tag}" ]]; then
            echo "既に最新です (${tag})。再取得するには --force。"
            return 0
        fi
    fi

    local url tmp
    url="https://github.com/${REPO}/releases/latest/download/${asset}"
    mkdir -p "${DEST_DIR}"
    tmp="$(mktemp)"
    # shellcheck disable=SC2064
    trap "rm -f '${tmp}'" EXIT

    echo "取得中: ${url}"
    curl -fsSL "${url}" -o "${tmp}"
    install -m 0755 "${tmp}" "${DEST}"

    echo "配置完了: ${DEST}"
    "${DEST}" --version || true
    case ":${PATH}:" in
        *":${DEST_DIR}:"*) : ;;
        *) echo "注意: ${DEST_DIR} が PATH に含まれていません。PATH へ追加してください。" >&2 ;;
    esac
}

main "$@"
