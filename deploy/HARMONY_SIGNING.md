# HarmonyOS 应用签名与 GitHub Release 打包指南

仓库已在 [`.github/workflows/release.yml`](../.github/workflows/release.yml) 中内置 HarmonyOS 7+（API 26，`ArkUI V2` + `liborbit_ohos.so` N-API）自动构建与发布流水线，并在每次 Tag 发布（`v*`）时自动产出 `Orbit-HarmonyOS.hap`。

---

## CI 自动构建与回退策略

1. **剥离本机路径配置**：CI 构建前会自动执行 [`apps/harmony/scripts/strip-signing.py`](../apps/harmony/scripts/strip-signing.py)，临时移除 `build-profile.json5` 中的 `signingConfigs` / `signingConfig`（避免 DevEco Studio 写入的本机绝对证书路径导致云端失败）。
2. **默认未签名包回退**：当尚未配置签名 Secrets 时，流水线仍会完成 `liborbit_ohos.so`（`arm64-v8a` + `x86_64`）编译与 `hvigorw assembleHap` 打包，生成未签名的 `Orbit-HarmonyOS.hap` 上传为 Artifact 并随 Release 发布。
3. **自动云端签名（可选）**：当下方 6 项 Secrets 配置齐全后，CI 会自动调用 SDK 内置 `hap-sign-tool.jar`（`SHA256withECDSA`）完成云端签名，产出的 `Orbit-HarmonyOS.hap` 可直接通过 `hdc install` 安装到真机。

---

## 配置 HarmonyOS 签名 Secrets（可选）

在 AppGallery Connect（或 DevEco Studio 自动签名目录 `~/.ohos/config/`）准备好签名证书（`.cer`）、Profile（`.p7b`）与密钥库（`.p12`）后，写入以下 6 个仓库 Secrets：

```bash
base64 -i path/to/app.cer | tr -d '\n' | gh secret set HARMONY_SIGNING_CERT --repo <OWNER>/orbit
base64 -i path/to/profile.p7b | tr -d '\n' | gh secret set HARMONY_SIGNING_PROFILE --repo <OWNER>/orbit
base64 -i path/to/keystore.p12 | tr -d '\n' | gh secret set HARMONY_SIGNING_KEYSTORE --repo <OWNER>/orbit

gh secret set HARMONY_SIGNING_KEY_ALIAS --repo <OWNER>/orbit --body "<KEY_ALIAS>"
gh secret set HARMONY_SIGNING_KEY_PASSWORD --repo <OWNER>/orbit --body "<KEY_PASSWORD>"
gh secret set HARMONY_SIGNING_STORE_PASSWORD --repo <OWNER>/orbit --body "<KEYSTORE_PASSWORD>"
```

---

## 本地调试安装

若下载了未签名的 `Orbit-HarmonyOS.hap`，可通过以下两种方式在本机安装验证：
- **真机 / 模拟器自动化签名安装**：使用 DevEco Studio 打开 [`apps/harmony`](../apps/harmony) 并启用自动签名或手动签名后运行。
- **命令行使用本机证书签名后安装**：
  ```bash
  java -jar "$HOS_SDK_HOME/default/openharmony/toolchains/lib/hap-sign-tool.jar" sign-app \
    -mode localSign -signAlg SHA256withECDSA \
    -keyAlias <KEY_ALIAS> \
    -appCertFile <CERT.cer> -profileFile <PROFILE.p7b> -keystoreFile <KEYSTORE.p12> \
    -keyPwd <KEY_PWD> -keystorePwd <STORE_PWD> \
    -inFile Orbit-HarmonyOS.hap -outFile Orbit-HarmonyOS-signed.hap -signCode 1
  hdc install Orbit-HarmonyOS-signed.hap
  ```
