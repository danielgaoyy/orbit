# iPhone TestFlight 零钥匙串一键接入指南

仓库已内置 [`.github/workflows/testflight.yml`](../.github/workflows/testflight.yml) 云端自动签名与上传流水线。**无需在本地打开 Xcode 或手动导出 `.p12` / `.mobileprovision` 描述文件**，GitHub Actions 会通过 Apple 官方 API 自动签发证书并将构建包推送至你 iPhone 上的 **TestFlight** App。

---

## 仅需一次性配置的 3 步（约 3 分钟）

### 第 1 步：在 Apple 后台注册 Bundle ID 与 App
1. 登录 [Apple Developer Identifiers](https://developer.apple.com/account/resources/identifiers/list)，点击 `+` 新建一个 **App IDs (App)**，填写你专属的唯一 Bundle ID（例如 `com.yourname.orbit`）。
2. 打开 [App Store Connect -> 我的 App](https://appstoreconnect.apple.com/apps)，点击 `+` -> **新建 App**，平台勾选 **iOS**，套装 ID 选择刚刚创建的 Bundle ID。

### 第 2 步：创建一把 App Store Connect API 密钥
1. 打开 [App Store Connect -> 用户和访问 -> 整合 -> App Store Connect API](https://appstoreconnect.apple.com/access/integrations/api)。
2. 点击 `+` 生成新密钥（权限选 **管理员 Admin** 或 **App 管理 App Manager**）。
3. 复制页面顶部的 **Issuer ID**、该密钥的 **密钥 ID (Key ID)**，并下载 **`AuthKey_XXXXXX.p8`** 私钥文件。

### 第 3 步：在本地终端一行命令写入 GitHub Secrets 并触发推送

在装有 `gh` 的终端执行（全程加密传输至 GitHub Secrets，绝不落盘进代码库）：

```bash
# 1. 自动生成一把用于云端签名的 2048 位 RSA 私钥并存入 Secret
openssl genrsa 2048 2>/dev/null | gh secret set CERTIFICATE_PRIVATE_KEY --repo <OWNER>/orbit

# 2. 填入你的 App Store Connect API 凭据
gh secret set APP_STORE_CONNECT_ISSUER_ID --repo <OWNER>/orbit --body "<你的_ISSUER_ID>"
gh secret set APP_STORE_CONNECT_KEY_IDENTIFIER --repo <OWNER>/orbit --body "<你的_KEY_ID>"
gh secret set APP_STORE_CONNECT_PRIVATE_KEY --repo <OWNER>/orbit < ~/Downloads/AuthKey_XXXXXX.p8

# 3. 保存你的 Bundle ID 变量并一键触发云端编译上传 TestFlight！
gh variable set IOS_BUNDLE_ID --repo <OWNER>/orbit --body "com.yourname.orbit"
gh workflow run testflight.yml --repo <OWNER>/orbit -f bundle_id="com.yourname.orbit"
```

流水线跑完（约 6 分钟）后，Apple 处理 2~5 分钟，你 iPhone 上的 **TestFlight** App 就会直接收到安装推送！此后每次发布新 `v*` 标签都会自动推送新版本到你的 TestFlight。
