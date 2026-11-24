//! Pinned weight and sidecar files. Model IDs never become user-selected paths.
use std::{
    io::ErrorKind,
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::{Context, Result, ensure};
use orbit_protocol::{VoiceModelDescriptor, VoiceModelKind};
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

struct Asset {
    file: &'static str,
    size: u64,
    hash: &'static str,
    url: &'static str,
}

struct Model {
    id: &'static str,
    name: &'static str,
    kind: VoiceModelKind,
    primary: Asset,
    companions: &'static [Asset],
}

const MODELS: &[Model] = &[
    Model {
        id: "silero-vad-v5",
        name: "Silero VAD v5.1.2 (ONNX)",
        kind: VoiceModelKind::Vad,
        primary: Asset {
            file: "silero_vad.onnx",
            size: 2327524,
            hash: "2623a2953f6ff3d2c1e61740c6cdb7168133479b267dfef114a4a3cc5bdd788f",
            url: "https://raw.githubusercontent.com/snakers4/silero-vad/6478567951ae5c9979ad7b234185b5515f4be7a1/src/silero_vad/data/silero_vad.onnx",
        },
        companions: &[],
    },
    Model {
        id: "whisper-base-q5",
        name: "Whisper multilingual base Q5_1",
        kind: VoiceModelKind::Stt,
        primary: Asset {
            file: "ggml-base-q5_1.bin",
            size: 59707625,
            hash: "422f1ae452ade6f30a004d7e5c6a43195e4433bc370bf23fac9cc591f01a8898",
            url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/5359861c739e955e79d9a303bcbc70fb988958b1/ggml-base-q5_1.bin",
        },
        companions: &[],
    },
    Model {
        id: "sensevoice-small-int8",
        name: "SenseVoice Small INT8",
        kind: VoiceModelKind::Stt,
        primary: Asset {
            file: "model.int8.onnx",
            size: 237115547,
            hash: "12ca1a2ae7ecf3e0019ef2822307ee0b5cadc9196569e379b4c4026f8205276d",
            url: "https://huggingface.co/csukuangfj/sherpa-onnx-sense-voice-zh-en-ja-ko-yue-int8-2025-09-09/resolve/355f4d4884d8afd08aef04b9007a8556d7b463b2/model.int8.onnx",
        },
        companions: &[Asset {
            file: "tokens.txt",
            size: 315894,
            hash: "f449eb28dc567533d7fa59be34e2abca8784f771850c78a47fb731a31429a1dc",
            url: "https://huggingface.co/csukuangfj/sherpa-onnx-sense-voice-zh-en-ja-ko-yue-int8-2025-09-09/resolve/355f4d4884d8afd08aef04b9007a8556d7b463b2/tokens.txt",
        }],
    },
    Model {
        id: "kokoro-82m-q8",
        name: "Kokoro-82M INT8",
        kind: VoiceModelKind::Tts,
        primary: Asset {
            file: "kokoro-v1.0.onnx",
            // kokoro-tts uses the tokens/style/speed graph ABI of model-files-v1.0.
            size: 92_361_271,
            hash: "6e742170d309016e5891a994e1ce1559c702a2ccd0075e67ef7157974f6406cb",
            url: "https://github.com/thewh1teagle/kokoro-onnx/releases/download/model-files-v1.0/kokoro-v1.0.int8.onnx",
        },
        companions: &[Asset {
            file: "voices-v1.0.bin",
            size: 28214398,
            hash: "bca610b8308e8d99f32e6fe4197e7ec01679264efed0cac9140fe9c29f1fbf7d",
            url: "https://github.com/thewh1teagle/kokoro-onnx/releases/download/model-files-v1.0/voices-v1.0.bin",
        }],
    },
];

fn model(id: &str) -> Result<&'static Model> {
    MODELS
        .iter()
        .find(|m| m.id == id)
        .context("unknown voice model ID")
}

fn files(m: &Model) -> impl Iterator<Item = &Asset> {
    std::iter::once(&m.primary).chain(m.companions)
}

/// Require real directories for every component below the trusted Orbit home.
fn directory(path: &Path, create: bool) -> Result<bool> {
    if create
        && let Err(e) = std::fs::create_dir(path)
        && e.kind() != ErrorKind::AlreadyExists
    {
        return Err(e.into());
    }
    match std::fs::symlink_metadata(path) {
        Ok(meta) => {
            ensure!(meta.is_dir(), "unsafe voice model directory");
            Ok(true)
        }
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e.into()),
    }
}

fn verified_sync(asset: &Asset, path: &Path) -> Result<bool> {
    use std::io::Read;
    let meta = match std::fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(e) if e.kind() == ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e.into()),
    };
    ensure!(meta.is_file(), "unsafe voice model file");
    if meta.len() != asset.size {
        return Ok(false);
    }
    let mut file = std::fs::File::open(path)?.take(asset.size + 1);
    let mut buf = vec![0u8; 256 * 1024];
    let mut digest = Sha256::new();
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        digest.update(&buf[..n]);
    }
    Ok(format!("{:x}", digest.finalize()) == asset.hash)
}

async fn verified(asset: &'static Asset, path: PathBuf) -> Result<bool> {
    tokio::task::spawn_blocking(move || verified_sync(asset, &path)).await?
}

async fn stage_verified(
    dir: &Path,
    asset: &Asset,
    mut response: reqwest::Response,
) -> Result<tempfile::NamedTempFile> {
    if let Some(len) = response.content_length() {
        ensure!(len <= asset.size, "voice model exceeds manifest length");
    }
    let dir_owned = dir.to_path_buf();
    let (temp, std_file) = tokio::task::spawn_blocking(move || -> Result<_> {
        let temp = tempfile::Builder::new()
            .suffix(".part")
            .tempfile_in(&dir_owned)?;
        let std_file = temp.reopen()?;
        Ok((temp, std_file))
    })
    .await??;
    let mut writer = tokio::fs::File::from_std(std_file);
    let mut digest = Sha256::new();
    let mut size = 0;
    while let Some(chunk) = response.chunk().await? {
        size += chunk.len() as u64;
        ensure!(size <= asset.size, "voice model exceeds manifest length");
        digest.update(&chunk);
        writer.write_all(&chunk).await?;
    }
    ensure!(
        size == asset.size && format!("{:x}", digest.finalize()) == asset.hash,
        "voice model SHA-256 or length mismatch"
    );
    writer.sync_all().await?;
    drop(writer);
    Ok(temp)
}

/// Keep verified weights and companion files installed for the entire inference invocation.
pub(super) struct VerifiedModel<'a> {
    pub path: PathBuf,
    _lease: tokio::sync::RwLockReadGuard<'a, ()>,
}

pub struct VoiceModelManager {
    published: tokio::sync::RwLock<()>,
    home: PathBuf,
    mutations: tokio::sync::Mutex<()>,
    verified_cache:
        std::sync::Mutex<std::collections::HashMap<PathBuf, (u64, std::time::SystemTime)>>,
}

impl VoiceModelManager {
    pub fn new(home: &Path) -> Self {
        Self {
            published: Default::default(),
            home: home.into(),
            mutations: Default::default(),
            verified_cache: Default::default(),
        }
    }
    async fn directory(&self, m: &'static Model, create: bool) -> Result<Option<PathBuf>> {
        let home = self.home.clone();
        tokio::task::spawn_blocking(move || {
            let mut dir = std::fs::canonicalize(&home)?;
            for part in ["models", "voice", m.id] {
                dir.push(part);
                if !directory(&dir, create)? {
                    return Ok(None);
                }
            }
            Ok(Some(dir))
        })
        .await?
    }
    pub(super) async fn open_verified(&self, id: &str) -> Result<Option<VerifiedModel<'_>>> {
        let lease = self.published.read().await;
        Ok(self
            .verified_path(id, false)
            .await?
            .map(|path| VerifiedModel {
                path,
                _lease: lease,
            }))
    }

    async fn verified_path(&self, id: &str, use_cache: bool) -> Result<Option<PathBuf>> {
        let m = model(id)?;
        let Some(dir) = self.directory(m, false).await? else {
            return Ok(None);
        };
        for asset in files(m) {
            let path = dir.join(asset.file);
            if use_cache {
                let p_meta = path.clone();
                let meta_sig = tokio::task::spawn_blocking(move || {
                    let meta = std::fs::symlink_metadata(&p_meta).ok()?;
                    if !meta.is_file() {
                        return None;
                    }
                    let modified = meta.modified().ok()?;
                    Some((meta.len(), modified))
                })
                .await?;
                if let Some((len, modified)) = meta_sig
                    && len == asset.size
                    && self
                        .verified_cache
                        .lock()
                        .ok()
                        .and_then(|c| c.get(&path).copied())
                        == Some((len, modified))
                {
                    continue;
                }
            }
            if !verified(asset, path.clone()).await? {
                if let Ok(mut c) = self.verified_cache.lock() {
                    c.remove(&path);
                }
                return Ok(None);
            }
            let p_sig = path.clone();
            if let Ok(Some(sig)) = tokio::task::spawn_blocking(move || {
                let meta = std::fs::symlink_metadata(&p_sig).ok()?;
                Some((meta.len(), meta.modified().ok()?))
            })
            .await
                && let Ok(mut c) = self.verified_cache.lock()
            {
                c.insert(path, sig);
            }
        }
        Ok(Some(dir.join(m.primary.file)))
    }
    pub async fn list(&self) -> Result<Vec<VoiceModelDescriptor>> {
        let _lease = self.published.read().await;
        let mut models = Vec::new();
        for m in MODELS {
            let path = self.verified_path(m.id, true).await?;
            models.push(VoiceModelDescriptor {
                id: m.id.into(),
                name: m.name.into(),
                kind: m.kind,
                size_bytes: m.primary.size,
                sha256: m.primary.hash.into(),
                download_url: m.primary.url.into(),
                installed: path.is_some(),
                local_path: path.map(|p| p.to_string_lossy().into_owned()),
            });
        }
        Ok(models)
    }
    pub async fn download(&self, id: &str) -> Result<()> {
        let m = model(id)?;
        let _guard = self.mutations.lock().await;
        let dir = self
            .directory(m, true)
            .await?
            .context("missing voice directory")?;
        let client = reqwest::Client::builder()
            .https_only(true)
            .timeout(Duration::from_secs(300))
            .build()?;
        let mut staged = Vec::new();
        for asset in files(m) {
            let path = dir.join(asset.file);
            if verified(asset, path.clone()).await? {
                continue;
            }
            let response = client.get(asset.url).send().await?.error_for_status()?;
            let temp = stage_verified(&dir, asset, response).await?;
            staged.push((temp, path));
        }
        // Publish companions before the primary. Readers independently verify the entire set.
        let _publish = self.published.write().await;
        tokio::task::spawn_blocking(move || -> Result<()> {
            for (temp, path) in staged.into_iter().rev() {
                temp.persist(path)?;
            }
            Ok(())
        })
        .await??;
        if let Ok(mut c) = self.verified_cache.lock() {
            c.clear();
        }
        Ok(())
    }
    pub async fn delete(&self, id: &str) -> Result<()> {
        let m = model(id)?;
        let _guard = self.mutations.lock().await;
        let _publish = self.published.write().await;
        if let Some(dir) = self.directory(m, false).await? {
            for asset in files(m) {
                let p = dir.join(asset.file);
                if let Err(e) = tokio::fs::remove_file(&p).await
                    && e.kind() != ErrorKind::NotFound
                {
                    return Err(e.into());
                }
                if let Ok(mut c) = self.verified_cache.lock() {
                    c.remove(&p);
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn manifest_confinement_and_tampering() -> Result<()> {
        let home = tempfile::tempdir()?;
        let manager = VoiceModelManager::new(home.path());
        assert!(manager.download("../escape").await.is_err());
        assert!(manager.delete("/absolute").await.is_err());
        let file = home.path().join("weight");
        let asset = Asset {
            file: "weight",
            size: 3,
            hash: "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
            url: "",
        };
        let response = |s: &'static str| reqwest::Response::from(axum::http::Response::new(s));
        for bad in ["abd", "ab", "abcd"] {
            assert!(
                stage_verified(home.path(), &asset, response(bad))
                    .await
                    .is_err()
            );
            assert_eq!(std::fs::read_dir(home.path())?.count(), 0);
        }
        stage_verified(home.path(), &asset, response("abc"))
            .await?
            .persist(&file)?;
        assert!(verified_sync(&asset, &file)?);
        std::fs::write(&file, b"abd")?;
        assert!(!verified_sync(&asset, &file)?);
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(home.path(), home.path().join("models"))?;
            assert!(manager.delete("silero-vad-v5").await.is_err());
        }
        Ok(())
    }
}
