// Vault minimalista com separação "chave pública/privada":
//
//   • Device-key (32B random, arquivo 0600) encripta o cofre. Fica em `<memDir>/.vault-devicekey`.
//     Qualquer processo local (servidor, agentes) que leia o arquivo abre o cofre.
//     O ACL por agente é feito no `getSecrets` via nomes declarados no manifest.
//
//   • Senha mestra só autentica o humano na UI — guardada como PBKDF2-SHA512 hash+salt
//     dentro do próprio cofre (plaintext JSON, campo `auth`). Não participa da encriptação.
//     Auto-lock da UI zera só a session do humano; agentes seguem trabalhando.
//
// Threat model: "se roubar o servidor, problema não é da aplicação". Encriptação em repouso
// protege só contra backup/cloud sync acidental e leitura casual — como `.env` ou
// `~/.aws/credentials`. Pra hardening real, mover device-key pro OS keychain no futuro.
//
// File layout do vault.enc (magic PMV2):
//   magic[4]=`PMV2` + saltLen[2 BE] + salt[32] + nonce[12] + ciphertext[N] + tag[16]
//   Plaintext JSON:
//     { "__version": 2,
//       "secrets":  { "KEY": "value", ... },
//       "envfiles": { "nome": { "content": "...", "updated_at": "..." }, ... },
//       "auth":     { "password_hash": "b64", "password_salt": "b64", "iters": 600000 } }
//
// Legacy (PMV1): plaintext `{ __version: 1, secrets, envfiles }` OU plano. Sem device-key.
// Migração automática no primeiro `unlock(password)`: decripta com senha (PBKDF2-derived key),
// gera device-key, grava formato PMV2 com auth block.

const fs = require('fs');
const path = require('path');
const crypto = require('crypto');

const MAGIC_V1 = Buffer.from('PMV1', 'ascii');
const MAGIC_V2 = Buffer.from('PMV2', 'ascii');
const SALT_LEN = 32;
const NONCE_LEN = 12;
const TAG_LEN = 16;
const KEY_LEN = 32;
const PW_SALT_LEN = 16;
const PW_HASH_LEN = 64;
const KDF_ITERS = 600_000;
const KDF_HASH = 'sha512';
const DEFAULT_TTL_MS = 30 * 60 * 1000;
const DEVICE_KEY_FILE = '.vault-devicekey';

// Cache in-process da device-key pra não syscall cada spawn. Resetado em destroy().
let _cachedDeviceKey = null;

// Escrita atômica com O_NOFOLLOW — previne symlink-attack (atacante que
// consegue escrever no memDir trocando arquivo-alvo por link pra /etc/x).
// Sempre escreve em tmp + rename; rename substitui entry do diretório, não
// segue symlink pré-existente.
function writeFileSafe(filePath, data, mode) {
  const m = typeof mode === 'number' ? mode : 0o600;
  const tmp = `${filePath}.tmp-${crypto.randomBytes(6).toString('hex')}`;
  const flags =
    fs.constants.O_WRONLY |
    fs.constants.O_CREAT |
    fs.constants.O_EXCL |
    (fs.constants.O_NOFOLLOW || 0);
  const fd = fs.openSync(tmp, flags, m);
  try {
    fs.writeSync(fd, typeof data === 'string' ? Buffer.from(data, 'utf8') : data);
    try { fs.fsyncSync(fd); } catch {}
  } finally {
    try { fs.closeSync(fd); } catch {}
  }
  try {
    fs.renameSync(tmp, filePath);
  } catch (e) {
    try { fs.unlinkSync(tmp); } catch {}
    throw e;
  }
}

// Session da UI humana — só marca que alguém digitou a senha. Não guarda chave.
// Expiração apenas força re-autenticação no próximo acesso CRUD pela UI.
let _uiSession = null; // { expiresAt, ttlMs }

function vaultPath(memDir) {
  return path.join(memDir, 'vault.enc');
}

function deviceKeyPath(memDir) {
  return path.join(memDir, DEVICE_KEY_FILE);
}

function isInitialized(memDir) {
  return fs.existsSync(vaultPath(memDir));
}

function hasDeviceKey(memDir) {
  return fs.existsSync(deviceKeyPath(memDir));
}

function loadDeviceKey(memDir) {
  if (_cachedDeviceKey) return _cachedDeviceKey;
  const p = deviceKeyPath(memDir);
  if (!fs.existsSync(p)) throw new Error('vault: device-key missing');
  const buf = fs.readFileSync(p);
  if (buf.length !== KEY_LEN) throw new Error('vault: device-key corrupted');
  _cachedDeviceKey = buf;
  return buf;
}

function writeDeviceKey(memDir, key) {
  writeFileSafe(deviceKeyPath(memDir), key, 0o600);
  _cachedDeviceKey = key;
}

function clearDeviceKeyCache() {
  if (_cachedDeviceKey) { try { _cachedDeviceKey.fill(0); } catch {} }
  _cachedDeviceKey = null;
}

// Garante que `<memDir>/.gitignore` lista os arquivos sensíveis do cofre.
// Idempotente: preserva linhas existentes. Funciona pra qualquer projeto —
// plugin não precisa tocar no `.gitignore` do repo do usuário.
const GITIGNORE_MARKER = '# project-mem vault — nunca commitar';
const GITIGNORE_ENTRIES = ['vault.enc', '.vault-devicekey', '.vault-unlock-state.json'];

function ensureGitignore(memDir) {
  const p = path.join(memDir, '.gitignore');
  let current = '';
  if (fs.existsSync(p)) {
    try { current = fs.readFileSync(p, 'utf8'); } catch { current = ''; }
  }
  const lines = current.split(/\r?\n/);
  const missing = GITIGNORE_ENTRIES.filter((e) => !lines.some((l) => l.trim() === e));
  if (!missing.length) return;
  const addition = (current && !current.endsWith('\n') ? '\n' : '')
    + GITIGNORE_MARKER + '\n'
    + missing.join('\n') + '\n';
  writeFileSafe(p, current + addition, 0o644);
}

function buildHeader(magic, salt) {
  const saltLenBuf = Buffer.alloc(2);
  saltLenBuf.writeUInt16BE(salt.length, 0);
  return Buffer.concat([magic, saltLenBuf, salt]);
}

function parseFile(buf) {
  if (buf.length < 4 + 2) throw new Error('vault: file too small');
  const magic = buf.slice(0, 4);
  let version;
  if (magic.equals(MAGIC_V2)) version = 2;
  else if (magic.equals(MAGIC_V1)) version = 1;
  else throw new Error('vault: bad magic');
  const saltLen = buf.readUInt16BE(4);
  const saltStart = 6;
  const saltEnd = saltStart + saltLen;
  if (buf.length < saltEnd + NONCE_LEN + TAG_LEN) throw new Error('vault: truncated');
  const salt = buf.slice(saltStart, saltEnd);
  const header = buf.slice(0, saltEnd);
  const nonce = buf.slice(saltEnd, saltEnd + NONCE_LEN);
  const tag = buf.slice(buf.length - TAG_LEN);
  const ciphertext = buf.slice(saltEnd + NONCE_LEN, buf.length - TAG_LEN);
  return { version, header, salt, nonce, ciphertext, tag };
}

function encryptWithKey(key, plaintextBuf) {
  const salt = crypto.randomBytes(SALT_LEN);
  const nonce = crypto.randomBytes(NONCE_LEN);
  const header = buildHeader(MAGIC_V2, salt);
  const cipher = crypto.createCipheriv('aes-256-gcm', key, nonce);
  cipher.setAAD(header);
  const ct = Buffer.concat([cipher.update(plaintextBuf), cipher.final()]);
  const tag = cipher.getAuthTag();
  return Buffer.concat([header, nonce, ct, tag]);
}

function decryptWithKey(file, key) {
  const { header, nonce, ciphertext, tag } = parseFile(file);
  const decipher = crypto.createDecipheriv('aes-256-gcm', key, nonce);
  decipher.setAAD(header);
  decipher.setAuthTag(tag);
  return Buffer.concat([decipher.update(ciphertext), decipher.final()]);
}

// KDF pra password hash: scrypt (memory-hard, resistente a GPU/ASIC cracking).
// Parâmetros: N=2^15, r=8, p=1 → ~32 MB RAM e ~200-400ms por hash. OWASP 2023.
// Legacy (PBKDF2-SHA512 600k) ainda aceito em verifyPassword pra cofres antigos —
// re-hash automático no próximo unlock (ver upgradeAuthIfNeeded).
const SCRYPT_N = 1 << 15;
const SCRYPT_R = 8;
const SCRYPT_P = 1;
const SCRYPT_MAXMEM = 64 * 1024 * 1024; // 64 MB — folga sobre o uso real (~32 MB)

function hashPasswordScrypt(password, salt) {
  return crypto.scryptSync(String(password), salt, PW_HASH_LEN, {
    N: SCRYPT_N, r: SCRYPT_R, p: SCRYPT_P, maxmem: SCRYPT_MAXMEM,
  });
}

function hashPasswordPBKDF2(password, salt, iters) {
  return crypto.pbkdf2Sync(String(password), salt, iters, PW_HASH_LEN, KDF_HASH);
}

function verifyPassword(password, auth) {
  if (!auth || !auth.password_hash || !auth.password_salt) return false;
  const storedHash = Buffer.from(auth.password_hash, 'base64');
  const storedSalt = Buffer.from(auth.password_salt, 'base64');
  let computed;
  if (auth.kdf === 'scrypt') {
    computed = hashPasswordScrypt(password, storedSalt);
  } else if (auth.iters) {
    // Legacy PBKDF2
    computed = hashPasswordPBKDF2(password, storedSalt, auth.iters);
  } else {
    return false;
  }
  if (computed.length !== storedHash.length) return false;
  return crypto.timingSafeEqual(computed, storedHash);
}

function buildAuthBlock(password) {
  const salt = crypto.randomBytes(PW_SALT_LEN);
  const hash = hashPasswordScrypt(password, salt);
  return {
    kdf: 'scrypt',
    password_hash: hash.toString('base64'),
    password_salt: salt.toString('base64'),
    n: SCRYPT_N, r: SCRYPT_R, p: SCRYPT_P,
  };
}

// Se o auth block é PBKDF2 (legacy), re-hash com scrypt. Transparente pro user.
function upgradeAuthIfNeeded(memDir, store, password) {
  if (store.auth && store.auth.kdf === 'scrypt') return false;
  store.auth = buildAuthBlock(password);
  saveStore(memDir, store);
  return true;
}

function normalizeStore(raw) {
  if (raw && typeof raw === 'object') {
    return {
      secrets: (raw.secrets && typeof raw.secrets === 'object') ? raw.secrets : {},
      envfiles: (raw.envfiles && typeof raw.envfiles === 'object') ? raw.envfiles : {},
      auth: (raw.auth && typeof raw.auth === 'object') ? raw.auth : null,
    };
  }
  return { secrets: {}, envfiles: {}, auth: null };
}

function loadStore(memDir) {
  const file = fs.readFileSync(vaultPath(memDir));
  const key = loadDeviceKey(memDir);
  const plain = decryptWithKey(file, key);
  return normalizeStore(JSON.parse(plain.toString('utf8') || '{}'));
}

function saveStore(memDir, store) {
  const key = loadDeviceKey(memDir);
  const wrapped = {
    __version: 2,
    secrets: store.secrets || {},
    envfiles: store.envfiles || {},
    auth: store.auth,
  };
  const file = encryptWithKey(key, Buffer.from(JSON.stringify(wrapped), 'utf8'));
  writeFileSafe(vaultPath(memDir), file, 0o600);
}

// Migra cofre PMV1 (encriptação por senha) pra PMV2 (device-key + auth block).
// Requer a senha pra decriptar uma última vez.
function migrateLegacy(memDir, legacyFile, password) {
  const { salt } = parseFile(legacyFile);
  const legacyKey = crypto.pbkdf2Sync(String(password), salt, KDF_ITERS, KEY_LEN, KDF_HASH);
  let legacyPlain;
  try {
    legacyPlain = decryptWithKey(legacyFile, legacyKey);
  } catch {
    throw new Error('vault: senha incorreta (cofre legado)');
  }
  let raw;
  try { raw = JSON.parse(legacyPlain.toString('utf8') || '{}'); }
  catch { raw = {}; }
  let legacySecrets = {};
  let legacyEnvfiles = {};
  if (raw && typeof raw === 'object') {
    if (raw.__version === 1) {
      legacySecrets = raw.secrets || {};
      legacyEnvfiles = raw.envfiles || {};
    } else {
      // pre-version: objeto plano KEY→value
      legacySecrets = raw;
    }
  }
  const deviceKey = crypto.randomBytes(KEY_LEN);
  writeDeviceKey(memDir, deviceKey);
  const newStore = {
    __version: 2,
    secrets: legacySecrets,
    envfiles: legacyEnvfiles,
    auth: buildAuthBlock(password),
  };
  const newFile = encryptWithKey(deviceKey, Buffer.from(JSON.stringify(newStore), 'utf8'));
  writeFileSafe(vaultPath(memDir), newFile, 0o600);
}

function init(memDir, password) {
  if (isInitialized(memDir)) throw new Error('vault: already initialized');
  if (!password || String(password).length < 8) {
    throw new Error('vault: password must be at least 8 chars');
  }
  ensureGitignore(memDir);
  const deviceKey = crypto.randomBytes(KEY_LEN);
  writeDeviceKey(memDir, deviceKey);
  const store = {
    __version: 2,
    secrets: {},
    envfiles: {},
    auth: buildAuthBlock(password),
  };
  const file = encryptWithKey(deviceKey, Buffer.from(JSON.stringify(store), 'utf8'));
  writeFileSafe(vaultPath(memDir), file, 0o600);
  _uiSession = { expiresAt: Date.now() + DEFAULT_TTL_MS, ttlMs: DEFAULT_TTL_MS };
  return { initialized: true, expires_at: _uiSession.expiresAt };
}

// Autentica humano e abre session de UI. Se cofre for legado (PMV1), migra em cima.
function unlock(memDir, password, ttlMs = DEFAULT_TTL_MS) {
  if (!isInitialized(memDir)) throw new Error('vault: not initialized');
  let file = fs.readFileSync(vaultPath(memDir));
  const { version } = parseFile(file);

  if (version === 1) {
    migrateLegacy(memDir, file, password);
    file = fs.readFileSync(vaultPath(memDir));
  }

  const key = loadDeviceKey(memDir);
  let plain;
  try {
    plain = decryptWithKey(file, key);
  } catch {
    throw new Error('vault: cofre corrompido ou device-key inválida');
  }
  const store = normalizeStore(JSON.parse(plain.toString('utf8') || '{}'));
  if (!store.auth) throw new Error('vault: auth block faltando');
  if (!verifyPassword(password, store.auth)) {
    throw new Error('vault: senha incorreta');
  }
  // Reaplica gitignore a cada unlock — cobre o caso de cofre criado antes
  // deste fix, ou arquivo deletado manualmente.
  ensureGitignore(memDir);
  // Upgrade auth pra scrypt se cofre vem do formato PBKDF2 legacy.
  // Primeiro precisamos de session ativa pra saveStore funcionar.
  _uiSession = { expiresAt: Date.now() + ttlMs, ttlMs };
  try { upgradeAuthIfNeeded(memDir, store, password); } catch {}
  return { expires_at: _uiSession.expiresAt };
}

function lock() {
  _uiSession = null;
}

// Troca senha sem re-encriptar secrets — só re-hash e save.
// Requer UI unlock + senha atual válida (defesa em profundidade: mesmo com UI
// aberta, humano precisa provar que conhece a senha antiga).
function changePassword(memDir, oldPassword, newPassword) {
  requireUIUnlock();
  if (!newPassword || String(newPassword).length < 8) {
    throw new Error('vault: nova senha precisa de no mínimo 8 caracteres');
  }
  const store = loadStore(memDir);
  if (!store.auth) throw new Error('vault: auth block faltando');
  if (!verifyPassword(oldPassword, store.auth)) {
    throw new Error('vault: senha atual incorreta');
  }
  store.auth = buildAuthBlock(newPassword);
  saveStore(memDir, store);
}

function isUnlocked() {
  if (!_uiSession) return false;
  if (Date.now() > _uiSession.expiresAt) { _uiSession = null; return false; }
  return true;
}

function requireUIUnlock() {
  if (!isUnlocked()) throw new Error('vault: UI locked');
}

function status(memDir) {
  const initialized = isInitialized(memDir);
  const legacy = initialized && !hasDeviceKey(memDir);
  const unlocked = isUnlocked();
  return {
    initialized,
    // "locked" aqui é sempre do ponto de vista da UI humana. Agente não vê isso.
    // Cofre legado força locked=true até migrar (user digita senha → migração roda).
    locked: !unlocked || legacy,
    expires_at: unlocked ? _uiSession.expiresAt : null,
    ttl_ms: _uiSession?.ttlMs || null,
    needs_migration: legacy,
  };
}

// ── Secrets individuais (CRUD via UI humana) ────────────────────────────────
function listKeys(memDir) {
  requireUIUnlock();
  return Object.keys(loadStore(memDir).secrets).sort();
}

function getSecret(memDir, name) {
  requireUIUnlock();
  const s = loadStore(memDir).secrets;
  return Object.prototype.hasOwnProperty.call(s, name) ? s[name] : null;
}

function setSecret(memDir, name, value) {
  requireUIUnlock();
  if (!name || typeof name !== 'string' || !/^[A-Z][A-Z0-9_]{0,63}$/.test(name)) {
    throw new Error('vault: name must match [A-Z][A-Z0-9_]{0,63}');
  }
  if (typeof value !== 'string') throw new Error('vault: value must be string');
  if (value.length > 8192) throw new Error('vault: value too long (max 8192)');
  const store = loadStore(memDir);
  store.secrets[name] = value;
  saveStore(memDir, store);
}

function deleteSecret(memDir, name) {
  requireUIUnlock();
  const store = loadStore(memDir);
  if (!Object.prototype.hasOwnProperty.call(store.secrets, name)) return false;
  delete store.secrets[name];
  saveStore(memDir, store);
  return true;
}

// ── Agentes: injeção de secrets via device-key, sem depender da UI ──────────
// Filtragem pelo manifest (`secrets: [...]`) é o ACL. Se cofre for legado (sem
// device-key), degradamos silenciosamente — agente roda sem envs e server loga.
function getSecrets(memDir, names) {
  if (!Array.isArray(names) || !names.length) return {};
  if (!isInitialized(memDir)) return {};
  if (!hasDeviceKey(memDir)) return {};
  try {
    const store = loadStore(memDir);
    const out = {};
    for (const n of names) {
      if (Object.prototype.hasOwnProperty.call(store.secrets, n)) out[n] = store.secrets[n];
    }
    return out;
  } catch {
    return {};
  }
}

// ── Envfiles (CRUD via UI humana) ───────────────────────────────────────────
const ENVFILE_NAME_RE = /^[A-Za-z0-9._\-\/]{1,120}$/;

function listEnvFiles(memDir) {
  requireUIUnlock();
  const ef = loadStore(memDir).envfiles;
  return Object.keys(ef).sort().map((name) => ({
    name,
    updated_at: ef[name]?.updated_at || null,
    size: (ef[name]?.content || '').length,
  }));
}

function getEnvFile(memDir, name) {
  requireUIUnlock();
  const entry = loadStore(memDir).envfiles[name];
  return entry ? entry.content : null;
}

function setEnvFile(memDir, name, content) {
  requireUIUnlock();
  if (!name || !ENVFILE_NAME_RE.test(name)) {
    throw new Error('vault: envfile name must match [A-Za-z0-9._-/]{1,120}');
  }
  if (typeof content !== 'string') throw new Error('vault: content must be string');
  if (content.length > 128 * 1024) throw new Error('vault: envfile too large (max 128 KB)');
  const store = loadStore(memDir);
  store.envfiles[name] = {
    content,
    updated_at: new Date().toISOString(),
  };
  saveStore(memDir, store);
}

function deleteEnvFile(memDir, name) {
  requireUIUnlock();
  const store = loadStore(memDir);
  if (!Object.prototype.hasOwnProperty.call(store.envfiles, name)) return false;
  delete store.envfiles[name];
  saveStore(memDir, store);
  return true;
}

function destroy(memDir) {
  lock();
  clearDeviceKeyCache();
  const vp = vaultPath(memDir);
  if (fs.existsSync(vp)) fs.unlinkSync(vp);
  const dp = deviceKeyPath(memDir);
  if (fs.existsSync(dp)) fs.unlinkSync(dp);
}

// ── Export / Import ─────────────────────────────────────────────────────────
// Bundle encriptado com a senha mestra (scrypt + AES-GCM). Empacota vault.enc +
// device-key num único arquivo seguro pra backup externo (cloud, pendrive).
//
// File layout (PMVEXP1):
//   magic[7]="PMVEXP1" + salt[32] + nonce[12] + ciphertext + tag[16]
//   Plaintext JSON:
//     { "__version": 1, "exported_at": iso, "vault_b64": "...", "device_key_b64": "..." }
const EXPORT_MAGIC = Buffer.from('PMVEXP1', 'ascii');

function deriveExportKey(password, salt) {
  return crypto.scryptSync(String(password), salt, KEY_LEN, {
    N: SCRYPT_N, r: SCRYPT_R, p: SCRYPT_P, maxmem: SCRYPT_MAXMEM,
  });
}

// `masterPassword` autoriza o export (sempre verificado contra auth block).
// `bundlePassword` (opcional) encripta o bundle; se ausente, usa masterPassword.
// Senha separada é útil quando o backup vai pra nuvem/pendrive — forçar uma
// senha aleatória forte faz o bundle sobreviver mesmo se a senha mestra vazar.
function exportBundle(memDir, masterPassword, bundlePassword) {
  requireUIUnlock();
  if (!isInitialized(memDir)) throw new Error('vault: not initialized');
  const vaultBuf = fs.readFileSync(vaultPath(memDir));
  const dkBuf = fs.readFileSync(deviceKeyPath(memDir));
  // Re-valida senha mestra mesmo com UI aberta — prova que o humano ainda está ali.
  const store = loadStore(memDir);
  if (!store.auth || !verifyPassword(masterPassword, store.auth)) {
    throw new Error('vault: senha mestra incorreta');
  }
  const encPass = bundlePassword && String(bundlePassword).length > 0
    ? String(bundlePassword)
    : String(masterPassword);
  const payload = Buffer.from(JSON.stringify({
    __version: 1,
    exported_at: new Date().toISOString(),
    vault_b64: vaultBuf.toString('base64'),
    device_key_b64: dkBuf.toString('base64'),
  }), 'utf8');
  const salt = crypto.randomBytes(SALT_LEN);
  const nonce = crypto.randomBytes(NONCE_LEN);
  const key = deriveExportKey(encPass, salt);
  const cipher = crypto.createCipheriv('aes-256-gcm', key, nonce);
  cipher.setAAD(Buffer.concat([EXPORT_MAGIC, salt]));
  const ct = Buffer.concat([cipher.update(payload), cipher.final()]);
  const tag = cipher.getAuthTag();
  return Buffer.concat([EXPORT_MAGIC, salt, nonce, ct, tag]);
}

// `bundlePassword` decripta o bundle. `newMasterPassword` (obrigatório) define a
// senha mestra da UI no destino — permite que outra pessoa importe sem precisar
// da senha mestra original (ex: Alice compartilha bundle+senha com Bob, Bob
// escolhe a própria master). Device-key vem do bundle, secrets continuam
// encriptados por ela; só o auth block é regravado com o novo scrypt hash.
function importBundle(memDir, bundleBuf, bundlePassword, replace, newMasterPassword) {
  if (!Buffer.isBuffer(bundleBuf)) bundleBuf = Buffer.from(bundleBuf);
  const minLen = EXPORT_MAGIC.length + SALT_LEN + NONCE_LEN + TAG_LEN;
  if (bundleBuf.length < minLen) throw new Error('vault: bundle truncado');
  if (!bundleBuf.slice(0, EXPORT_MAGIC.length).equals(EXPORT_MAGIC)) {
    throw new Error('vault: formato de bundle inválido');
  }
  if (!newMasterPassword || String(newMasterPassword).length < 8) {
    throw new Error('vault: nova senha mestra precisa de no mínimo 8 caracteres');
  }
  const already = isInitialized(memDir);
  if (already && !replace) throw new Error('vault: já existe cofre — use replace=true');
  if (already && !isUnlocked()) throw new Error('vault: UI locked');

  const salt = bundleBuf.slice(EXPORT_MAGIC.length, EXPORT_MAGIC.length + SALT_LEN);
  const nonce = bundleBuf.slice(
    EXPORT_MAGIC.length + SALT_LEN,
    EXPORT_MAGIC.length + SALT_LEN + NONCE_LEN,
  );
  const tag = bundleBuf.slice(bundleBuf.length - TAG_LEN);
  const ciphertext = bundleBuf.slice(
    EXPORT_MAGIC.length + SALT_LEN + NONCE_LEN,
    bundleBuf.length - TAG_LEN,
  );
  const key = deriveExportKey(bundlePassword, salt);
  const decipher = crypto.createDecipheriv('aes-256-gcm', key, nonce);
  decipher.setAAD(Buffer.concat([EXPORT_MAGIC, salt]));
  decipher.setAuthTag(tag);
  let plain;
  try {
    plain = Buffer.concat([decipher.update(ciphertext), decipher.final()]);
  } catch {
    throw new Error('vault: senha do bundle incorreta ou bundle corrompido');
  }
  let payload;
  try { payload = JSON.parse(plain.toString('utf8')); }
  catch { throw new Error('vault: bundle ilegível'); }
  if (!payload || payload.__version !== 1 || !payload.vault_b64 || !payload.device_key_b64) {
    throw new Error('vault: bundle com campos faltando');
  }
  const newVault = Buffer.from(payload.vault_b64, 'base64');
  const newDeviceKey = Buffer.from(payload.device_key_b64, 'base64');
  if (newDeviceKey.length !== KEY_LEN) throw new Error('vault: device-key do bundle inválida');
  if (newVault.length < 4 || !newVault.slice(0, 4).equals(MAGIC_V2)) {
    throw new Error('vault: vault.enc do bundle inválido');
  }

  // Escrita atômica — se algo falhar, cofre atual fica intacto.
  ensureGitignore(memDir);
  writeFileSafe(deviceKeyPath(memDir), newDeviceKey, 0o600);
  _cachedDeviceKey = newDeviceKey;
  writeFileSafe(vaultPath(memDir), newVault, 0o600);
  // Regrava auth block com a nova senha mestra escolhida por quem importou.
  // Secrets/envfiles continuam intactos (device-key não mudou). Isso permite
  // compartilhar o bundle sem vazar a senha mestra da origem.
  const store = loadStore(memDir);
  store.auth = buildAuthBlock(newMasterPassword);
  saveStore(memDir, store);
  // Fecha sessão — humano precisa digitar a nova senha mestra pra entrar na UI.
  lock();
  return {
    imported_at: new Date().toISOString(),
    exported_at: payload.exported_at || null,
  };
}

module.exports = {
  isInitialized,
  init,
  unlock,
  lock,
  isUnlocked,
  status,
  changePassword,
  listKeys,
  getSecret,
  getSecrets,
  setSecret,
  deleteSecret,
  listEnvFiles,
  getEnvFile,
  setEnvFile,
  deleteEnvFile,
  destroy,
  exportBundle,
  importBundle,
};
