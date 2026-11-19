# macOS 应用签名与 Apple 公证（Notarization）配置指南

仓库已在 [`.github/workflows/release.yml`](../.github/workflows/release.yml) 与 [`apps/desktop/src-tauri/tauri.conf.json`](../apps/desktop/src-tauri/tauri.conf.json) 中内置 Tauri v2 官方 macOS **Developer ID 签名 + Hardened Runtime + Apple 公证 + `.app` / `.dmg` 双重 Staple** 流水线。

当以下 Secrets 配置完成后，GitHub Releases 产出的 `Orbit-macOS-AppleSilicon.dmg` 在任何用户的 Mac 上从浏览器下载均可直接双击安装运行，不再出现“文件已损坏，无法打开”或 Gatekeeper 拦截弹窗。

---

## 仅需一次性配置的 2 部分 Secrets

### 第 1 部分：导出 Developer ID Application 证书（必需）

> 需要已加入 **Apple Developer Program** 的账号（`.dmg` 分发必须使用 **Developer ID Application** 证书，个人免费账号或 Mac App Distribution 证书无法通过公证）。

1. 打开 Mac 上的 **钥匙串访问（Keychain Access）**（或登录 [Apple Developer Certificates](https://developer.apple.com/account/resources/certificates/list) 签发一张 **Developer ID Application** 证书并双击导入）。
2. 在“我的证书”中右键该 `Developer ID Application: <你的名称> (<TEAM_ID>)` 证书（连带展开的私钥一起，或直接选中证书导出）-> **导出** -> 格式选择 **个人信息交换 (.p12)** -> 设置导出密码。
3. 在终端将其编码为单行 Base64 并写入 GitHub Secrets（假设仓库为 `<OWNER>/orbit`）：

```bash
base64 -i ~/Desktop/DeveloperID.p12 | tr -d '\n' | gh secret set APPLE_CERTIFICATE --repo <OWNER>/orbit
gh secret set APPLE_CERTIFICATE_PASSWORD --repo <OWNER>/orbit --body "<导出_.p12_时设置的密码>"

# 可选：如果 .p12 内包含多张证书，可显式指定签名身份全名；单张证书可省略，Tauri 会自动识别
gh secret set APPLE_SIGNING_IDENTITY --repo <OWNER>/orbit --body "Developer ID Application: <你的名称> (<TEAM_ID>)"
```

---

### 第 2 部分：配置 Apple 公证凭据（二选一，推荐方案 A）

#### 方案 A：App Store Connect API Key（推荐，最稳定且不依赖双重认证）

> 如果你已经按照 [`TESTFLIGHT.md`](./TESTFLIGHT.md) 配置过 `APP_STORE_CONNECT_KEY_IDENTIFIER`、`APP_STORE_CONNECT_ISSUER_ID`、`APP_STORE_CONNECT_PRIVATE_KEY`，工作流会**自动复用这三项 Secrets**，无需重复写入。

如需为桌面端单独配置 API Key，请在 [App Store Connect -> 整合 -> App Store Connect API](https://appstoreconnect.apple.com/access/integrations/api) 创建或复用具备 **Admin / Developer / App Manager** 权限的 `.p8` 密钥后执行：

```bash
gh secret set APPLE_API_KEY --repo <OWNER>/orbit --body "<你的_KEY_ID>"
gh secret set APPLE_API_ISSUER --repo <OWNER>/orbit --body "<你的_ISSUER_ID>"
gh secret set APPLE_API_PRIVATE_KEY --repo <OWNER>/orbit < ~/Downloads/AuthKey_XXXXXX.p8
```

#### 方案 B：Apple ID + App 专用密码（备选）

若暂不使用 API Key，可在 [account.apple.com](https://account.apple.com) -> **登录与安全 -> App 专用密码** 生成一个密码后写入：

```bash
gh secret set APPLE_ID --repo <OWNER>/orbit --body "<你的_APPLE_ID_邮箱>"
gh secret set APPLE_PASSWORD --repo <OWNER>/orbit --body "<xxxx-xxxx-xxxx-xxxx>"
gh secret set APPLE_TEAM_ID --repo <OWNER>/orbit --body "<10位_TEAM_ID>"
```

---

## 触发签名与公证构建

写入 Secrets 后，直接推送新 `v*` 标签或手动触发 `release.yml` 即可：

```bash
gh workflow run release.yml --repo <OWNER>/orbit
```

CI 会在构建后自动执行以下校验（全部通过才上传产物）：
- `codesign --verify --deep --strict` 严格签名校验
- `spctl --assess --type execute` Gatekeeper 放行校验
- `xcrun notarytool submit --wait` 提交 `.app` 与 `.dmg` 公证并执行 `xcrun stapler staple` + `validate`

---

## 未配置 Secrets 时的回退行为与本地临时打开方法

- **自动回退**：当仓库尚未设置 `APPLE_CERTIFICATE` 时，CI 会自动以 `APPLE_SIGNING_IDENTITY="-"` 执行完整的 Bundle 级 Ad-Hoc 封签（绑定 `Info.plist` 与 `_CodeSignature/CodeResources`），确保普通 PR 检查与 CI 构建不会失败。
- **临时运行未公证包**：未经 Apple 公证的 `.app` 从浏览器下载后仍会带有隔离属性 `com.apple.quarantine`，需在本机执行一次解除隔离即可正常打开：

```bash
xattr -dr com.apple.quarantine /Applications/Orbit.app
```
