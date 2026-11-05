<script>
  import { onMount, onDestroy, tick } from 'svelte';
  import * as api from './api.js';
  import {
    themeMode,
    initTheme,
    cycleTheme,
    themeIcon
  } from './theme.js';
  import { detectLang, makeT, saveLang, t as translate } from './i18n.js';

  const TABS = [
    { id: 'vaults', labelKey: 'nav.vaults' },
    { id: 'logs', labelKey: 'nav.logs' },
    { id: 'meta', labelKey: 'nav.meta' },
    { id: 'security', labelKey: 'nav.security' }
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

  // one-time login link (phone sign-in)
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
  // 2FA status: prompt after sign-in when not enabled yet (default: off).
  let twofaStatus = null;
  let twofaDismissed = (() => {
    try {
      return localStorage.getItem('gfs-2fa-dismissed') === '1';
    } catch (_) {
      return false;
    }
  })();

  async function loadTwofaStatus() {
    try {
      twofaStatus = await api.get2faStatus();
    } catch (_) {
      twofaStatus = null;
    }
  }

  function dismissTwofaPrompt() {
    twofaDismissed = true;
    try {
      localStorage.setItem('gfs-2fa-dismissed', '1');
    } catch (_) {
      /* ignore */
    }
  }
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

  // --- i18n ----------------------------------------------------------------

  let lang = detectLang();
  $: tr = makeT(lang);
  $: nextTheme = theme === 'auto' ? 'light' : theme === 'light' ? 'dark' : 'auto';
  $: applyLang(lang);

  function applyLang(code) {
    if (typeof document === 'undefined') return;
    document.documentElement.lang = code === 'zh' ? 'zh-CN' : 'en';
    document.title = translate(code, 'app.title');
  }

  function toggleLang() {
    lang = lang === 'zh' ? 'en' : 'zh';
    saveLang(lang);
    applyLang(lang);
  }

  // Localized error state. Known failures keep a key (so a language switch
  // re-renders them); raw server text is shown as-is.
  function trErr(key, vars) {
    return { key, vars: vars || null };
  }

  function rawErr(text) {
    return { raw: text };
  }

  function errText(value, translateFn) {
    if (!value) return '';
    if (typeof value === 'string') return value;
    if (value.key) return translateFn(value.key, value.vars);
    return value.raw || '';
  }

  function apiErr(e, fallbackKey) {
    if (e && e.status === 0) return trErr('login.serverUnreachable');
    if (e && e.message) return rawErr(e.message);
    return trErr(fallbackKey || 'common.error');
  }

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
            ? trErr('login.linkExpired')
            : apiErr(err, 'login.failed');
      } finally {
        // Never keep a consumed/expired nonce in the URL.
        stripHash();
      }
    }

    try {
      vaults = (await api.getVaults()).map(normalizeVault);
      authed = true;
      loginError = '';
      loadTwofaStatus();
    } catch (err) {
      authed = false;
      if (err.status !== 401 && !loginError) loginError = apiErr(err, 'login.failed');
    } finally {
      checking = false;
    }
  }

  // After running `gocryptfs-web user add <name>` on the server, refetching the
  // status is enough; no restart is needed.
  async function recheckAuth() {
    try {
      authStatus = await api.authStatus();
      loginError = authStatus.password_auth ? '' : trErr('login.noUsers');
    } catch (e) {
      loginError = trErr('login.serverUnreachable');
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
      vaultsError = apiErr(err, 'vaults.loadFailed');
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
      logsError = apiErr(err, 'logs.loadFailed');
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
      metaError = apiErr(err, 'meta.loadFailed');
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
    loginError = trErr('login.expired');
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
    if (err.status === 429) return trErr('login.rateLimited');
    if (err.status === 401) {
      const msg = String(err.message || '').toLowerCase();
      if (msg.includes('totp_required') || msg.includes('totp required')) return '';
      if (msg.includes('totp') || msg.includes('backup')) return trErr('login.invalidCode');
      return loginMode === 'token' ? trErr('login.invalidToken') : trErr('login.invalidCredentials');
    }
    if (err.status === 0) return trErr('login.serverUnreachable');
    return trErr('login.failed');
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
      loadTwofaStatus();
    } catch (err) {
      const msg = String(err.message || '').toLowerCase();
      if (err.status === 401 && msg.includes('totp')) {
        totpRequired = true;
        loginError = msg.includes('required')
          ? trErr('login.totpRequired')
          : trErr('login.invalidCodeTry');
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
      if (!data || !data.svg) {
        qrError = trErr('login.qrMissing');
        return;
      }
      qr = data;
      qrLeft = Number(data.expires_in) || 120;
      startQrTimer();
    } catch (err) {
      qrError = apiErr(err, 'login.qrError');
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
      if (!data.secret || !data.svg) {
        enrollError = trErr('security.serverMissing');
        return;
      }
      enroll = data;
      enrollCode = '';
    } catch (err) {
      if (err.status === 401) return onUnauthorized();
      enrollError = apiErr(err, 'security.enrollFailed');
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
        enrollError = trErr('security.invalidCodeTry');
      } else if (err.status === 401) {
        return onUnauthorized();
      } else {
        enrollError = apiErr(err, 'security.activateFailed');
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
      disableNotice = trErr('security.disabled');
    loadTwofaStatus();
    loadTwofaStatus();
    } catch (err) {
      if (err.status === 401 && String(err.message || '').includes('totp')) {
        disableError = trErr('security.invalidCode');
      } else {
        disableError = apiErr(err, 'security.disableFailed');
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
        regenError = trErr('security.invalidCode');
      } else {
        regenError = apiErr(err, 'security.regenFailed');
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
        error: code === 0 ? '' : trErr('job.exited', { code })
      });
      closeStream(name);
      loadVaults();
    });

    es.addEventListener('error', (ev) => {
      if (ev.data) {
        const d = parseEvent(ev);
        patchJob(name, {
          active: false,
          error: d.message ? rawErr(String(d.message)) : trErr('job.failed')
        });
        closeStream(name);
        loadVaults();
      } else {
        patchJob(name, {
          active: false,
          error: trErr('job.lostConnection')
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
      const jobId = await api.mountVault(name, passphrase, lang === 'zh' ? 'zh-CN' : 'en-US');
      mountTarget = null;
      passphrase = '';
      if (jobId) startJob(name, jobId);
      else await loadVaults();
    } catch (err) {
      if (err.status === 401) return onUnauthorized();
      mountError = apiErr(err, 'mount.failed');
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
      const jobId = await api.umountVault(name, lang === 'zh' ? 'zh-CN' : 'en-US');
      if (jobId) startJob(name, jobId);
      else {
        patchJob(name, { active: false, pct: 100 });
        await loadVaults();
      }
    } catch (err) {
      if (err.status === 401) return onUnauthorized();
      patchJob(name, { active: false, error: apiErr(err, 'job.failed') });
    }
  }

  function isBusy(name) {
    return !!(jobs[name] && jobs[name].active);
  }

  // --- formatting ----------------------------------------------------------

  function fmtTime(t, code) {
    const locale = code === 'zh' ? 'zh-CN' : 'en-US';
    const local = (d) => (isNaN(d.getTime()) ? String(t) : d.toLocaleString(locale));
    if (t === null || t === undefined || t === '') return '—';
    if (typeof t === 'number') {
      const d = new Date(t < 1e12 ? t * 1000 : t);
      return local(d);
    }
    const d = new Date(t);
    return local(d);
  }

  function depInfo(v) {
    if (v === true) return { ok: true, text: 'ok', kind: 'ok' };
    if (v === false) return { ok: false, text: 'missing', kind: 'missing' };
    if (v && typeof v === 'object') {
      const ok = v.ok !== undefined ? v.ok : v.available !== undefined ? v.available : v.found;
      const detail = v.version || v.path;
      const kind = detail ? 'value' : ok === false ? 'missing' : ok === true ? 'ok' : 'unknown';
      const text = detail || (kind === 'missing' ? 'missing' : kind === 'ok' ? 'ok' : 'unknown');
      return { ok: ok !== false, text: String(text), kind };
    }
    if (v === null || v === undefined) return { ok: true, text: 'unknown', kind: 'unknown' };
    return { ok: true, text: String(v), kind: 'value' };
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
    {tr('banner.insecure')}
  </div>
{/if}

{#if checking}
  <div class="boot">{tr('common.loading')}</div>
{:else if !authed}
  <main class="login-wrap">
    <form class="card login" on:submit|preventDefault={submitLogin}>
      <div class="lang-switch">
        <button
          class="btn ghost small"
          type="button"
          on:click={toggleLang}
          title={tr(lang === 'zh' ? 'nav.switchToEnglish' : 'nav.switchToChinese')}
          aria-label={tr(lang === 'zh' ? 'nav.switchToEnglish' : 'nav.switchToChinese')}
        >
          {lang === 'zh' ? tr('nav.langToggleToEn') : tr('nav.langToggleToZh')}
        </button>
      </div>
      <h1>{tr('app.title')}</h1>

      {#if !showPasswordTab}
        <p class="muted small-text">
          {tr('login.disabledHint')}
          <code>gocryptfs-web user add &lt;name&gt;</code>{tr('login.disabledHintMid')}<button
            type="button"
            class="link-btn"
            on:click={recheckAuth}>{tr('login.recheck')}</button
          >{tr('login.disabledHintEnd')}
        </p>
      {/if}

      {#if showPasswordTab && authStatus.token_auth}
        <div class="seg" role="tablist" aria-label={tr('login.methodAria')}>
          <button
            type="button"
            class="seg-btn"
            class:active={loginMode === 'token'}
            role="tab"
            aria-selected={loginMode === 'token'}
            on:click={() => switchLoginMode('token')}
          >
            {tr('login.tokenTab')}
          </button>
          <button
            type="button"
            class="seg-btn"
            class:active={loginMode === 'password'}
            role="tab"
            aria-selected={loginMode === 'password'}
            on:click={() => switchLoginMode('password')}
          >
            {tr('login.passwordTab')}
          </button>
        </div>
      {/if}

      {#if loginMode === 'token'}
        <p class="muted">{tr('login.tokenHint')}</p>
        <label for="token">{tr('login.tokenLabel')}</label>
        <input
          id="token"
          type="password"
          bind:value={token}
          autocomplete="current-password"
          placeholder={tr('login.tokenPlaceholder')}
          disabled={loginBusy}
        />
      {:else}
        <p class="muted">{tr('login.passwordHint')}</p>
        <label for="username">{tr('login.username')}</label>
        <input
          id="username"
          type="text"
          bind:value={username}
          autocomplete="username"
          autocapitalize="none"
          spellcheck="false"
          disabled={loginBusy}
        />
        <label for="password">{tr('login.password')}</label>
        <input
          id="password"
          type="password"
          bind:value={password}
          autocomplete="current-password"
          disabled={loginBusy}
        />
        {#if totpRequired}
          <label for="totp">{tr('login.totpLabel')}</label>
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
            placeholder={tr('login.totpPlaceholder')}
            maxlength="24"
            disabled={loginBusy}
          />
          <p class="muted small-text">{tr('login.totpHint')}</p>
        {/if}
      {/if}

      {#if loginError}
        <p class="error" role="alert">{errText(loginError, tr)}</p>
      {/if}

      <button
        class="btn primary block"
        type="submit"
        disabled={loginBusy ||
          (loginMode === 'token' ? !token.trim() : !username.trim() || !password)}
      >
        {loginBusy ? tr('login.busy') : tr('login.submit')}
      </button>

      {#if authStatus['2fa_available']}
        <p class="muted small-text center">{tr('login.backupHint')}</p>
      {/if}
    </form>
  </main>
{:else}
  <header class="topbar">
    <div class="topbar-inner">
      <div class="brand">{tr('app.title')}</div>
      <nav class="tabs" aria-label={tr('nav.pagesAria')}>
        {#each TABS as t}
          <button
            type="button"
            class="tab"
            class:active={tab === t.id}
            aria-current={tab === t.id ? 'page' : undefined}
            on:click={() => selectTab(t.id)}
          >
            {tr(t.labelKey)}
          </button>
        {/each}
      </nav>
      <div class="topbar-actions">
        <button
          class="btn ghost small lang-toggle"
          type="button"
          on:click={toggleLang}
          title={tr(lang === 'zh' ? 'nav.switchToEnglish' : 'nav.switchToChinese')}
          aria-label={tr(lang === 'zh' ? 'nav.switchToEnglish' : 'nav.switchToChinese')}
        >
          {lang === 'zh' ? tr('nav.langToggleToEn') : tr('nav.langToggleToZh')}
        </button>
        <button
          class="btn ghost small theme-toggle"
          type="button"
          on:click={cycleTheme}
          title={tr('theme.title', { mode: tr('theme.' + theme) })}
          aria-label={tr('theme.aria', {
            mode: tr('theme.' + theme),
            next: tr('theme.' + nextTheme)
          })}
        >
          <span class="theme-icon" aria-hidden="true">{themeIcon(theme)}</span>
          <span class="theme-label">{tr('theme.' + theme)}</span>
        </button>
        <button
          class="btn ghost small"
          type="button"
          on:click={openQr}
          disabled={qrBusy}
          title={tr('login.qrButton')}
          aria-label={tr('login.qrButton')}
        >
          {qrBusy ? '…' : tr('nav.phoneLogin')}
        </button>
        <button class="btn ghost small" type="button" on:click={doLogout}>{tr('nav.logout')}</button>
      </div>
    </div>
  </header>

  {#if twofaStatus && !twofaStatus.enabled && !twofaDismissed}
    <div class="notice twofa-prompt" role="status">
      <div>
        <strong>{tr('twofa.promptTitle')}</strong>
        <span class="muted small-text">
          {twofaStatus.can_enable ? tr('twofa.promptBody') : tr('twofa.promptBodyToken')}
        </span>
      </div>
      <div class="row-actions">
        {#if twofaStatus.can_enable}
          <button class="btn primary" type="button" on:click={() => selectTab('security')}>
            {tr('twofa.enableNow')}
          </button>
        {/if}
        <button class="btn" type="button" on:click={dismissTwofaPrompt}>
          {tr('twofa.later')}
        </button>
      </div>
    </div>
  {/if}

  <main class="content">
    {#if tab === 'vaults'}
      <section>
        <div class="section-head">
          <h2>{tr('vaults.title')}</h2>
          <button class="btn" type="button" on:click={loadVaults} disabled={vaultsLoading}>
            {vaultsLoading ? tr('common.refreshing') : tr('common.refresh')}
          </button>
        </div>

        {#if vaultsError}
          <div class="notice error" role="alert">
            <span>{errText(vaultsError, tr)}</span>
            <button class="btn small" type="button" on:click={loadVaults}>{tr('common.retry')}</button>
          </div>
        {/if}

        {#if !vaults.length && !vaultsLoading}
          <p class="muted empty">{tr('vaults.empty')}</p>
        {:else}
          <div class="vault-grid">
            {#each vaults as vault (vault.name)}
              {@const job = jobs[vault.name]}
              <article class="card vault">
                <header class="vault-head">
                  <h3>{vault.name}</h3>
                  <span class="badge" class:on={vault.mounted} class:off={!vault.mounted}>
                    {vault.mounted ? tr('vaults.mounted') : tr('vaults.notMounted')}
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
                    {tr('vaults.mount')}
                  </button>
                  <button
                    class="btn"
                    type="button"
                    disabled={!vault.mounted || isBusy(vault.name)}
                    on:click={() => doUmount(vault)}
                  >
                    {tr('vaults.umount')}
                  </button>
                </div>

                {#if job}
                  <div class="job">
                    <div
                      class="bar"
                      role="progressbar"
                      aria-label={tr('job.progress')}
                      aria-valuemin="0"
                      aria-valuemax="100"
                      aria-valuenow={Math.round(job.pct)}
                    >
                      <div class="bar-fill" class:failed={!!job.error} style={`width:${job.pct}%`}></div>
                    </div>
                    <div class="job-meta">
                      <span>{Math.round(job.pct)}%</span>
                      {#if job.total > 0}<span>{job.done}/{job.total}</span>{/if}
                      {#if job.active}<span class="muted">{tr('job.running')}</span>{/if}
                      {#if job.code !== null && !job.error}<span class="ok-text">{tr('job.done')}</span>{/if}
                    </div>
                    {#if job.error}
                      <p class="error small-text" role="alert">{errText(job.error, tr)}</p>
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
          <h2>{tr('logs.title')}</h2>
          <button class="btn" type="button" on:click={loadLogs} disabled={logsLoading}>
            {logsLoading ? tr('common.refreshing') : tr('logs.refresh')}
          </button>
        </div>

        {#if logsError}
          <div class="notice error" role="alert">
            <span>{errText(logsError, tr)}</span>
            <button class="btn small" type="button" on:click={loadLogs}>{tr('common.retry')}</button>
          </div>
        {/if}

        {#if !logs.length && !logsLoading}
          <p class="muted empty">{tr('logs.empty')}</p>
        {:else}
          <div class="table-wrap">
            <div class="logs-table" role="table" aria-label={tr('logs.titleAria')}>
              <div class="log-row log-head" role="row">
                <span class="cell when" role="columnheader">{tr('logs.time')}</span>
                <span class="cell src" role="columnheader">{tr('logs.source')}</span>
                <span class="cell action" role="columnheader">{tr('logs.action')}</span>
                <span class="cell target" role="columnheader">{tr('logs.target')}</span>
                <span class="cell result" role="columnheader">{tr('logs.result')}</span>
                <span class="cell detail" role="columnheader">{tr('logs.detail')}</span>
              </div>
              {#each logs as row, i (i)}
                {@const tone = resultTone(row.result)}
                <div class="log-row" role="row">
                  <span class="cell when nowrap" role="cell">
                    {fmtTime(row.time ?? row.ts ?? row.timestamp, lang)}
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
          <h2>{tr('meta.title')}</h2>
          <button class="btn" type="button" on:click={loadMeta} disabled={metaLoading}>
            {metaLoading ? tr('common.refreshing') : tr('common.refresh')}
          </button>
        </div>

        {#if metaError}
          <div class="notice error" role="alert">
            <span>{errText(metaError, tr)}</span>
            <button class="btn small" type="button" on:click={loadMeta}>{tr('common.retry')}</button>
          </div>
        {/if}

        {#if meta}
          <div class="card">
            <h3>{tr('meta.version')}</h3>
            <p class="mono">{meta.version ?? meta.version_string ?? '—'}</p>
          </div>

          <div class="card">
            <h3>{tr('meta.paths')}</h3>
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
            <h3>{tr('meta.deps')}</h3>
            {#if Object.keys(metaDeps).length}
              <ul class="deps">
                {#each Object.entries(metaDeps) as [name, value]}
                  {@const info = depInfo(value)}
                  <li>
                    <span class="dep-name">{name}</span>
                    <span class="badge" class:on={info.ok} class:off={!info.ok}>{info.kind === 'ok'
                        ? tr('meta.depsOk')
                        : info.kind === 'missing'
                          ? tr('meta.depsMissing')
                          : info.kind === 'unknown'
                            ? tr('meta.unknown')
                            : info.text}</span>
                  </li>
                {/each}
              </ul>
            {:else}
              <p class="muted">{tr('meta.noDeps')}</p>
            {/if}
          </div>
        {/if}
      </section>
    {/if}

    {#if tab === 'security'}
      <section>
        <div class="section-head">
          <h2>{tr('security.title')}</h2>
          <span class="badge" class:on={!!backupCodes} class:off={!backupCodes}>
            {backupCodes ? tr('security.codesShown') : tr('security.badge')}
          </span>
        </div>

        {#if twofaStatus}
          <p class="muted small-text">
            {twofaStatus.enabled ? tr('twofa.statusOn') : tr('twofa.statusOff')}
            {#if twofaStatus.enabled && twofaStatus.backup_codes > 0}
              · {tr('twofa.backupLeft', { n: twofaStatus.backup_codes })}
            {/if}
            {#if !twofaStatus.can_enable}
              · {tr('twofa.promptBodyToken')}
            {/if}
            {#if !twofaStatus.enabled}
              · {tr('twofa.defaultNote')}
            {/if}
          </p>
        {/if}

        {#if !authStatus['2fa_available']}
          <div class="notice error" role="status">
            <span>{tr('security.unavailable')}</span>
          </div>
        {/if}

        <div class="card">
          <h3>{tr('security.twoFactor')}</h3>
          <p class="muted small-text">{tr('security.enrollHint')}</p>

          {#if !enroll}
            <div class="row-actions">
              <button class="btn primary" type="button" on:click={startEnroll} disabled={enrollBusy}>
                {enrollBusy ? tr('security.preparing') : tr('security.enroll')}
              </button>
            </div>
          {:else}
            <div class="enroll">
              <div class="qr-box" aria-label={tr('security.qrAria')}>
                {@html enroll.svg}
              </div>
              <div class="enroll-info">
                <p class="muted small-text">{tr('security.activateHint')}</p>
                <div class="copy-row">
                  <code class="secret" aria-label={tr('security.secret')}>{enroll.secret}</code>
                  <button
                    class="btn small"
                    type="button"
                    on:click={() => copyValue(enroll.secret, 'secret')}
                  >
                    {copiedKey === 'secret' ? tr('security.copied') : tr('common.copy')}
                  </button>
                </div>
                <label for="enroll-code">{tr('security.codeLabel')}</label>
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
                  <p class="error small-text" role="alert">{errText(enrollError, tr)}</p>
                {/if}
                <div class="row-actions">
                  <button class="btn" type="button" on:click={cancelEnroll} disabled={enrollBusy}>
                    {tr('common.cancel')}
                  </button>
                  <button
                    class="btn primary"
                    type="button"
                    on:click={activateEnroll}
                    disabled={enrollBusy || enrollCode.trim().length < 6}
                  >
                    {enrollBusy ? tr('security.verifying') : tr('security.activate')}
                  </button>
                </div>
              </div>
            </div>
          {/if}
        </div>

        {#if backupCodes}
          <div class="card backup-card">
            <h3>{tr('security.backupCodes')}</h3>
            <p class="warn-text" role="alert">{tr('security.backupHint')}</p>
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
                {copiedKey === 'codes' ? tr('security.copied') : tr('security.copyAll')}
              </button>
              <button class="btn primary" type="button" on:click={dismissBackupCodes}>
                {tr('security.saved')}
              </button>
            </div>
            <p class="muted small-text">
              {backupOrigin === 'activate' ? tr('security.enabled') : tr('security.generated')}
            </p>
          </div>
        {/if}

        <div class="card">
          <h3>{tr('security.regenerate')}</h3>
          <p class="muted small-text">{tr('security.regenerateHint')}</p>
          <label for="regen-code">{tr('security.codeLabel')}</label>
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
            <p class="error small-text" role="alert">{errText(regenError, tr)}</p>
          {/if}
          <div class="row-actions">
            <button
              class="btn"
              type="button"
              on:click={submitRegen}
              disabled={regenBusy || regenCode.trim().length < 6}
            >
              {regenBusy ? tr('security.generating') : tr('security.generate')}
            </button>
          </div>
        </div>

        <div class="card danger-card">
          <h3>{tr('security.disableTitle')}</h3>
          <p class="muted small-text">{tr('security.disableHint')}</p>
          <label for="disable-code">{tr('security.disableAuthLabel')}</label>
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
            <p class="error small-text" role="alert">{errText(disableError, tr)}</p>
          {/if}
          {#if disableNotice}
            <p class="ok-text small-text" role="status">{errText(disableNotice, tr)}</p>
          {/if}
          <div class="row-actions">
            <button
              class="btn danger"
              type="button"
              on:click={submitDisable}
              disabled={disableBusy || disableCode.trim().length < 6}
            >
              {disableBusy ? tr('security.disabling') : tr('security.disable')}
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
        <h2 id="mount-title">{tr('mount.title', { name: mountTarget.name })}</h2>
        <p class="muted mono small-text">{mountTarget.mountPoint}</p>
        <label for="passphrase">{tr('mount.passphrase')}</label>
        <input
          id="passphrase"
          type="password"
          bind:value={passphrase}
          bind:this={passInput}
          autocomplete="current-password"
          disabled={mountBusy}
        />
        {#if mountError}
          <p class="error" role="alert">{errText(mountError, tr)}</p>
        {/if}
        <div class="modal-actions">
          <button class="btn" type="button" on:click={closeMount} disabled={mountBusy}>
            {tr('mount.cancel')}
          </button>
          <button class="btn primary" type="submit" disabled={mountBusy || !passphrase}>
            {mountBusy ? tr('mount.mounting') : tr('mount.confirm')}
          </button>
        </div>
      </form>
    </div>
  {/if}

  {#if qrOpen}
    <div class="modal-backdrop" role="presentation" on:click|self={closeQr}>
      <div class="modal card qr-modal" role="dialog" aria-modal="true" aria-labelledby="qr-title">
        <div class="sheet-grab" aria-hidden="true"></div>
        <h2 id="qr-title">{tr('login.qrTitle')}</h2>
        <p class="muted small-text">{tr('login.qrHint')}</p>
        {#if qr}
          <div class="qr-box" aria-label={tr('login.qrAria')}>
            {@html qr.svg}
          </div>
          <div class="copy-row">
            <input
              class="url-input mono"
              type="text"
              readonly
              value={qr.url}
              aria-label={tr('login.qrUrlAria')}
            />
            <button class="btn small" type="button" on:click={copyQrUrl}>
              {qrCopied ? tr('qr.copied') : tr('qr.copy')}
            </button>
          </div>
        {:else if qrBusy}
          <p class="muted small-text">{tr('qr.generating')}</p>
        {/if}
        {#if qr && qrLeft > 0}
          <p class="muted small-text">{tr('qr.expires', { seconds: qrLeft })}</p>
        {:else if qr}
          <p class="error small-text" role="status">{tr('qr.expired')}</p>
        {/if}
        {#if qrError}
          <p class="error small-text" role="alert">{errText(qrError, tr)}</p>
        {/if}
        <div class="modal-actions">
          <button class="btn" type="button" on:click={closeQr}>{tr('common.close')}</button>
          <button class="btn primary" type="button" on:click={openQr} disabled={qrBusy}>
            {qrBusy ? tr('qr.creating') : qrLeft > 0 ? tr('qr.newLink') : tr('qr.createLink')}
          </button>
        </div>
      </div>
    </div>
  {/if}
{/if}
