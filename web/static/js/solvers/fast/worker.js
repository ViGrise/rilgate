const K = new Uint32Array([
  0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5,
  0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
  0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3,
  0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
  0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc,
  0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
  0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
  0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
  0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13,
  0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
  0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3,
  0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
  0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5,
  0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
  0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208,
  0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
]);

const encoder = typeof TextEncoder === "function" ? new TextEncoder() : null;

function utf8(input) {
  if (encoder) return encoder.encode(input);
  const bytes = [];
  for (let i = 0; i < input.length; i++) {
    let code = input.charCodeAt(i);
    if (code >= 0xd800 && code <= 0xdbff) {
      const low = input.charCodeAt(i + 1);
      if (low >= 0xdc00 && low <= 0xdfff) {
        code = 0x10000 + ((code - 0xd800) << 10) + low - 0xdc00;
        i++;
      } else { code = 0xfffd; }
    } else if (code >= 0xdc00 && code <= 0xdfff) { code = 0xfffd; }
    if (code < 0x80) bytes.push(code);
    else if (code < 0x800) bytes.push(0xc0 | (code >> 6), 0x80 | (code & 63));
    else if (code < 0x10000) bytes.push(0xe0 | (code >> 12), 0x80 | ((code >> 6) & 63), 0x80 | (code & 63));
    else bytes.push(0xf0 | (code >> 18), 0x80 | ((code >> 12) & 63), 0x80 | ((code >> 6) & 63), 0x80 | (code & 63));
  }
  return new Uint8Array(bytes);
}

function rotr(value, shift) {
  return (value >>> shift) | (value << (32 - shift));
}

function sha256(input) {
  const bytes = utf8(input);
  const bitLength = bytes.length * 8;
  const paddedLength = Math.ceil((bytes.length + 9) / 64) * 64;
  const padded = new Uint8Array(paddedLength);
  padded.set(bytes);
  padded[bytes.length] = 0x80;

  const view = new DataView(padded.buffer);
  const high = Math.floor(bitLength / 0x100000000);
  const low = bitLength >>> 0;
  view.setUint32(paddedLength - 8, high, false);
  view.setUint32(paddedLength - 4, low, false);

  let h0 = 0x6a09e667;
  let h1 = 0xbb67ae85;
  let h2 = 0x3c6ef372;
  let h3 = 0xa54ff53a;
  let h4 = 0x510e527f;
  let h5 = 0x9b05688c;
  let h6 = 0x1f83d9ab;
  let h7 = 0x5be0cd19;

  const w = new Uint32Array(64);

  for (let offset = 0; offset < paddedLength; offset += 64) {
    for (let i = 0; i < 16; i++) {
      w[i] = view.getUint32(offset + i * 4, false);
    }

    for (let i = 16; i < 64; i++) {
      const s0 =
        rotr(w[i - 15], 7) ^
        rotr(w[i - 15], 18) ^
        (w[i - 15] >>> 3);

      const s1 =
        rotr(w[i - 2], 17) ^
        rotr(w[i - 2], 19) ^
        (w[i - 2] >>> 10);

      w[i] = (
        w[i - 16] +
        s0 +
        w[i - 7] +
        s1
      ) >>> 0;
    }

    let a = h0;
    let b = h1;
    let c = h2;
    let d = h3;
    let e = h4;
    let f = h5;
    let g = h6;
    let h = h7;

    for (let i = 0; i < 64; i++) {
      const s1 = rotr(e, 6) ^ rotr(e, 11) ^ rotr(e, 25);
      const ch = (e & f) ^ (~e & g);
      const t1 = (h + s1 + ch + K[i] + w[i]) >>> 0;

      const s0 = rotr(a, 2) ^ rotr(a, 13) ^ rotr(a, 22);
      const maj = (a & b) ^ (a & c) ^ (b & c);
      const t2 = (s0 + maj) >>> 0;

      h = g;
      g = f;
      f = e;
      e = (d + t1) >>> 0;
      d = c;
      c = b;
      b = a;
      a = (t1 + t2) >>> 0;
    }

    h0 = (h0 + a) >>> 0;
    h1 = (h1 + b) >>> 0;
    h2 = (h2 + c) >>> 0;
    h3 = (h3 + d) >>> 0;
    h4 = (h4 + e) >>> 0;
    h5 = (h5 + f) >>> 0;
    h6 = (h6 + g) >>> 0;
    h7 = (h7 + h) >>> 0;
  }

  const out = new Uint8Array(32);
  const outView = new DataView(out.buffer);
  [h0, h1, h2, h3, h4, h5, h6, h7].forEach((value, index) => {
    outView.setUint32(index * 4, value, false);
  });

  return out;
}

function meetsDifficulty(hash, difficulty) {
  const fullBytes = Math.floor(difficulty / 2);
  const halfNibble = difficulty % 2 === 1;

  for (let i = 0; i < fullBytes; i++) {
    if (hash[i] !== 0) return false;
  }

  if (halfNibble && (hash[fullBytes] & 0xf0) !== 0) {
    return false;
  }

  return true;
}

function startFastWork(data, report) {
  const { challenge, difficulty, start = 0 } = data;
  let timer;
  let canceled = false;
  const cancel = () => { canceled = true; clearTimeout(timer); };
  if (typeof difficulty !== "number" || !isFinite(difficulty) || Math.floor(difficulty) !== difficulty || difficulty < 1 || difficulty > 64) {
    timer = setTimeout(() => report({ type: "error", message: "Invalid fast difficulty" }), 0);
    return cancel;
  }
  let nonce = Number(start) || 0;
  let attempts = 0;
  const startedAt = Date.now();
  function chunk() {
    const deadline = Date.now() + 8;
    while (!canceled) {
      const hash = sha256(`${challenge}${nonce}`);
      attempts++;
      if (meetsDifficulty(hash, difficulty)) {
        report({ type: "solved", nonce: String(nonce), attempts, elapsed_ms: Date.now() - startedAt });
        return;
      }
      nonce++;
      if (attempts % 128 === 0 && Date.now() >= deadline) break;
    }
    if (!canceled) {
      report({ type: "progress", attempts, elapsed_ms: Date.now() - startedAt });
      timer = setTimeout(chunk, 0);
    }
  }
  timer = setTimeout(chunk, 0);
  return cancel;
}

if (typeof window !== "undefined") {
  window.PowClient.startFastWork = startFastWork;
} else {
  let cancel;
  self.onmessage = (event) => {
    if (cancel) cancel();
    cancel = startFastWork(event.data, (message) => self.postMessage(message));
  };
}
