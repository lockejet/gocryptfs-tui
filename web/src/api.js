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
        return body.error || body.message || text;
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
  }
  if (body !== undefined) {
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

// --- session ---------------------------------------------------------------

export async function login(token) {
  await request('/api/session', { method: 'POST', body: { token } });
}

export async function logout() {
  await request('/api/session', { method: 'DELETE' });
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
