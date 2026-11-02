#!/usr/bin/env bash
# 端到端自检：监听 / 状态页 / 明文抓包 / HTTPS 解密 / 证书信任 / 系统代理
# 用法：scripts/verify.sh [代理地址]
set -uo pipefail

PROXY_URL="${1:-http://127.0.0.1:8899}"
PORT="${PROXY_URL##*:}"
CA="${HOME}/.packet-sniffer/certs/root-ca.pem"
pass=0
fail=0

ok()   { printf '  \033[32m✔\033[0m %s\n' "$*"; pass=$((pass + 1)); }
bad()  { printf '  \033[31m✘\033[0m %s\n' "$*"; fail=$((fail + 1)); }

# 注意：curl 默认对 localhost 直连，测试本机代理必须加 --noproxy ""
proxied() { curl -sS --noproxy "" --max-time 25 -x "$PROXY_URL" "$@"; }

echo "== 1. 引擎监听 =="
if lsof -nP -iTCP:"$PORT" -sTCP:LISTEN >/dev/null 2>&1; then
  ok "端口 $PORT 正在监听"
else
  bad "端口 $PORT 没有监听，先执行：cargo run -p desktop -- run --port $PORT"
  echo "自检中止"; exit 1
fi

echo "== 2. 状态页（不经代理会递归，必须 -x）=="
page="$(proxied "http://127.0.0.1:$PORT/" || true)"
grep -q "packet-sniffer" <<<"$page" && ok "状态页可访问" || bad "状态页异常"

echo "== 3. 明文 HTTP 抓包 =="
code="$(proxied -o /dev/null -w '%{http_code}' http://example.com/ || true)"
[ "$code" = "200" ] && ok "http://example.com -> 200" || bad "http://example.com -> ${code:-连接失败}"

echo "== 4. HTTPS MITM 解密 =="
code="$(proxied --cacert "$CA" -o /dev/null -w '%{http_code}' https://example.com/ || true)"
[ "$code" = "200" ] && ok "https://example.com -> 200（已解密）" || bad "https://example.com -> ${code:-连接失败}"
# 直接看站点证书的签发者：由本机 CA 签发才说明真的在解密
issuer="$(proxied --cacert "$CA" -o /dev/null -w '%{certs}' https://example.com/ 2>/dev/null \
  | sed -n '/BEGIN CERTIFICATE/,/END CERTIFICATE/p' \
  | openssl x509 -noout -issuer 2>/dev/null || true)"
if grep -q "Packet Sniffer Local CA" <<<"$issuer"; then
  ok "站点证书由本机 CA 动态签发：${issuer#issuer=}"
else
  bad "站点证书签发者不是本机 CA（${issuer:-取不到证书}），MITM 未生效"
fi

echo "== 5. 证书信任（macOS 系统信任库）=="
leaf_dir="$(mktemp -d)"
openssl req -new -newkey ec -pkeyopt ec_paramgen_curve:P-256 -nodes \
  -keyout "$leaf_dir/leaf.key" -subj "/CN=verify.local" -out "$leaf_dir/leaf.csr" >/dev/null 2>&1
openssl x509 -req -in "$leaf_dir/leaf.csr" -CA "$CA" \
  -CAkey "${CA%.pem}.key" -CAcreateserial -out "$leaf_dir/leaf.pem" -days 1 >/dev/null 2>&1
if security verify-cert -p ssl -c "$leaf_dir/leaf.pem" >/dev/null 2>&1; then
  ok "系统信任库接受由本机 CA 签发的证书"
else
  bad "系统不信任本机 CA，浏览器会报证书错误：cargo run -p desktop -- install-ca"
fi
rm -rf "$leaf_dir"

echo "== 6. 系统代理 =="
effective="$(scutil --proxy | awk '/HTTPSProxy|HTTPProxy/{print; exit}')"
sysproxy="$(networksetup -getsecurewebproxy Wi-Fi 2>/dev/null | awk '/Enabled|Port/{printf "%s ", $0}')"
if grep -q "8899" <<<"$sysproxy"; then
  ok "Wi-Fi 系统代理指向 $PORT"
else
  echo "  · Wi-Fi 系统代理当前：${sysproxy:-未配置}（需先关闭 MonoProxy 的「设为系统代理」再执行 system-proxy enable）"
fi
[ -n "$effective" ] && echo "  · scutil --proxy: $effective"

echo
echo "结果：$pass 项通过，$fail 项失败"
[ "$fail" -eq 0 ]
