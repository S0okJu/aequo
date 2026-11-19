// Tauri's base64-wrapped minisign format, checked with Node's built-in Ed25519.
// Verify both the package and trusted comment (including the announced version).
import { createHash, createPublicKey, verify } from 'node:crypto';
import { readFileSync } from 'node:fs';
import { Buffer } from 'node:buffer';
import process from 'node:process';
import console from 'node:console';

export function verifyUpdate(bytes, signature, publicKey, version) {
  const keyLines = Buffer.from(publicKey.trim(), 'base64').toString().trim().split(/\r?\n/);
  const sigLines = Buffer.from(signature.trim(), 'base64').toString().trim().split(/\r?\n/);
  const key = Buffer.from(keyLines[1] ?? '', 'base64');
  const sig = Buffer.from(sigLines[1] ?? '', 'base64');
  if (key.length !== 42 || sig.length !== 74 || key.subarray(0, 2).toString() !== 'Ed')
    throw new Error('Invalid updater key or signature');
  if (!key.subarray(2, 10).equals(sig.subarray(2, 10))) throw new Error('Wrong signing key');
  const algorithm = sig.subarray(0, 2).toString();
  if (!['Ed', 'ED'].includes(algorithm)) throw new Error('Unsupported signature algorithm');
  const pk = createPublicKey({
    key: Buffer.concat([Buffer.from('302a300506032b6570032100', 'hex'), key.subarray(10)]),
    format: 'der',
    type: 'spki',
  });
  const message = algorithm === 'ED' ? createHash('blake2b512').update(bytes).digest() : bytes;
  if (!verify(null, message, pk, sig.subarray(10))) throw new Error('Package signature mismatch');
  const comment = sigLines[2];
  if (!comment?.startsWith('trusted comment: ')) throw new Error('Missing trusted comment');
  const trusted = comment.slice('trusted comment: '.length);
  if (
    !verify(
      null,
      Buffer.concat([sig.subarray(10), Buffer.from(trusted)]),
      pk,
      Buffer.from(sigLines[3] ?? '', 'base64'),
    )
  )
    throw new Error('Trusted comment signature mismatch');
  if (!trusted.split(/\s+/).includes(`version:${version}`))
    throw new Error('Signature does not bind the announced version');
}

if (process.argv[1]?.endsWith('verify-update.mjs')) {
  try {
    verifyUpdate(
      readFileSync(process.argv[2]),
      readFileSync(process.argv[3], 'utf8'),
      process.env.TAURI_UPDATER_PUBLIC_KEY ?? '',
      process.argv[4],
    );
  } catch (error) {
    console.error(error.message);
    process.exitCode = 1;
  }
}
