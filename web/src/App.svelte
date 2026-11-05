<script>
  import { onMount, onDestroy, tick } from 'svelte';
  import * as api from './api.js';

  const TABS = [
    { id: 'vaults', label: 'Vaults' },
    { id: 'logs', label: 'Logs' },
    { id: 'meta', label: 'Meta' }
  ];

  const LOOPBACK_HOSTS = ['localhost', '127.0.0.1', '::1', '[::1]'];
  const isLoopback =
    typeof location !== 'undefined' && LOOPBACK_HOSTS.includes(location.hostname);

  // session
  let checking = true;
  let authed = false;
  let token = '';
  let loginError = '';
  let loginBusy = false;

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

  $: metaPaths = (meta && meta.paths) || {};
  $: metaDeps = (meta && (meta.deps || meta.dependencies)) || {};

  onMount(() => {
    bootstrap();
  });

  onDestroy(() => {
    closeAllStreams();
  });

  async function bootstrap() {
    checking = true;
    try {
      vaults = await api.getVaults();
      authed = true;
    } catch (err) {
      authed = false;
      if (err.status !== 401) loginError = err.message;
    } finally {
      checking = false;
    }
  }

  function normalizeVault(v) {
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

  async function submitLogin() {
    loginBusy = true;
    loginError = '';
    try {
      await api.login(token.trim());
      authed = true;
      token = '';
      tab = 'vaults';
      await loadVaults();
    } catch (err) {
      if (err.status === 401) loginError = 'Invalid token.';
      else loginError = err.message;
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
    jobs = {};
    vaults = [];
    logs = [];
    meta = null;
    authed = false;
    token = '';
    loginError = '';
    checking = false;
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
    if (ev.key === 'Escape' && mountTarget && !mountBusy) closeMount();
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
      {#if loginError}
        <p class="error" role="alert">{loginError}</p>
      {/if}
      <button class="btn primary" type="submit" disabled={loginBusy || !token.trim()}>
        {loginBusy ? 'Signing in…' : 'Sign in'}
      </button>
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
      <button class="btn ghost" type="button" on:click={doLogout}>Logout</button>
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
                    <div class="bar" role="progressbar" aria-valuemin="0" aria-valuemax="100" aria-valuenow={Math.round(job.pct)}>
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
            <table>
              <thead>
                <tr>
                  <th>Time</th>
                  <th>Src</th>
                  <th>Action</th>
                  <th>Target</th>
                  <th>Result</th>
                  <th>Detail</th>
                </tr>
              </thead>
              <tbody>
                {#each logs as row, i}
                  <tr>
                    <td class="nowrap">{fmtTime(row.time ?? row.ts ?? row.timestamp)}</td>
                    <td>{row.src ?? row.source ?? '—'}</td>
                    <td>{row.action ?? '—'}</td>
                    <td class="mono">{row.target ?? '—'}</td>
                    <td>
                      <span
                        class="badge"
                        class:on={row.result === 'ok' || row.result === 'success' || row.result === true}
                        class:off={row.result === 'error' || row.result === 'fail' || row.result === false}
                      >
                        {row.result ?? '—'}
                      </span>
                    </td>
                    <td class="detail">{row.detail ?? ''}</td>
                  </tr>
                {/each}
              </tbody>
            </table>
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
{/if}
