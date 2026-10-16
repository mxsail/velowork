#!/usr/bin/env bash
# ==============================================================================
# Velowork - GitHub Wiki 自动化同步发布脚本
# 将本地 docs/wiki/*.md 一键推送至 GitHub Wiki 官方仓库
# ==============================================================================

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
WIKI_SRC_DIR="${REPO_ROOT}/docs/wiki"
DEFAULT_WIKI_REMOTE="git@github.com:mxsail/velowork.wiki.git"
HTTPS_WIKI_REMOTE="https://github.com/mxsail/velowork.wiki.git"

echo "================================================================="
echo "  Velowork GitHub Wiki 自动化同步工具"
echo "================================================================="

if [ ! -d "${WIKI_SRC_DIR}" ]; then
    echo "❌ 错误：未找到源文档目录: ${WIKI_SRC_DIR}"
    exit 1
fi

WIKI_REMOTE="${1:-${DEFAULT_WIKI_REMOTE}}"

# 检查当前用户是否有 GitHub SSH 认证权限
if [[ "${WIKI_REMOTE}" == git@* ]]; then
    SSH_TEST_OUTPUT="$(ssh -T git@github.com 2>&1 || true)"
    if echo "${SSH_TEST_OUTPUT}" | grep -q "successfully authenticated"; then
        echo "✅ 已检测到 GitHub SSH 认证（用户：$(echo "${SSH_TEST_OUTPUT}" | sed -n 's/Hi \([^!]*\).*/\1/p')）"
    else
        echo "ℹ️  未检测到有效 GitHub SSH 密钥，回退为 HTTPS 协议..."
        WIKI_REMOTE="${HTTPS_WIKI_REMOTE}"
    fi
fi

TEMP_WIKI_DIR="$(mktemp -d /tmp/velowork-wiki-sync.XXXXXX)"
cleanup() {
    rm -rf "${TEMP_WIKI_DIR}"
}
trap cleanup EXIT

echo "📡 正在连接 GitHub Wiki 远端仓库: ${WIKI_REMOTE} ..."

if ! git clone --depth 1 "${WIKI_REMOTE}" "${TEMP_WIKI_DIR}" 2>/tmp/wiki_clone.log; then
    echo ""
    echo "⚠️  远端 Wiki 仓库克隆失败！通常原因："
    echo "  1. GitHub 仓库尚未完成首次 Wiki 初始化（尚未创建首个页面）；"
    echo "  2. 权限不足或网络连接异常。"
    echo ""
    echo "💡 解决方法："
    echo "  请首先在浏览器中打开: https://github.com/mxsail/velowork/wiki"
    echo "  点击网页上的 'Create the first page' 按钮并点击保存一次默认主页，"
    echo "  激活 GitHub Wiki 仓库后再执行此脚本同步！"
    echo ""
    cat /tmp/wiki_clone.log
    exit 1
fi

echo "📋 正在复制本地 docs/wiki/ 文档矩阵到发布目录..."
cp -v "${WIKI_SRC_DIR}"/*.md "${TEMP_WIKI_DIR}/"

cd "${TEMP_WIKI_DIR}"

if [ -z "$(git status --porcelain)" ]; then
    echo "✨ 远端 Wiki 已经是最新状态，无需重复发布！"
    exit 0
fi

git add .
COMMIT_MSG="docs: sync user manuals and wiki documentation [$(date '+%Y-%m-%d %H:%M:%S')]"
git commit -m "${COMMIT_MSG}"

echo "🚀 正在推送最新文档矩阵至 GitHub Wiki..."
git push origin master || git push origin main

echo ""
echo "================================================================="
echo "🎉 恭喜！Velowork 官方 Wiki 矩阵同步成功！"
echo "🌐 立即在浏览器访问查看: https://github.com/mxsail/velowork/wiki"
echo "================================================================="
