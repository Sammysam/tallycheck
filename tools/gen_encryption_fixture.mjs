// Encrypted-file compatibility fixture produced by the reference's own
// backend/src/common/storage/encryption.ts (AES-256-GCM, "QBE1" | IV | TAG | ciphertext).
//
//   node tools/gen_encryption_fixture.mjs <reference backend dir> <output json>
//   node tools/gen_encryption_fixture.mjs --verify <reference backend dir> <key hex> <file>
//
// The second form decrypts a file written by this project with the reference code, proving
// compatibility in the other direction.
import { readFileSync, writeFileSync } from 'node:fs';
import { stripTypeScriptTypes } from 'node:module';
import { join } from 'node:path';

async function loadReference(backend) {
  const src = readFileSync(join(backend, 'src', 'common', 'storage', 'encryption.ts'), 'utf8');
  const js = stripTypeScriptTypes(src).replace("from 'crypto'", "from 'node:crypto'");
  return import('data:text/javascript;base64,' + Buffer.from(js).toString('base64'));
}

const args = process.argv.slice(2);
if (args[0] === '--verify') {
  const [, backend, keyHex, file] = args;
  const enc = await loadReference(backend);
  const plain = enc.decryptBuffer(readFileSync(file), enc.parseKey(keyHex));
  process.stdout.write(plain.toString('utf8'));
} else {
  const [backend, out] = args;
  const enc = await loadReference(backend);
  const keyHex = '00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff';
  const key = enc.parseKey(keyHex);
  const keyB64 = key.toString('base64');
  const samples = [
    Buffer.alloc(0),
    Buffer.from('Hello from the reference app'),
    Buffer.from(Array.from({ length: 70000 }, (_, i) => i % 251)),
  ];
  const cases = samples.map((p) => ({ plain: p.toString('hex'), cipher: enc.encryptBuffer(p, key).toString('hex') }));
  writeFileSync(out, JSON.stringify({ keyHex, keyB64, base64KeyParsesTo: enc.parseKey(keyB64).toString('hex'), cases }, null, 1));
  console.log(`${cases.length} cases`);
}
