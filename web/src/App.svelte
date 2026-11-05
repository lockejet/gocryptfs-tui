<script>
  import { onMount, onDestroy, tick } from 'svelte';
  import * as api from './api.js';
  import {
    themeMode,
    initTheme,
    cycleTheme,
    themeLabel,
    themeIcon
  } from './theme.js';

  const TABS = [
    { id: 'vaults', label: 'Vaults' },
    { id: 'logs', label: 'Logs' },
    { id: 'meta', label: 'Meta' },
    { id: 'security', label: 'Security' }
  ];

  const LOOPBACK_HOSTS = ['localhost', '127.0.0.1', '::1', '[::1]'];
  const isLoopback =
    typeof location !== 'undefined' && LOOPBACK_HOSTS.includes(location.hostname);

  // session
  let checking = true;
  let authed = false;
  let loginError = '';
  let loginBusy = false;
  let authStatus = { token_auth: true, password_auth: false, '2fa_available': false };

  // login form
  let loginMode = 'token'; // 'token' | 'password'
  let token = '';
  let username = '';
  let password = '';
  let totp = '';
  let totpRequired = false;

  // navigation
  let tab = 'vaults';

  // vaults
  let vaults = [];
  let vaultsLoading = false;
  let vaultsError = '';

  // logs
  let logs = [];
  let logsLoading = false;
  let logsError = '';

  // meta
  let meta = null;
  let metaLoading = false;
  let metaError = '';

  // jobs: name -> { pct, done, total, logs, error, code, active }
  let jobs = {};
  const streams = new Map();

  // mount dialog
  let mountTarget = null;
  let passphrase = '';
  let mountError = '';
  let mountBusy = false;
  let passInput;

  // one-time login link (手机登录)
  let qr = null;
  let qrOpen = false;
  let qrBusy = false;
  let qrError = '';
  let qrLeft = 0;
  let qrTimer = null;
  let qrCopied = false;

  // security tab
  let enroll = null; // { secret, otpauth_url, svg }
  let enrollCode = '';
  let enrollBusy = false;
  let enrollError = '';
  let backupCodes = null;
  let backupOrigin = ''; // 'activate' | 'regenerate'
  let disableCode = '';
  let disableBusy = false;
  let disableError = '';
  let disableNotice = '';
  let regenCode = '';
  let regenBusy = false;
  let regenError = '';

  let copiedKey = '';
  let copyTimer = null;
  let totpInput;

  $: theme = $themeMode;
  $: metaPaths = (meta && meta.paths) || {};
  $: metaDeps = (meta && (meta.deps || meta.dependencies)) || {};
  $: showPasswordTab = authStatus.password_auth === true;

  onMount(() => {
    const stopTheme = initTheme();
    bootstrap();
    return () => {
      stopTheme();
    };
  });

  onDestroy(() => {
    closeAllStreams();
    stopQrTimer();
    if (copyTimer) clearTimeout(copyTimer);
  });

  // --- bootstrap -----------------------------------------------------------

  function readNonce() {
    if (typeof location === 'undefined') return '';
    const hash = location.hash || '';
    const match = /^#t=(.+)$/.exec(hash);
    if (!match) return '';
    try {
      return decodeURIComponent(match[1]);
    } catch (_) {
      return match[1];
    }
  }

  function stripHash() {
    if (typeof history === 'undefined' || !history.replaceState) return;
    try {
      history.replaceState(null, '', location.pathname + location.search);
    } catch (_) {
      /* ignore */
    }
  }

  async function bootstrap() {
    checking = true;
    authStatus = await api.authStatus();
    if (!authStatus.token_auth && authStatus.password_auth) loginMode = 'password';

    const nonce = readNonce();
    if (nonce) {
      try {
        await api.loginWithNonce(nonce);
      } catch (err) {
        loginError =
          err.status === 401 || err.status === 400
            ? 'This one-time login link is invalid or has expired.'
            : err.message;
      } finally {
        // Never keep a consumed/expired nonce in the URL.
        stripHash();
      }
    }

    try {
      vaults = (await api.getVaults()).map(normalizeVault);
      authed = true;
      loginError = '';
    } catch (err) {
      authed = false;
      if (err.status !== 401 && !loginError) loginError = err.message;
    } finally {
      checking = false;
    }
  }

  function normalizeVault(v) {
    if (typeof v === 'string') return { name: v, mountPoint: '', mounted: false };
    const mounted =
      typeof v.mounted === 'boolean'
        ? v.mounted
        : v.state === 'mounted' || v.status === 'mounted' || v.is_mounted === true;
    return {
      name: v.name != null ? String(v.name) : v.vault != null ? String(v.vault) : '',
      mountPoint: v.mount_point || v.mountpoint || v.mountPoint || v.path || '',
      mounted
    };
  }

  async function loadVaults() {
    vaultsLoading = true;
    vaultsError = '';
    try {
      const data = await api.getVaults();
      vaults = data.map(normalizeVault);
    } catch (err) {
      if (err.status === 401) return onUnauthorized();
      vaultsError = err.message;
    } finally {
      vaultsLoading = false;
    }
  }

  async function loadLogs() {
    logsLoading = true;
    logsError = '';
    try {
      logs = await api.getLogs(200);
    } catch (err) {
      if (err.status === 401) return onUnauthorized();
      logsError = err.message;
      logs = [];
    } finally {
      logsLoading = false;
    }
  }

  async function loadMeta() {
    metaLoading = true;
    metaError = '';
    try {
      meta = await api.getMeta();
    } catch (err) {
      if (err.status === 401) return onUnauthorized();
      metaError = err.message;
      meta = null;
    } finally {
      metaLoading = false;
    }
  }

  function selectTab(id) {
    if (tab === id) return;
    tab = id;
    if (id === 'vaults') loadVaults();
    else if (id === 'logs') loadLogs();
    else if (id === 'meta') loadMeta();
  }

  function onUnauthorized() {
    closeAllStreams();
    jobs = {};
    authed = false;
    loginError = 'Session expired. Please sign in again.';
  }

  // --- login ---------------------------------------------------------------

  function switchLoginMode(mode) {
    if (mode === loginMode) return;
    loginMode = mode;
    loginError = '';
    totpRequired = false;
    totp = '';
  }

  function loginMessage(err) {
    if (err.status === 429) return 'Too many attempts. Please wait a minute and try again.';
    if (err.status === 401) {
      const msg = String(err.message || '').toLowerCase();
      if (msg.includes('totp_required') || msg.includes('totp required')) return '';
      if (msg.includes('totp') || msg.includes('backup')) return 'Invalid authentication code.';
      return loginMode === 'token' ? 'Invalid token.' : 'Invalid username or password.';
    }
    if (err.status === 0) return 'Cannot reach the server. Check that it is running.';
    return err.message || 'Sign-in failed.';
  }

  async function submitLogin() {
    loginBusy = true;
    loginError = '';
    try {
      if (loginMode === 'token') {
        await api.loginWithToken(token.trim());
      } else {
        await api.loginWithPassword(username.trim(), password, totpRequired ? totp.trim() : '');
      }
      token = '';
      password = '';
      totp = '';
      totpRequired = false;
      authed = true;
      tab = 'vaults';
      await loadVaults();
    } catch (err) {
      const msg = String(err.message || '').toLowerCase();
      if (err.status === 401 && msg.includes('totp')) {
        totpRequired = true;
        loginError = msg.includes('required')
          ? 'Enter the 6-digit code from your authenticator app.'
          : 'Invalid authentication code. Try again or use a backup code.';
        tick().then(() => totpInput && totpInput.focus());
      } else {
        loginError = loginMessage(err);
      }
    } finally {
      loginBusy = false;
    }
  }

  async function doLogout() {
    try {
      await api.logout();
    } catch (_) {
      /* logging out locally is enough */
    }
    closeAllStreams();
    closeQr();
    jobs = {};
    vaults = [];
    logs = [];
    meta = null;
    enroll = null;
    backupCodes = null;
    authed = false;
    token = '';
    password = '';
    totp = '';
    totpRequired = false;
    loginError = '';
    loginBusy = false;
    checking = false;
  }

  // --- one-time login link -------------------------------------------------

  async function openQr() {
    qrBusy = true;
    qrOpen = true;
    qr = null;
    qrLeft = 0;
    stopQrTimer();
    qrError = '';
    qrCopied = false;
    try {
      const data = await api.createLoginLink();
      if (!data || !data.svg) throw new api.ApiError('Server did not return a login QR code', 0);
      qr = data;
      qrLeft = Number(data.expires_in) || 120;
      startQrTimer();
    } catch (err) {
      qrError = err.message || 'Could not create a login link.';
    } finally {
      qrBusy = false;
    }
  }

  function startQrTimer() {
    stopQrTimer();
    qrTimer = setInterval(() => {
      qrLeft = Math.max(0, qrLeft - 1);
      if (qrLeft <= 0) stopQrTimer();
    }, 1000);
  }

  function stopQrTimer() {
    if (qrTimer) {
      clearInterval(qrTimer);
      qrTimer = null;
    }
  }

  function closeQr() {
    stopQrTimer();
    qr = null;
    qrOpen = false;
    qrError = '';
    qrLeft = 0;
    qrCopied = false;
  }

  async function copyQrUrl() {
    if (!qr) return;
    qrCopied = await copyValue(qr.url, 'qrurl');
  }

  // --- clipboard -----------------------------------------------------------

  async function copyValue(value, key) {
    let ok = false;
    try {
      await navigator.clipboard.writeText(value);
      ok = true;
    } catch (_) {
      ok = fallbackCopy(value);
    }
    if (ok) {
      copiedKey = key;
      if (copyTimer) clearTimeout(copyTimer);
      copyTimer = setTimeout(() => {
        copiedKey = '';
        copyTimer = null;
      }, 1800);
    }
    return ok;
  }

  function fallbackCopy(value) {
    try {
      const ta = document.createElement('textarea');
      ta.value = value;
      ta.setAttribute('readonly', '');
      ta.style.position = 'fixed';
      ta.style.opacity = '0';
      document.body.appendChild(ta);
      ta.select();
      const ok = document.execCommand('copy');
      document.body.removeChild(ta);
      return ok;
    } catch (_) {
      return false;
    }
  }

  // --- 2FA -----------------------------------------------------------------

  async function startEnroll() {
    enrollBusy = true;
    enrollError = '';
    backupCodes = null;
    try {
      const data = await api.enroll2fa();
      if (!data.secret || !data.svg) throw new api.ApiError('Server did not return 2FA data', 0);
      enroll = data;
      enrollCode = '';
    } catch (err) {
      if (err.status === 401) return onUnauthorized();
      enrollError = err.message || 'Enrollment failed.';
    } finally {
      enrollBusy = false;
    }
  }

  async function activateEnroll() {
    if (!enroll) return;
    enrollBusy = true;
    enrollError = '';
    try {
      const data = await api.activate2fa(enrollCode.trim());
      backupCodes = Array.isArray(data.backup_codes) ? data.backup_codes : [];
      backupOrigin = 'activate';
      enroll = null;
      enrollCode = '';
    } catch (err) {
      if (err.status === 401 && String(err.message || '').includes('totp')) {
        enrollError = 'That code is not valid. Check your app and try again.';
      } else if (err.status === 401) {
        return onUnauthorized();
      } else {
        enrollError = err.message || 'Activation failed.';
      }
    } finally {
      enrollBusy = false;
    }
  }

  function cancelEnroll() {
    enroll = null;
    enrollCode = '';
    enrollError = '';
  }

  async function submitDisable() {
    disableBusy = true;
    disableError = '';
    disableNotice = '';
    try {
      await api.disable2fa(disableCode.trim());
      disableCode = '';
      disableNotice = 'Two-factor authentication disabled.';
    } catch (err) {
      if (err.status === 401 && String(err.message || '').includes('totp')) {
        disableError = 'Invalid authentication code.';
      } else {
        disableError = err.message || 'Could not disable 2FA.';
      }
    } finally {
      disableBusy = false;
    }
  }

  async function submitRegen() {
    regenBusy = true;
    regenError = '';
    try {
      const data = await api.regenerateBackupCodes(regenCode.trim());
      backupCodes = Array.isArray(data.backup_codes) ? data.backup_codes : [];
      backupOrigin = 'regenerate';
      regenCode = '';
    } catch (err) {
      if (err.status === 401 && String(err.message || '').includes('totp')) {
        regenError = 'Invalid authentication code.';
      } else {
        regenError = err.message || 'Could not generate new backup codes.';
      }
    } finally {
      regenBusy = false;
    }
  }

  function dismissBackupCodes() {
    backupCodes = null;
    backupOrigin = '';
  }

  // --- jobs ----------------------------------------------------------------

  function patchJob(name, patch) {
    const prev = jobs[name] || {
      pct: 0,
      done: 0,
      total: 0,
      logs: [],
      error: '',
      code: null,
      active: true
    };
    jobs = { ...jobs, [name]: { ...prev, ...patch } };
  }

  function parseEvent(ev) {
    if (!ev.data) return {};
    try {
      return JSON.parse(ev.data);
    } catch (_) {
      return { line: ev.data };
    }
  }

  function startJob(name, jobId) {
    closeStream(name);
    jobs = {
      ...jobs,
      [name]: { pct: 0, done: 0, total: 0, logs: [], error: '', code: null, active: true }
    };

    const es = new EventSource(`/api/jobs/${encodeURIComponent(jobId)}/events`);
    streams.set(name, es);

    es.addEventListener('progress', (ev) => {
      const d = parseEvent(ev);
      let pct = Number(d.pct);
      if (!Number.isFinite(pct) && Number(d.total) > 0) {
        pct = (Number(d.done) / Number(d.total)) * 100;
      }
      if (!Number.isFinite(pct)) pct = 0;
      pct = Math.max(0, Math.min(100, pct));
      patchJob(name, { pct, done: Number(d.done) || 0, total: Number(d.total) || 0 });
    });

    es.addEventListener('log', (ev) => {
      const d = parseEvent(ev);
      const line = d.line != null ? String(d.line) : '';
      const prev = jobs[name] || { logs: [] };
      const next = prev.logs.concat([line]);
      patchJob(name, { logs: next.slice(-300) });
    });

    es.addEventListener('done', (ev) => {
      const d = parseEvent(ev);
      const code = d.code == null ? 0 : Number(d.code);
      patchJob(name, {
        pct: 100,
        active: false,
        code,
        error: code === 0 ? '' : `Job exited with code ${code}`
      });
      closeStream(name);
      loadVaults();
    });

    es.addEventListener('error', (ev) => {
      if (ev.data) {
        const d = parseEvent(ev);
        patchJob(name, {
          active: false,
          error: d.message ? String(d.message) : 'Job failed'
        });
        closeStream(name);
        loadVaults();
      } else {
        patchJob(name, {
          active: false,
          error: 'Lost connection to the job event stream'
        });
        closeStream(name);
        loadVaults();
      }
    });
  }

  function closeStream(name) {
    const es = streams.get(name);
    if (es) {
      es.close();
      streams.delete(name);
    }
  }

  function closeAllStreams() {
    streams.forEach((es) => es.close());
    streams.clear();
  }

  // --- actions -------------------------------------------------------------

  function openMount(vault) {
    mountTarget = vault;
    passphrase = '';
    mountError = '';
    tick().then(() => passInput && passInput.focus());
  }

  function closeMount() {
    if (mountBusy) return;
    mountTarget = null;
    passphrase = '';
    mountError = '';
  }

  function onKeydown(ev) {
    if (ev.key === 'Escape') {
      if (mountTarget && !mountBusy) closeMount();
      else if (qrOpen) closeQr();
    }
  }

  async function submitMount() {
    if (!mountTarget) return;
    const name = mountTarget.name;
    mountBusy = true;
    mountError = '';
    try {
      const jobId = await api.mountVault(name, passphrase);
      mountTarget = null;
      passphrase = '';
      if (jobId) startJob(name, jobId);
      else await loadVaults();
    } catch (err) {
      if (err.status === 401) return onUnauthorized();
      mountError = err.message;
    } finally {
      mountBusy = false;
    }
  }

  async function doUmount(vault) {
    const name = vault.name;
    jobs = {
      ...jobs,
      [name]: { pct: 0, done: 0, total: 0, logs: [], error: '', code: null, active: true }
    };
    try {
      const jobId = await api.umountVault(name);
      if (jobId) startJob(name, jobId);
      else {
        patchJob(name, { active: false, pct: 100 });
        await loadVaults();
      }
    } catch (err) {
      if (err.status === 401) return onUnauthorized();
      patchJob(name, { active: false, error: err.message });
    }
  }

  function isBusy(name) {
    return !!(jobs[name] && jobs[name].active);
  }

  // --- formatting ----------------------------------------------------------

  function fmtTime(t) {
    if (t === null || t === undefined || t === '') return '—';
    if (typeof t === 'number') {
      const d = new Date(t < 1e12 ? t * 1000 : t);
      return isNaN(d.getTime()) ? String(t) : d.toLocaleString();
    }
    const d = new Date(t);
    return isNaN(d.getTime()) ? String(t) : d.toLocaleString();
  }

  function depInfo(v) {
    if (v === true) return { ok: true, text: 'ok' };
    if (v === false) return { ok: false, text: 'missing' };
    if (v && typeof v === 'object') {
      const ok = v.ok !== undefined ? v.ok : v.available !== undefined ? v.available : v.found;
      const text = v.version || v.path || (ok === false ? 'missing' : ok === true ? 'ok' : 'unknown');
      return { ok: ok !== false, text: String(text) };
    }
    if (v === null || v === undefined) return { ok: true, text: 'unknown' };
    return { ok: true, text: String(v) };
  }

  function resultTone(result) {
    return {
      on: result === 'ok' || result === 'success' || result === true,
      off: result === 'error' || result === 'fail' || result === 'false' || result === false
    };
  }
</script>

<svelte:window on:keydown={onKeydown} />

{#if !isLoopback}
  <div class="banner" role="status">
    You are not accessing this UI over loopback. The session cookie may be exposed on the network.
  </div>
{/if}

{#if checking}
  <div class="boot">Loading…</div>
{:else if !authed}
  <main class="login-wrap">
    <form class="card login" on:submit|preventDefault={submitLogin}>
      <h1>gocryptfs-ui</h1>

      {#if showPasswordTab && authStatus.token_auth}
        <div class="seg" role="tablist" aria-label="Sign-in method">
          <button
            type="button"
            class="seg-btn"
            class:active={loginMode === 'token'}
            role="tab"
            aria-selected={loginMode === 'token'}
            on:click={() => switchLoginMode('token')}
          >
            Token
          </button>
          <button
            type="button"
            class="seg-btn"
            class:active={loginMode === 'password'}
            role="tab"
            aria-selected={loginMode === 'password'}
            on:click={() => switchLoginMode('password')}
          >
            Username &amp; password
          </button>
        </div>
      {/if}

      {#if loginMode === 'token'}
        <p class="muted">Enter the access token to unlock this console.</p>
        <label for="token">Access token</label>
        <input
          id="token"
          type="password"
          bind:value={token}
          autocomplete="current-password"
          placeholder="session token"
          disabled={loginBusy}
        />
      {:else}
        <p class="muted">Sign in with your account credentials.</p>
        <label for="username">Username</label>
        <input
          id="username"
          type="text"
          bind:value={username}
          autocomplete="username"
          autocapitalize="none"
          spellcheck="false"
          disabled={loginBusy}
        />
        <label for="password">Password</label>
        <input
          id="password"
          type="password"
          bind:value={password}
          autocomplete="current-password"
          disabled={loginBusy}
        />
        {#if totpRequired}
          <label for="totp">Authentication code</label>
          <input
            id="totp"
            class="totp"
            type="text"
            bind:value={totp}
            bind:this={totpInput}
            inputmode="numeric"
            autocomplete="one-time-code"
            autocapitalize="none"
            spellcheck="false"
            placeholder="123456 or backup code"
            maxlength="24"
            disabled={loginBusy}
          />
          <p class="muted small-text">
            Two-factor authentication is enabled. Enter the 6-digit code from your authenticator
            app, or one of your backup codes.
          </p>
        {/if}
      {/if}

      {#if loginError}
        <p class="error" role="alert">{loginError}</p>
      {/if}

      <button
        class="btn primary block"
        type="submit"
        disabled={loginBusy ||
          (loginMode === 'token' ? !token.trim() : !username.trim() || !password)}
      >
        {loginBusy ? 'Signing in…' : 'Sign in'}
      </button>

      {#if authStatus['2fa_available']}
        <p class="muted small-text center">
          Lost your device? Use a backup code, or ask the host operator for a one-time login link.
        </p>
      {/if}
    </form>
  </main>
{:else}
  <header class="topbar">
    <div class="topbar-inner">
      <div class="brand">gocryptfs-ui</div>
      <nav class="tabs" aria-label="Pages">
        {#each TABS as t}
          <button
            type="button"
            class="tab"
            class:active={tab === t.id}
            aria-current={tab === t.id ? 'page' : undefined}
            on:click={() => selectTab(t.id)}
          >
            {t.label}
          </button>
        {/each}
      </nav>
      <div class="topbar-actions">
        <button
          class="btn ghost small theme-toggle"
          type="button"
          on:click={cycleTheme}
          title={`Theme: ${themeLabel(theme)} (click to change)`}
          aria-label={`Theme: ${themeLabel(theme)}. Click to switch to ${themeLabel(
            theme === 'auto' ? 'light' : theme === 'light' ? 'dark' : 'auto'
          )}.`}
        >
          <span class="theme-icon" aria-hidden="true">{themeIcon(theme)}</span>
          <span class="theme-label">{themeLabel(theme)}</span>
        </button>
        <button class="btn ghost small" type="button" on:click={openQr} disabled={qrBusy}>
          {qrBusy ? '…' : '手机登录'}
        </button>
        <button class="btn ghost small" type="button" on:click={doLogout}>Logout</button>
      </div>
    </div>
  </header>

  <main class="content">
    {#if tab === 'vaults'}
      <section>
        <div class="section-head">
          <h2>Vaults</h2>
          <button class="btn" type="button" on:click={loadVaults} disabled={vaultsLoading}>
            {vaultsLoading ? 'Refreshing…' : 'Refresh'}
          </button>
        </div>

        {#if vaultsError}
          <div class="notice error" role="alert">
            <span>{vaultsError}</span>
            <button class="btn small" type="button" on:click={loadVaults}>Retry</button>
          </div>
        {/if}

        {#if !vaults.length && !vaultsLoading}
          <p class="muted empty">No vaults configured.</p>
        {:else}
          <div class="vault-grid">
            {#each vaults as vault (vault.name)}
              {@const job = jobs[vault.name]}
              <article class="card vault">
                <header class="vault-head">
                  <h3>{vault.name}</h3>
                  <span class="badge" class:on={vault.mounted} class:off={!vault.mounted}>
                    {vault.mounted ? 'mounted' : 'not mounted'}
                  </span>
                </header>
                <p class="path" title={vault.mountPoint}>{vault.mountPoint || '—'}</p>
                <div class="actions">
                  <button
                    class="btn primary"
                    type="button"
                    disabled={vault.mounted || isBusy(vault.name)}
                    on:click={() => openMount(vault)}
                  >
                    Mount
                  </button>
                  <button
                    class="btn"
                    type="button"
                    disabled={!vault.mounted || isBusy(vault.name)}
                    on:click={() => doUmount(vault)}
                  >
                    Umount
                  </button>
                </div>

                {#if job}
                  <div class="job">
                    <div
                      class="bar"
                      role="progressbar"
                      aria-valuemin="0"
                      aria-valuemax="100"
                      aria-valuenow={Math.round(job.pct)}
                    >
                      <div class="bar-fill" class:failed={!!job.error} style={`width:${job.pct}%`}></div>
                    </div>
                    <div class="job-meta">
                      <span>{Math.round(job.pct)}%</span>
                      {#if job.total > 0}<span>{job.done}/{job.total}</span>{/if}
                      {#if job.active}<span class="muted">working…</span>{/if}
                      {#if job.code !== null && !job.error}<span class="ok-text">done</span>{/if}
                    </div>
                    {#if job.error}
                      <p class="error small-text" role="alert">{job.error}</p>
                    {/if}
                    {#if job.logs.length}
                      <pre class="job-log">{job.logs.join('\n')}</pre>
                    {/if}
                  </div>
                {/if}
              </article>
            {/each}
          </div>
        {/if}
      </section>
    {/if}

    {#if tab === 'logs'}
      <section>
        <div class="section-head">
          <h2>Logs</h2>
          <button class="btn" type="button" on:click={loadLogs} disabled={logsLoading}>
            {logsLoading ? 'Refreshing…' : 'Refresh'}
          </button>
        </div>

        {#if logsError}
          <div class="notice error" role="alert">
            <span>{logsError}</span>
            <button class="btn small" type="button" on:click={loadLogs}>Retry</button>
          </div>
        {/if}

        {#if !logs.length && !logsLoading}
          <p class="muted empty">No log entries.</p>
        {:else}
          <div class="table-wrap">
            <div class="logs-table" role="table" aria-label="Log entries">
              <div class="log-row log-head" role="row">
                <span class="cell when" role="columnheader">Time</span>
                <span class="cell src" role="columnheader">Src</span>
                <span class="cell action" role="columnheader">Action</span>
                <span class="cell target" role="columnheader">Target</span>
                <span class="cell result" role="columnheader">Result</span>
                <span class="cell detail" role="columnheader">Detail</span>
              </div>
              {#each logs as row, i (i)}
                {@const tone = resultTone(row.result)}
                <div class="log-row" role="row">
                  <span class="cell when nowrap" role="cell">
                    {fmtTime(row.time ?? row.ts ?? row.timestamp)}
                  </span>
                  <span class="cell src" role="cell">{row.src ?? row.source ?? '—'}</span>
                  <span class="cell action" role="cell">{row.action ?? '—'}</span>
                  <span class="cell target mono" role="cell">{row.target ?? '—'}</span>
                  <span class="cell result" role="cell">
                    <span class="badge" class:on={tone.on} class:off={tone.off}>
                      {row.result ?? '—'}
                    </span>
                  </span>
                  <span class="cell detail" role="cell">{row.detail ?? ''}</span>
                  <span class="cell sub" role="cell">
                    {row.action ?? '—'} · {row.target ?? '—'}{row.detail ? ` · ${row.detail}` : ''}
                  </span>
                </div>
              {/each}
            </div>
          </div>
        {/if}
      </section>
    {/if}

    {#if tab === 'meta'}
      <section>
        <div class="section-head">
          <h2>Meta</h2>
          <button class="btn" type="button" on:click={loadMeta} disabled={metaLoading}>
            {metaLoading ? 'Refreshing…' : 'Refresh'}
          </button>
        </div>

        {#if metaError}
          <div class="notice error" role="alert">
            <span>{metaError}</span>
            <button class="btn small" type="button" on:click={loadMeta}>Retry</button>
          </div>
        {/if}

        {#if meta}
          <div class="card">
            <h3>Version</h3>
            <p class="mono">{meta.version ?? meta.version_string ?? '—'}</p>
          </div>

          <div class="card">
            <h3>Paths</h3>
            <dl class="kv">
              {#each ['config', 'data', 'state', 'log', 'history'] as key}
                <dt>{key}</dt>
                <dd class="mono">{metaPaths[key] ?? '—'}</dd>
              {/each}
              {#each Object.entries(metaPaths).filter(([k]) => !['config', 'data', 'state', 'log', 'history'].includes(k)) as [k, v]}
                <dt>{k}</dt>
                <dd class="mono">{v}</dd>
              {/each}
            </dl>
          </div>

          <div class="card">
            <h3>Dependencies</h3>
            {#if Object.keys(metaDeps).length}
              <ul class="deps">
                {#each Object.entries(metaDeps) as [name, value]}
                  {@const info = depInfo(value)}
                  <li>
                    <span class="dep-name">{name}</span>
                    <span class="badge" class:on={info.ok} class:off={!info.ok}>{info.text}</span>
                  </li>
                {/each}
              </ul>
            {:else}
              <p class="muted">No dependency information reported.</p>
            {/if}
          </div>
        {/if}
      </section>
    {/if}

    {#if tab === 'security'}
      <section>
        <div class="section-head">
          <h2>Security</h2>
          <span class="badge" class:on={!!backupCodes} class:off={!backupCodes}>
            {backupCodes ? '2FA codes shown' : '2FA'}
          </span>
        </div>

        {#if !authStatus['2fa_available']}
          <div class="notice error" role="status">
            <span>This server build does not advertise two-factor authentication support.</span>
          </div>
        {/if}

        <div class="card">
          <h3>Two-factor authentication</h3>
          <p class="muted small-text">
            Add a second factor to your account. You will need an authenticator app such as
            Aegis, 1Password, or Google Authenticator.
          </p>

          {#if !enroll}
            <div class="row-actions">
              <button class="btn primary" type="button" on:click={startEnroll} disabled={enrollBusy}>
                {enrollBusy ? 'Preparing…' : 'Set up 2FA'}
              </button>
            </div>
          {:else}
            <div class="enroll">
              <div class="qr-box" aria-label="2FA enrollment QR code">
                {@html enroll.svg}
              </div>
              <div class="enroll-info">
                <p class="muted small-text">
                  Scan the QR code, or enter this secret manually:
                </p>
                <div class="copy-row">
                  <code class="secret">{enroll.secret}</code>
                  <button
                    class="btn small"
                    type="button"
                    on:click={() => copyValue(enroll.secret, 'secret')}
                  >
                    {copiedKey === 'secret' ? 'Copied' : 'Copy'}
                  </button>
                </div>
                <label for="enroll-code">6-digit code</label>
                <input
                  id="enroll-code"
                  class="totp"
                  type="text"
                  bind:value={enrollCode}
                  inputmode="numeric"
                  autocomplete="one-time-code"
                  placeholder="123456"
                  maxlength="10"
                  disabled={enrollBusy}
                />
                {#if enrollError}
                  <p class="error small-text" role="alert">{enrollError}</p>
                {/if}
                <div class="row-actions">
                  <button class="btn" type="button" on:click={cancelEnroll} disabled={enrollBusy}>
                    Cancel
                  </button>
                  <button
                    class="btn primary"
                    type="button"
                    on:click={activateEnroll}
                    disabled={enrollBusy || enrollCode.trim().length < 6}
                  >
                    {enrollBusy ? 'Verifying…' : 'Activate'}
                  </button>
                </div>
              </div>
            </div>
          {/if}
        </div>

        {#if backupCodes}
          <div class="card backup-card">
            <h3>Backup codes</h3>
            <p class="warn-text" role="alert">
              Save these codes now — they are shown only once. Each code can be used once if you
              lose access to your authenticator.
            </p>
            <ul class="code-list">
              {#each backupCodes as code}
                <li class="mono">{code}</li>
              {/each}
            </ul>
            <div class="row-actions">
              <button
                class="btn"
                type="button"
                on:click={() => copyValue(backupCodes.join('\n'), 'codes')}
              >
                {copiedKey === 'codes' ? 'Copied' : 'Copy all'}
              </button>
              <button class="btn primary" type="button" on:click={dismissBackupCodes}>
                I saved them
              </button>
            </div>
            <p class="muted small-text">
              {backupOrigin === 'activate' ? '2FA is now enabled.' : 'New backup codes generated.'}
            </p>
          </div>
        {/if}

        <div class="card">
          <h3>Regenerate backup codes</h3>
          <p class="muted small-text">Requires a valid 6-digit code. Old codes stop working.</p>
          <label for="regen-code">6-digit code</label>
          <input
            id="regen-code"
            class="totp"
            type="text"
            bind:value={regenCode}
            inputmode="numeric"
            autocomplete="one-time-code"
            placeholder="123456"
            maxlength="10"
            disabled={regenBusy}
          />
          {#if regenError}
            <p class="error small-text" role="alert">{regenError}</p>
          {/if}
          <div class="row-actions">
            <button
              class="btn"
              type="button"
              on:click={submitRegen}
              disabled={regenBusy || regenCode.trim().length < 6}
            >
              {regenBusy ? 'Generating…' : 'Generate new codes'}
            </button>
          </div>
        </div>

        <div class="card danger-card">
          <h3>Disable two-factor authentication</h3>
          <p class="muted small-text">
            Enter a current 6-digit code (or a backup code) to turn 2FA off for this account.
          </p>
          <label for="disable-code">Authentication code</label>
          <input
            id="disable-code"
            class="totp"
            type="text"
            bind:value={disableCode}
            inputmode="numeric"
            autocomplete="one-time-code"
            placeholder="123456"
            maxlength="24"
            disabled={disableBusy}
          />
          {#if disableError}
            <p class="error small-text" role="alert">{disableError}</p>
          {/if}
          {#if disableNotice}
            <p class="ok-text small-text" role="status">{disableNotice}</p>
          {/if}
          <div class="row-actions">
            <button
              class="btn danger"
              type="button"
              on:click={submitDisable}
              disabled={disableBusy || disableCode.trim().length < 6}
            >
              {disableBusy ? 'Disabling…' : 'Disable 2FA'}
            </button>
          </div>
        </div>
      </section>
    {/if}
  </main>

  {#if mountTarget}
    <div class="modal-backdrop">
      <form
        class="modal card"
        role="dialog"
        aria-modal="true"
        aria-labelledby="mount-title"
        on:submit|preventDefault={submitMount}
      >
        <div class="sheet-grab" aria-hidden="true"></div>
        <h2 id="mount-title">Mount {mountTarget.name}</h2>
        <p class="muted mono small-text">{mountTarget.mountPoint}</p>
        <label for="passphrase">Passphrase</label>
        <input
          id="passphrase"
          type="password"
          bind:value={passphrase}
          bind:this={passInput}
          autocomplete="current-password"
          disabled={mountBusy}
        />
        {#if mountError}
          <p class="error" role="alert">{mountError}</p>
        {/if}
        <div class="modal-actions">
          <button class="btn" type="button" on:click={closeMount} disabled={mountBusy}>Cancel</button>
          <button class="btn primary" type="submit" disabled={mountBusy || !passphrase}>
            {mountBusy ? 'Mounting…' : 'Mount'}
          </button>
        </div>
      </form>
    </div>
  {/if}

  {#if qrOpen}
    <div class="modal-backdrop" role="presentation" on:click|self={closeQr}>
      <div class="modal card qr-modal" role="dialog" aria-modal="true" aria-labelledby="qr-title">
        <div class="sheet-grab" aria-hidden="true"></div>
        <h2 id="qr-title">手机登录</h2>
        <p class="muted small-text">
          Scan this code with your phone, or open the link on a device on the same network.
        </p>
        {#if qr}
          <div class="qr-box" aria-label="One-time login QR code">
            {@html qr.svg}
          </div>
          <div class="copy-row">
            <input class="url-input mono" type="text" readonly value={qr.url} aria-label="Login URL" />
            <button class="btn small" type="button" on:click={copyQrUrl}>
              {qrCopied ? 'Copied' : 'Copy'}
            </button>
          </div>
        {:else if qrBusy}
          <p class="muted small-text">Generating a one-time link…</p>
        {/if}
        {#if qr && qrLeft > 0}
          <p class="muted small-text">Link expires in {qrLeft}s · single use.</p>
        {:else if qr}
          <p class="error small-text" role="status">This link has expired.</p>
        {/if}
        {#if qrError}
          <p class="error small-text" role="alert">{qrError}</p>
        {/if}
        <div class="modal-actions">
          <button class="btn" type="button" on:click={closeQr}>Close</button>
          <button class="btn primary" type="button" on:click={openQr} disabled={qrBusy}>
            {qrBusy ? 'Creating…' : qrLeft > 0 ? 'New link' : 'Create new link'}
          </button>
        </div>
      </div>
    </div>
  {/if}
{/if}
