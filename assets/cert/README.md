# assets/cert

根证书模板目录。

默认情况下根证书与私钥由 `proxy-engine` 在首次启动时生成，并写入
`EngineConfig.ca_dir`（默认 `~/.packet-sniffer/certs`）：

- `root-ca.pem`：根证书，供用户安装到系统信任存储
- `root-ca.key`：根证书私钥（PKCS#8 PEM），**不要提交到版本库**

如果希望随应用分发固定的根证书，把 `root-ca.pem` / `root-ca.key` 放到本目录，
然后启动时用 `--ca-dir assets/cert` 指定即可。
