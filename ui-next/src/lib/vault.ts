export interface VaultStatus {
  initialized: boolean;
  locked: boolean;
  expires_at: number | null;
  ttl_ms: number | null;
  needs_migration?: boolean;
}

export async function getVaultStatus(): Promise<VaultStatus> {
  const r = await fetch('/api/vault/status');
  if (!r.ok) throw new Error(`HTTP ${r.status}`);
  return r.json();
}

export async function initVault(password: string): Promise<void> {
  const r = await fetch('/api/vault/init', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ password }),
  });
  if (!r.ok) {
    const j = await r.json().catch(() => ({}));
    throw new Error(j.error || `HTTP ${r.status}`);
  }
}

export async function unlockVault(password: string, ttlMin = 30): Promise<void> {
  const r = await fetch('/api/vault/unlock', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ password, ttl_min: ttlMin }),
  });
  if (!r.ok) {
    const j = await r.json().catch(() => ({}));
    throw new Error(j.error || 'senha inválida');
  }
}

export async function lockVault(): Promise<void> {
  await fetch('/api/vault/lock', { method: 'POST' });
}

export async function changeVaultPassword(oldPassword: string, newPassword: string): Promise<void> {
  const r = await fetch('/api/vault/change-password', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ old_password: oldPassword, new_password: newPassword }),
  });
  if (!r.ok) {
    const j = await r.json().catch(() => ({}));
    throw new Error(j.error || `HTTP ${r.status}`);
  }
}

export async function listVaultKeys(): Promise<string[]> {
  const r = await fetch('/api/vault/keys');
  if (!r.ok) {
    const j = await r.json().catch(() => ({}));
    throw new Error(j.error || `HTTP ${r.status}`);
  }
  const j = await r.json();
  return j.keys || [];
}

export async function getVaultSecret(name: string): Promise<string> {
  const r = await fetch(`/api/vault/secret/${encodeURIComponent(name)}`);
  if (!r.ok) {
    const j = await r.json().catch(() => ({}));
    throw new Error(j.error || `HTTP ${r.status}`);
  }
  const j = await r.json();
  return j.value || '';
}

export async function setVaultSecret(name: string, value: string): Promise<void> {
  const r = await fetch(`/api/vault/secret/${encodeURIComponent(name)}`, {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ value }),
  });
  if (!r.ok) {
    const j = await r.json().catch(() => ({}));
    throw new Error(j.error || `HTTP ${r.status}`);
  }
}

export async function deleteVaultSecret(name: string): Promise<void> {
  const r = await fetch(`/api/vault/secret/${encodeURIComponent(name)}`, { method: 'DELETE' });
  if (!r.ok) {
    const j = await r.json().catch(() => ({}));
    throw new Error(j.error || `HTTP ${r.status}`);
  }
}

export interface EnvFileEntry {
  name: string;
  updated_at: string | null;
  size: number;
}

export async function listEnvFiles(): Promise<EnvFileEntry[]> {
  const r = await fetch('/api/vault/envfiles');
  if (!r.ok) {
    const j = await r.json().catch(() => ({}));
    throw new Error(j.error || `HTTP ${r.status}`);
  }
  const j = await r.json();
  return j.envfiles || [];
}

export async function getEnvFile(name: string): Promise<string> {
  const r = await fetch(`/api/vault/envfile/${encodeURIComponent(name)}`);
  if (!r.ok) {
    const j = await r.json().catch(() => ({}));
    throw new Error(j.error || `HTTP ${r.status}`);
  }
  const j = await r.json();
  return j.content || '';
}

export async function setEnvFile(name: string, content: string): Promise<void> {
  const r = await fetch(`/api/vault/envfile/${encodeURIComponent(name)}`, {
    method: 'PUT',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ content }),
  });
  if (!r.ok) {
    const j = await r.json().catch(() => ({}));
    throw new Error(j.error || `HTTP ${r.status}`);
  }
}

export async function deleteEnvFile(name: string): Promise<void> {
  const r = await fetch(`/api/vault/envfile/${encodeURIComponent(name)}`, { method: 'DELETE' });
  if (!r.ok) {
    const j = await r.json().catch(() => ({}));
    throw new Error(j.error || `HTTP ${r.status}`);
  }
}

export async function destroyVault(): Promise<void> {
  const r = await fetch('/api/vault/destroy', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ confirm: 'DESTROY' }),
  });
  if (!r.ok) {
    const j = await r.json().catch(() => ({}));
    throw new Error(j.error || `HTTP ${r.status}`);
  }
}

export interface VaultPaths {
  mem_dir: string;
  vault_file: string;
  device_key_file: string;
}

export async function getVaultPaths(): Promise<VaultPaths> {
  const r = await fetch('/api/vault/paths');
  if (!r.ok) throw new Error(`HTTP ${r.status}`);
  return r.json();
}

export async function exportVault(masterPassword: string, bundlePassword?: string): Promise<Blob> {
  const r = await fetch('/api/vault/export', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({
      master_password: masterPassword,
      bundle_password: bundlePassword || undefined,
    }),
  });
  if (!r.ok) {
    const j = await r.json().catch(() => ({}));
    throw new Error(j.error || `HTTP ${r.status}`);
  }
  return r.blob();
}

// Gera senha aleatória forte p/ bundle. 24 bytes = 192 bits → resistente a
// brute-force offline mesmo com GPU. Formato base64 URL-safe sem padding.
export function generateBundlePassword(): string {
  const bytes = new Uint8Array(24);
  crypto.getRandomValues(bytes);
  let s = btoa(String.fromCharCode(...bytes));
  s = s.replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
  return s;
}

function blobToBase64(blob: Blob): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => {
      const dataUrl = String(reader.result || '');
      const comma = dataUrl.indexOf(',');
      resolve(comma >= 0 ? dataUrl.slice(comma + 1) : dataUrl);
    };
    reader.onerror = () => reject(reader.error);
    reader.readAsDataURL(blob);
  });
}

export async function importVault(
  file: File | Blob,
  bundlePassword: string,
  newMasterPassword: string,
  replace: boolean,
): Promise<void> {
  const bundle_b64 = await blobToBase64(file);
  const r = await fetch('/api/vault/import', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({
      bundle_b64,
      bundle_password: bundlePassword,
      new_master_password: newMasterPassword,
      replace,
    }),
  });
  if (!r.ok) {
    const j = await r.json().catch(() => ({}));
    throw new Error(j.error || `HTTP ${r.status}`);
  }
}
