// Profiles and cached views share a non-extractable device key; the record ID is authenticated as AAD.
type Wrapped = { iv: Uint8Array<ArrayBuffer>; ciphertext: ArrayBuffer };

function result<T>(request: IDBRequest<T>): Promise<T> {
  return new Promise((resolve, reject) => {
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error);
  });
}

function committed(tx: IDBTransaction): Promise<void> {
  return new Promise((resolve, reject) => {
    tx.oncomplete = () => resolve();
    tx.onabort = tx.onerror = () =>
      reject(tx.error ?? new Error("IndexedDB transaction aborted"));
  });
}

export class WebProfileStore {
  private constructor(
    private db: IDBDatabase,
    private key: CryptoKey,
  ) {}

  static async open(): Promise<WebProfileStore> {
    const request = indexedDB.open("orbit-paired-hosts", 1);
    request.onupgradeneeded = () => {
      request.result.createObjectStore("keys");
      request.result.createObjectStore("records");
    };
    const db = await result(request);
    db.onversionchange = () => db.close();
    const read = () =>
      db.transaction("keys").objectStore("keys").get("wrapping-key");
    let key: CryptoKey | undefined = await result(read());
    if (!key) {
      const candidate = await crypto.subtle.generateKey(
        { name: "AES-GCM", length: 256 },
        false,
        ["encrypt", "decrypt"],
      );
      // Serialize simultaneous first opens from multiple tabs.
      const tx = db.transaction("keys", "readwrite");
      const done = committed(tx);
      const keys = tx.objectStore("keys");
      key = await result(keys.get("wrapping-key"));
      if (!key) {
        key = candidate;
        keys.add(key, "wrapping-key");
      }
      await done;
    }
    return new WebProfileStore(db, key);
  }

  async hosts(): Promise<string[]> {
    const keys = await result(
      this.db.transaction("records").objectStore("records").getAllKeys(),
    );
    return keys
      .filter(
        (k): k is string => typeof k === "string" && k.startsWith("profile/"),
      )
      .map((k) => k.slice(8));
  }

  async read<T>(id: string): Promise<T | null> {
    const entry: Wrapped | undefined = await result(
      this.db.transaction("records").objectStore("records").get(id),
    );
    if (!entry) return null;
    const bytes = await crypto.subtle.decrypt(
      {
        name: "AES-GCM",
        iv: entry.iv,
        additionalData: new TextEncoder().encode(id),
      },
      this.key,
      entry.ciphertext,
    );
    return JSON.parse(new TextDecoder().decode(bytes)) as T;
  }

  async write(id: string, value: unknown): Promise<void> {
    const bytes = new TextEncoder().encode(JSON.stringify(value));
    if (id.startsWith("snapshot/") && bytes.length > 2 * 1024 * 1024) return;
    const iv = crypto.getRandomValues(new Uint8Array(12));
    const ciphertext = await crypto.subtle.encrypt(
      { name: "AES-GCM", iv, additionalData: new TextEncoder().encode(id) },
      this.key,
      bytes,
    );
    const tx = this.db.transaction("records", "readwrite");
    tx.objectStore("records").put({ iv, ciphertext }, id);
    await committed(tx);
  }

  async remove(host: string): Promise<void> {
    const tx = this.db.transaction("records", "readwrite");
    for (const prefix of ["profile/", "snapshot/"])
      tx.objectStore("records").delete(prefix + host);
    await committed(tx);
  }
}
