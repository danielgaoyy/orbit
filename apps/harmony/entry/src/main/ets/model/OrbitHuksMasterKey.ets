import { common } from '@kit.AbilityKit';
import { preferences } from '@kit.ArkData';
import { util } from '@kit.ArkTS';
import { cryptoFramework } from '@kit.CryptoArchitectureKit';
import { huks } from '@kit.UniversalKeystoreKit';
import { orbitLogInfo, orbitLogWarn } from './OrbitLogger';

const HUKS_WRAP_KEY_ALIAS: string = 'dev.orbit.harmony.db.master_wrap_key.v1';
const HUKS_AAD_TEXT: string = 'orbit-harmony-master-key-v1';
const SECURE_KEY_PREFS_NAME: string = 'orbit_harmony_master_key_v1';
const KEY_WRAPPED_ENVELOPE: string = 'wrapped_master_key_v1';
const KEY_FALLBACK_MASTER: string = 'fallback_master_key_v1';
const MASTER_KEY_BYTES: number = 32;
const GCM_NONCE_BYTES: number = 12;
const GCM_TAG_BYTES: number = 16;

interface OrbitWrappedKeyEnvelope {
  version: number;
  nonceB64: string;
  ciphertextB64: string;
  tagB64: string;
}

export interface OrbitMasterKeyResult {
  masterKeyB64: string;
  protectionMode: string; // 'huks_hardware' | 'sandbox_fallback' | 'ephemeral'
  errorNote: string;
}

function encodeUtf8Bytes(text: string): Uint8Array {
  const encoder = new util.TextEncoder();
  return encoder.encodeInto(text);
}

function encodeBase64(bytes: Uint8Array): string {
  const helper = new util.Base64Helper();
  return helper.encodeToStringSync(bytes);
}

function decodeBase64(b64: string): Uint8Array {
  const helper = new util.Base64Helper();
  return helper.decodeSync(b64);
}

function generateCsprngBytes(length: number): Uint8Array {
  try {
    if (canIUse('SystemCapability.Security.CryptoFramework.Rand')) {
      const rand = cryptoFramework.createRandom();
      const blob = rand.generateRandomSync(length);
      if (blob.data.length === length) {
        return blob.data;
      }
    }
  } catch (err) {
    orbitLogWarn('OrbitHuks', 'CryptoFramework CSPRNG unavailable, falling back', err as Object);
  }
  const out = new Uint8Array(length);
  for (let i = 0; i < length; i++) {
    out[i] = Math.floor(Math.random() * 256) & 0xFF;
  }
  return out;
}

function buildGenerateKeyProperties(): Array<huks.HuksParam> {
  return [
    {
      tag: huks.HuksTag.HUKS_TAG_ALGORITHM,
      value: huks.HuksKeyAlg.HUKS_ALG_AES
    },
    {
      tag: huks.HuksTag.HUKS_TAG_KEY_SIZE,
      value: huks.HuksKeySize.HUKS_AES_KEY_SIZE_256
    },
    {
      tag: huks.HuksTag.HUKS_TAG_PURPOSE,
      value: huks.HuksKeyPurpose.HUKS_KEY_PURPOSE_ENCRYPT | huks.HuksKeyPurpose.HUKS_KEY_PURPOSE_DECRYPT
    },
    {
      tag: huks.HuksTag.HUKS_TAG_BLOCK_MODE,
      value: huks.HuksCipherMode.HUKS_MODE_GCM
    },
    {
      tag: huks.HuksTag.HUKS_TAG_PADDING,
      value: huks.HuksKeyPadding.HUKS_PADDING_NONE
    },
    {
      tag: huks.HuksTag.HUKS_TAG_AUTH_STORAGE_LEVEL,
      value: huks.HuksAuthStorageLevel.HUKS_AUTH_STORAGE_LEVEL_CE
    }
  ];
}

function buildEncryptProperties(nonce: Uint8Array, aad: Uint8Array): Array<huks.HuksParam> {
  return [
    {
      tag: huks.HuksTag.HUKS_TAG_ALGORITHM,
      value: huks.HuksKeyAlg.HUKS_ALG_AES
    },
    {
      tag: huks.HuksTag.HUKS_TAG_KEY_SIZE,
      value: huks.HuksKeySize.HUKS_AES_KEY_SIZE_256
    },
    {
      tag: huks.HuksTag.HUKS_TAG_PURPOSE,
      value: huks.HuksKeyPurpose.HUKS_KEY_PURPOSE_ENCRYPT
    },
    {
      tag: huks.HuksTag.HUKS_TAG_BLOCK_MODE,
      value: huks.HuksCipherMode.HUKS_MODE_GCM
    },
    {
      tag: huks.HuksTag.HUKS_TAG_PADDING,
      value: huks.HuksKeyPadding.HUKS_PADDING_NONE
    },
    {
      tag: huks.HuksTag.HUKS_TAG_NONCE,
      value: nonce
    },
    {
      tag: huks.HuksTag.HUKS_TAG_ASSOCIATED_DATA,
      value: aad
    },
    {
      tag: huks.HuksTag.HUKS_TAG_AUTH_STORAGE_LEVEL,
      value: huks.HuksAuthStorageLevel.HUKS_AUTH_STORAGE_LEVEL_CE
    }
  ];
}

function buildDecryptProperties(
  nonce: Uint8Array,
  aad: Uint8Array,
  aeTag: Uint8Array
): Array<huks.HuksParam> {
  return [
    {
      tag: huks.HuksTag.HUKS_TAG_ALGORITHM,
      value: huks.HuksKeyAlg.HUKS_ALG_AES
    },
    {
      tag: huks.HuksTag.HUKS_TAG_KEY_SIZE,
      value: huks.HuksKeySize.HUKS_AES_KEY_SIZE_256
    },
    {
      tag: huks.HuksTag.HUKS_TAG_PURPOSE,
      value: huks.HuksKeyPurpose.HUKS_KEY_PURPOSE_DECRYPT
    },
    {
      tag: huks.HuksTag.HUKS_TAG_BLOCK_MODE,
      value: huks.HuksCipherMode.HUKS_MODE_GCM
    },
    {
      tag: huks.HuksTag.HUKS_TAG_PADDING,
      value: huks.HuksKeyPadding.HUKS_PADDING_NONE
    },
    {
      tag: huks.HuksTag.HUKS_TAG_NONCE,
      value: nonce
    },
    {
      tag: huks.HuksTag.HUKS_TAG_ASSOCIATED_DATA,
      value: aad
    },
    {
      tag: huks.HuksTag.HUKS_TAG_AE_TAG,
      value: aeTag
    },
    {
      tag: huks.HuksTag.HUKS_TAG_AUTH_STORAGE_LEVEL,
      value: huks.HuksAuthStorageLevel.HUKS_AUTH_STORAGE_LEVEL_CE
    }
  ];
}

async function ensureHuksWrapKey(): Promise<void> {
  const probeOpts: huks.HuksOptions = {
    properties: [
      {
        tag: huks.HuksTag.HUKS_TAG_AUTH_STORAGE_LEVEL,
        value: huks.HuksAuthStorageLevel.HUKS_AUTH_STORAGE_LEVEL_CE
      }
    ]
  };
  let exists = false;
  try {
    exists = await huks.isKeyItemExist(HUKS_WRAP_KEY_ALIAS, probeOpts);
  } catch (_e) {
    exists = false;
  }
  if (!exists) {
    const genOpts: huks.HuksOptions = {
      properties: buildGenerateKeyProperties()
    };
    await huks.generateKeyItem(HUKS_WRAP_KEY_ALIAS, genOpts);
    orbitLogInfo('OrbitHuks', 'Generated hardware-backed AES-256 HUKS wrapping key');
  }
}

async function wrapMasterKeyWithHuks(rawKey: Uint8Array): Promise<OrbitWrappedKeyEnvelope> {
  const nonce = generateCsprngBytes(GCM_NONCE_BYTES);
  const aad = encodeUtf8Bytes(HUKS_AAD_TEXT);
  const props = buildEncryptProperties(nonce, aad);
  const initOpts: huks.HuksOptions = { properties: props };
  const session = await huks.initSession(HUKS_WRAP_KEY_ALIAS, initOpts);
  try {
    const finishOpts: huks.HuksOptions = {
      properties: props,
      inData: rawKey
    };
    const res = await huks.finishSession(session.handle, finishOpts);
    const out = res.outData;
    if (out === undefined || out.length <= GCM_TAG_BYTES) {
      throw new Error('HUKS AES-GCM wrap returned empty output');
    }
    const cipherLen = out.length - GCM_TAG_BYTES;
    const ciphertext = out.slice(0, cipherLen);
    const aeTag = out.slice(cipherLen);
    return {
      version: 1,
      nonceB64: encodeBase64(nonce),
      ciphertextB64: encodeBase64(ciphertext),
      tagB64: encodeBase64(aeTag)
    };
  } catch (err) {
    try {
      await huks.abortSession(session.handle, { properties: props });
    } catch (_abortErr) {
      // Best-effort session cleanup
    }
    throw err as Error;
  }
}

async function unwrapMasterKeyWithHuks(envelope: OrbitWrappedKeyEnvelope): Promise<Uint8Array> {
  const nonce = decodeBase64(envelope.nonceB64);
  const ciphertext = decodeBase64(envelope.ciphertextB64);
  const aeTag = decodeBase64(envelope.tagB64);
  const aad = encodeUtf8Bytes(HUKS_AAD_TEXT);
  const props = buildDecryptProperties(nonce, aad, aeTag);
  const initOpts: huks.HuksOptions = { properties: props };
  const session = await huks.initSession(HUKS_WRAP_KEY_ALIAS, initOpts);
  try {
    const finishOpts: huks.HuksOptions = {
      properties: props,
      inData: ciphertext
    };
    const res = await huks.finishSession(session.handle, finishOpts);
    const out = res.outData;
    if (out === undefined || out.length !== MASTER_KEY_BYTES) {
      throw new Error('HUKS unwrapped key length mismatch');
    }
    return out;
  } catch (err) {
    try {
      await huks.abortSession(session.handle, { properties: props });
    } catch (_abortErr) {
      // Best-effort session cleanup
    }
    throw err as Error;
  }
}

function parseEnvelope(rawJson: string): OrbitWrappedKeyEnvelope | null {
  try {
    const parsed = JSON.parse(rawJson) as OrbitWrappedKeyEnvelope;
    if (
      parsed.version === 1 &&
      typeof parsed.nonceB64 === 'string' &&
      parsed.nonceB64.length > 0 &&
      typeof parsed.ciphertextB64 === 'string' &&
      parsed.ciphertextB64.length > 0 &&
      typeof parsed.tagB64 === 'string' &&
      parsed.tagB64.length > 0
    ) {
      return parsed;
    }
  } catch (_e) {
    return null;
  }
  return null;
}

export class OrbitHuksMasterKey {
  public static async resolveOrCreateMasterKey(
    context: common.Context | undefined
  ): Promise<OrbitMasterKeyResult> {
    if (context === undefined) {
      const ephemeral = encodeBase64(generateCsprngBytes(MASTER_KEY_BYTES));
      return {
        masterKeyB64: ephemeral,
        protectionMode: 'ephemeral',
        errorNote: 'No ability context; using ephemeral preview key'
      };
    }

    let keyPrefs: preferences.Preferences | null = null;
    try {
      keyPrefs = preferences.getPreferencesSync(context, { name: SECURE_KEY_PREFS_NAME });
    } catch (err) {
      orbitLogWarn('OrbitHuks', 'Preferences unavailable for key storage', err as Object);
    }

    // 1. Primary path: Hardware-backed HUKS AES-256-GCM wrapping key
    if (canIUse('SystemCapability.Security.Huks.Core') && keyPrefs !== null) {
      try {
        await ensureHuksWrapKey();
        const existingJson = keyPrefs.getSync(KEY_WRAPPED_ENVELOPE, '') as string;
        if (typeof existingJson === 'string' && existingJson.length > 0) {
          const envelope = parseEnvelope(existingJson);
          if (envelope !== null) {
            const rawKey = await unwrapMasterKeyWithHuks(envelope);
            return {
              masterKeyB64: encodeBase64(rawKey),
              protectionMode: 'huks_hardware',
              errorNote: ''
            };
          }
        }

        // First boot (or migrating from fallback key if present)
        let rawKey: Uint8Array;
        const legacyFallback = keyPrefs.getSync(KEY_FALLBACK_MASTER, '') as string;
        if (typeof legacyFallback === 'string' && legacyFallback.length > 0) {
          const decoded = decodeBase64(legacyFallback);
          rawKey = decoded.length === MASTER_KEY_BYTES
            ? decoded
            : generateCsprngBytes(MASTER_KEY_BYTES);
        } else {
          rawKey = generateCsprngBytes(MASTER_KEY_BYTES);
        }

        const wrapped = await wrapMasterKeyWithHuks(rawKey);
        keyPrefs.putSync(KEY_WRAPPED_ENVELOPE, JSON.stringify(wrapped));
        keyPrefs.putSync(KEY_FALLBACK_MASTER, '');
        await keyPrefs.flush();
        orbitLogInfo('OrbitHuks', 'Master key sealed with HUKS hardware AES-256-GCM');
        return {
          masterKeyB64: encodeBase64(rawKey),
          protectionMode: 'huks_hardware',
          errorNote: ''
        };
      } catch (err) {
        orbitLogWarn('OrbitHuks', 'HUKS unavailable (previewer/simulator), using sandbox key', err as Object);
      }
    }

    // 2. Clean previewer / simulator sandbox fallback (consistent across reopens)
    if (keyPrefs !== null) {
      try {
        const stored = keyPrefs.getSync(KEY_FALLBACK_MASTER, '') as string;
        if (typeof stored === 'string' && stored.length > 0) {
          const decoded = decodeBase64(stored);
          if (decoded.length === MASTER_KEY_BYTES) {
            return {
              masterKeyB64: stored,
              protectionMode: 'sandbox_fallback',
              errorNote: 'HUKS hardware keystore unavailable in this environment'
            };
          }
        }
        const freshB64 = encodeBase64(generateCsprngBytes(MASTER_KEY_BYTES));
        keyPrefs.putSync(KEY_FALLBACK_MASTER, freshB64);
        await keyPrefs.flush();
        return {
          masterKeyB64: freshB64,
          protectionMode: 'sandbox_fallback',
          errorNote: 'HUKS hardware keystore unavailable in this environment'
        };
      } catch (err) {
        orbitLogWarn('OrbitHuks', 'Fallback preferences key storage failed', err as Object);
      }
    }

    return {
      masterKeyB64: encodeBase64(generateCsprngBytes(MASTER_KEY_BYTES)),
      protectionMode: 'ephemeral',
      errorNote: 'Preview environment'
    };
  }
}
