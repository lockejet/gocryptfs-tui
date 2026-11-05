// Minimal API client for the gocryptfs-ui Web UI.
// Every call is same-origin (cookie session) and every mutating call carries
// the `x-gfs-csrf: 1` header required by the backend.

export class ApiError extends Error {
  constructor(message, status) {
    super(message);
    this.name = 'ApiError';
    this.status = status;
  }
}

async function readError(res) {
  let text = '';
  try {
    text = await res.text();
  } catch (_) {
    /* ignore */
  }
  if (text) {
    try {
      const body = JSON.parse(text);
      if (body && typeof body === 'object') {
        if (typeof body.error === 'string') return body.error;
        if (body.error && typeof body.error === 'object') {
          return body.error.code || body.error.message || text;
        }
        return body.message || text;
      }
    } catch (_) {
      /* not JSON */
    }
    return text;
  }
  return `Request failed with HTTP ${res.status}`;
}

async function request(path, { method = 'GET', body } = {}) {
  const headers = {};
  if (method !== 'GET') {
    headers['x-gfs-csrf'] = '1';
    headers['content-type'] = 'application/json';
  }

  let res;
  try {
    res = await fetch(path, {
      method,
      credentials: 'same-origin',
      headers,
      body: body === undefined ? undefined : JSON.stringify(body)
    });
  } catch (err) {
    throw new ApiError(`Network error: ${err.message}`, 0);
  }

  if (!res.ok) {
    throw new ApiError(await readError(res), res.status);
  }
  return res;
}

async function json(res) {
  const text = await res.text();
  if (!text) return null;
  try {
    return JSON.parse(text);
  } catch (_) {
    throw new ApiError('Invalid JSON received from server', res.status);
  }
}

// --- auth status -----------------------------------------------------------

const AUTH_FALLBACK = { token_auth: true, password_auth: false, '2fa_available': false };

// GET /api/auth/status is public. Any failure degrades to token-only auth so
// the login screen always renders.
export async function authStatus() {
  try {
    const res = await fetch('/api/auth/status', {
      method: 'GET',
      credentials: 'same-origin',
      headers: { accept: 'application/json' }
    });
    if (!res.ok) return { ...AUTH_FALLBACK };
    const data = (await res.json()) || {};
    return {
      token_auth: typeof data.token_auth === 'boolean' ? data.token_auth : true,
      password_auth: data.password_auth === true,
      '2fa_available':
        data['2fa_available'] === true || data['twofa_available'] === true
    };
  } catch (_) {
    return { ...AUTH_FALLBACK };
  }
}

// --- session ---------------------------------------------------------------

export async function loginWithToken(token) {
  await request('/api/session', { method: 'POST', body: { token } });
}

// QR / one-time link login: the nonce travels in the page URL fragment.
export async function loginWithNonce(nonce) {
  await request('/api/session', { method: 'POST', body: { nonce } });
}

export async function loginWithPassword(username, password, totp) {
  const body = { username, password };
  if (totp) body.totp = totp;
  await request('/api/session', { method: 'POST', body });
}

// Backwards compatible alias.
export const login = loginWithToken;

export async function logout() {
  await request('/api/session', { method: 'DELETE' });
}

// --- one-time login links --------------------------------------------------

export async function createLoginLink() {
  const data = await json(await request('/api/login-links', { method: 'POST' }));
  return data || {};
}

// --- two-factor auth -------------------------------------------------------

export async function enroll2fa() {
  return (await json(await request('/api/2fa/enroll', { method: 'POST' }))) || {};
}

export async function activate2fa(code) {
  return (
    (await json(await request('/api/2fa/activate', { method: 'POST', body: { code } }))) || {}
  );
}

export async function disable2fa(code) {
  await request('/api/2fa/disable', { method: 'POST', body: { code } });
}

export async function regenerateBackupCodes(code) {
  return (
    (await json(await request('/api/2fa/backup-codes', { method: 'POST', body: { code } }))) || {}
  );
}

// --- vaults ----------------------------------------------------------------

export async function getVaults() {
  const data = await json(await request('/api/vaults'));
  if (Array.isArray(data)) return data;
  if (data && Array.isArray(data.vaults)) return data.vaults;
  return [];
}

export async function mountVault(name, passphrase) {
  const data = await json(
    await request(`/api/vaults/${encodeURIComponent(name)}/mount`, {
      method: 'POST',
      body: { passphrase }
    })
  );
  return jobIdOf(data);
}

export async function umountVault(name) {
  const data = await json(
    await request(`/api/vaults/${encodeURIComponent(name)}/umount`, {
      method: 'POST',
      body: { force: false }
    })
  );
  return jobIdOf(data);
}

function jobIdOf(data) {
  if (!data) return null;
  if (typeof data === 'string' || typeof data === 'number') return String(data);
  return data.id != null ? String(data.id) : data.job_id != null ? String(data.job_id) : null;
}

// --- logs / meta -----------------------------------------------------------

export async function getLogs(limit = 200) {
  const data = await json(await request(`/api/logs?limit=${encodeURIComponent(limit)}`));
  if (Array.isArray(data)) return data;
  if (data && Array.isArray(data.logs)) return data.logs;
  if (data && Array.isArray(data.items)) return data.items;
  return [];
}

export async function getMeta() {
  return (await json(await request('/api/meta'))) || {};
}
