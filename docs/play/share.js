// Share links: the source, deflated and base64url-encoded, in the fragment `#code=...`.
async function pipe(bytes, stream) {
  const out = new Blob([bytes]).stream().pipeThrough(stream);
  return new Uint8Array(await new Response(out).arrayBuffer());
}

function toBase64url(bytes) {
  let s = "";
  for (let i = 0; i < bytes.length; i += 0x8000) s += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
  return btoa(s).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
}

function fromBase64url(s) {
  const b = atob(s.replace(/-/g, "+").replace(/_/g, "/"));
  return Uint8Array.from(b, (c) => c.charCodeAt(0));
}

export async function encode(text) {
  return toBase64url(await pipe(new TextEncoder().encode(text), new CompressionStream("deflate-raw")));
}

export async function decode(code) {
  return new TextDecoder().decode(await pipe(fromBase64url(code), new DecompressionStream("deflate-raw")));
}

export async function fromHash(hash) {
  const m = /(?:^#|&)code=([A-Za-z0-9_-]+)/.exec(hash);
  return m ? decode(m[1]) : null;
}

export async function inputFromHash(hash) {
  const m = /(?:^#|&)in=([A-Za-z0-9_-]+)/.exec(hash);
  return m ? decode(m[1]) : null;
}
