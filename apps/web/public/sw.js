const name = "orbit-pwa-BUILD_VERSION";
const files = ["BUILD_ASSETS"];
const urls = new Set(
  files.map((file) => new URL(file, self.registration.scope).href),
);
self.addEventListener("install", (event) => {
  event.waitUntil(caches.open(name).then((cache) => cache.addAll([...urls])));
});
// Activate after old tabs close, so lazy-loaded chunks and WASM stay on the same release.
self.addEventListener("activate", (event) => {
  event.waitUntil(
    caches
      .keys()
      .then((keys) =>
        Promise.all(
          keys
            .filter((key) => key.startsWith("orbit-pwa-") && key !== name)
            .map((key) => caches.delete(key)),
        ),
      )
      .then(() => self.clients.claim()),
  );
});
self.addEventListener("fetch", (event) => {
  const req = event.request;
  if (req.method !== "GET") return;
  const url = new URL(req.url);
  if (url.origin !== self.location.origin) return;
  if (req.mode === "navigate" && url.href === self.registration.scope)
    url.pathname += "index.html";
  if (!urls.has(url.href)) return;
  event.respondWith(
    caches
      .open(name)
      .then(async (cache) => (await cache.match(url.href)) ?? fetch(req)),
  );
});
