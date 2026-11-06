// Tiny dependency-free i18n for the gocryptfs-ui Web UI.
//
// - `en` / `zh` catalogs must always share the same key set (`assertParity`).
// - `t(lang, key, vars)` supports `{name}` interpolation.
// - The choice is persisted in localStorage["gfs-lang"].
// - Keys are semantic; technical/protocol strings (paths, versions, raw
//   server messages) intentionally stay untranslated.

export const LANGS = ['en', 'zh'];

export const LANG_KEY = 'gfs-lang';

const en = {
  // app shell ---------------------------------------------------------------
  'app.title': 'gocryptfs-ui',
  'banner.insecure':
    'You are not accessing this UI over loopback. The session cookie may be exposed on the network.',

  // generic -----------------------------------------------------------------
  'common.loading': 'Loading…',
  'common.refresh': 'Refresh',
  'common.refreshing': 'Refreshing…',
  'common.retry': 'Retry',
  'common.copy': 'Copy',
  'common.copied': 'Copied',
  // two-factor authentication prompt ---------------------------------------
  'twofa.promptTitle': 'Two-factor authentication is off',
  'twofa.promptBody':
    'Enable TOTP 2FA so a stolen token or password alone cannot control your vaults.',
  'twofa.promptBodyToken':
    '2FA is tied to username/password accounts. Sign in with a user account to enable it.',
  'twofa.enableNow': 'Enable now',
  'twofa.later': 'Later',
  'twofa.statusOn': '2FA is enabled.',
  'twofa.statusOff': '2FA is currently not enabled (default).',
  'twofa.backupLeft': '{n} backup codes left',
  'twofa.defaultNote': 'New accounts start with 2FA disabled; enable it below.',
  'common.cancel': 'Cancel',
  'common.confirm': 'Confirm',
  'common.close': 'Close',
  'common.error': 'Something went wrong. Please try again.',
  'common.unknown': 'unknown',

  // login -------------------------------------------------------------------
  'login.methodAria': 'Sign-in method',
  'login.tokenTab': 'Token',
  'login.passwordTab': 'Username & password',
  'login.tokenHint': 'Enter the access token to unlock this console.',
  'login.tokenLabel': 'Access token',
  'login.tokenPlaceholder': 'session token',
  'login.passwordHint': 'Sign in with your account credentials.',
  'login.username': 'Username',
  'login.password': 'Password',
  'login.totpLabel': 'Authentication code',
  'login.totpPlaceholder': '123456 or backup code',
  'login.totpHint':
    'Two-factor authentication is enabled. Enter the 6-digit code from your authenticator app, or one of your backup codes.',
  'login.submit': 'Sign in',
  'login.busy': 'Signing in…',
  'login.backupHint':
    'Lost your device? Use a backup code, or ask the host operator for a one-time login link.',
  'login.disabledHint':
    'Username / password sign-in is disabled because no user exists yet. On the server run',
  'login.disabledHintMid': '(takes effect immediately), then ',
  'login.disabledHintEnd': '.',
  'login.recheck': 'check again',
  'login.invalidToken': 'Invalid token.',
  'login.invalidCredentials': 'Invalid username or password.',
  'login.invalidCode': 'Invalid authentication code.',
  'login.invalidCodeTry': 'That code is not valid. Check your app and try again.',
  'login.totpRequired': 'Enter the 6-digit code from your authenticator app.',
  'login.rateLimited': 'Too many attempts. Please wait a minute and try again.',
  'login.serverUnreachable': 'Cannot reach the server. Check that it is running.',
  'login.expired': 'Session expired. Please sign in again.',
  'login.linkExpired': 'This one-time login link is invalid or has expired.',
  'login.failed': 'Sign-in failed.',
  'login.noUsers': 'Still no password users. Run `gocryptfs-web user add <name>` on the server.',
  'login.qrTitle': 'Phone sign-in',
  'login.qrButton': 'Phone sign-in',
  'login.qrHint':
    'Scan this code with your phone, or open the link on a device on the same network.',
  'login.qrAria': 'One-time login QR code',
  'login.qrUrlAria': 'Login URL',
  'login.qrError': 'Could not create a login link.',
  'login.qrMissing': 'Server did not return a login QR code.',

  // one-time link (QR modal) ------------------------------------------------
  'qr.expired': 'This link has expired.',
  'qr.copy': 'Copy',
  'qr.copied': 'Copied',
  'qr.expires': 'Link expires in {seconds}s · single use.',
  'qr.generating': 'Generating a one-time link…',
  'qr.creating': 'Creating…',
  'qr.newLink': 'New link',
  'qr.createLink': 'Create new link',

  // navigation --------------------------------------------------------------
  'nav.vaults': 'Vaults',
  'nav.logs': 'Logs',
  'nav.meta': 'Meta',
  'nav.security': 'Security',
  'nav.logout': 'Logout',
  'nav.phoneLogin': 'Phone sign-in',
  'nav.pagesAria': 'Pages',
  'nav.switchToChinese': 'Switch to Chinese',
  'nav.switchToEnglish': 'Switch to English',
  'nav.langToggleToZh': '中文',
  'nav.langToggleToEn': 'EN',

  // theme -------------------------------------------------------------------
  'theme.auto': 'Auto',
  'theme.light': 'Light',
  'theme.dark': 'Dark',
  'theme.title': 'Theme: {mode} (click to change)',
  'theme.aria': 'Theme: {mode}. Click to switch to {next}.',

  // vaults ------------------------------------------------------------------
  'vaults.title': 'Vaults',
  'vaults.name': 'Name',
  'vaults.mountPoint': 'Mount point',
  'vaults.status': 'Status',
  'vaults.mounted': 'mounted',
  'vaults.notMounted': 'not mounted',
  'vaults.mount': 'Mount',
  'vaults.umount': 'Umount',
  'vaults.empty': 'No vaults configured.',
  'vaults.loadFailed': 'Could not load vaults.',

  // mount dialog ------------------------------------------------------------
  'mount.title': 'Mount {name}',
  'mount.passphrase': 'Passphrase',
  'mount.cancel': 'Cancel',
  'mount.confirm': 'Mount',
  'mount.mounting': 'Mounting…',
  'mount.failed': 'Could not mount the vault.',

  // background jobs ---------------------------------------------------------
  'job.running': 'working…',
  'job.done': 'done',
  'job.failed': 'Job failed',
  'job.progress': 'Progress',
  'job.exited': 'Job exited with code {code}',
  'job.lostConnection': 'Lost connection to the job event stream',

  // logs --------------------------------------------------------------------
  'logs.title': 'Logs',
  'logs.time': 'Time',
  'logs.source': 'Src',
  'logs.action': 'Action',
  'logs.target': 'Target',
  'logs.result': 'Result',
  'logs.detail': 'Detail',
  'logs.refresh': 'Refresh',
  'logs.empty': 'No log entries.',
  'logs.titleAria': 'Log entries',
  'logs.loadFailed': 'Could not load logs.',

  // meta --------------------------------------------------------------------
  'meta.title': 'Meta',
  'meta.version': 'Version',
  'meta.paths': 'Paths',
  'meta.deps': 'Dependencies',
  'meta.depsOk': 'ok',
  'meta.depsMissing': 'missing',
  'meta.unknown': 'unknown',
  'meta.noDeps': 'No dependency information reported.',
  'meta.remote': 'Remote access',
  'meta.publicUrl': 'Public URL',
  'meta.notSet': 'not set',
  'meta.trustProxy': 'Trust proxy headers',
  'meta.trustProxyOn': 'yes',
  'meta.trustProxyOff': 'no',
  'meta.loadFailed': 'Could not load server information.',

  // security / 2FA ----------------------------------------------------------
  'security.title': 'Security',
  'security.badge': '2FA',
  'security.codesShown': '2FA codes shown',
  'security.unavailable':
    'This server build does not advertise two-factor authentication support.',
  'security.twoFactor': 'Two-factor authentication',
  'security.enroll': 'Set up 2FA',
  'security.enrollHint':
    'Add a second factor to your account. You will need an authenticator app such as Aegis, 1Password, or Google Authenticator.',
  'security.preparing': 'Preparing…',
  'security.qrAria': '2FA enrollment QR code',
  'security.secret': 'Secret key',
  'security.activateHint':
    'Scan the QR code with your authenticator app, or enter the secret key manually.',
  'security.codeLabel': '6-digit code',
  'security.verifying': 'Verifying…',
  'security.activate': 'Activate',
  'security.activateFailed': 'Activation failed.',
  'security.enabled': '2FA is now enabled.',
  'security.disabled': 'Two-factor authentication disabled.',
  'security.invalidCode': 'Invalid authentication code.',
  'security.invalidCodeTry': 'That code is not valid. Check your app and try again.',
  'security.backupCodes': 'Backup codes',
  'security.backupHint':
    'Save these codes now — they are shown only once. Each code can be used once if you lose access to your authenticator.',
  'security.copyAll': 'Copy all',
  'security.copied': 'Copied',
  'security.saved': 'I saved them',
  'security.generated': 'New backup codes generated.',
  'security.regenerate': 'Regenerate backup codes',
  'security.regenerateHint': 'Requires a valid 6-digit code. Old codes stop working.',
  'security.generating': 'Generating…',
  'security.generate': 'Generate new codes',
  'security.disable': 'Disable 2FA',
  'security.disableTitle': 'Disable two-factor authentication',
  'security.disableHint':
    'Enter a current 6-digit code (or a backup code) to turn 2FA off for this account.',
  'security.disableAuthLabel': 'Authentication code',
  'security.disabling': 'Disabling…',
  'security.enrollFailed': 'Enrollment failed.',
  'security.disableFailed': 'Could not disable 2FA.',
  'security.regenFailed': 'Could not generate new backup codes.',
  'security.serverMissing': 'Server did not return 2FA data.'
};

const zh = {
  // app shell ---------------------------------------------------------------
  'app.title': 'gocryptfs-ui',
  'banner.insecure': '你正在通过非回环地址访问此界面，会话 Cookie 可能暴露在网络上。',

  // generic -----------------------------------------------------------------
  'common.loading': '加载中…',
  'common.refresh': '刷新',
  'common.refreshing': '刷新中…',
  'common.retry': '重试',
  'common.copy': '复制',
  'common.copied': '已复制',
  // two-factor authentication prompt ---------------------------------------
  'twofa.promptTitle': '两步验证未启用',
  'twofa.promptBody': '建议启用 TOTP 两步验证：仅凭泄露的 token 或密码无法控制加密卷。',
  'twofa.promptBodyToken': '两步验证绑定用户名密码账户；请用用户账户登录后再启用。',
  'twofa.enableNow': '立即启用',
  'twofa.later': '稍后',
  'twofa.statusOn': '两步验证已启用。',
  'twofa.statusOff': '当前未启用两步验证（默认）。',
  'twofa.backupLeft': '剩余 {n} 个备份码',
  'twofa.defaultNote': '新账户默认不启用 2FA，可在下方开启。',
  'common.cancel': '取消',
  'common.confirm': '确认',
  'common.close': '关闭',
  'common.error': '出错了，请重试。',
  'common.unknown': '未知',

  // login -------------------------------------------------------------------
  'login.methodAria': '登录方式',
  'login.tokenTab': '令牌',
  'login.passwordTab': '用户名与密码',
  'login.tokenHint': '输入访问令牌以解锁此控制台。',
  'login.tokenLabel': '访问令牌',
  'login.tokenPlaceholder': '会话令牌',
  'login.passwordHint': '使用账户凭据登录。',
  'login.username': '用户名',
  'login.password': '密码',
  'login.totpLabel': '验证码',
  'login.totpPlaceholder': '123456 或备用码',
  'login.totpHint': '已启用双重验证。请输入身份验证器应用中的 6 位验证码，或使用备用码。',
  'login.submit': '登录',
  'login.busy': '登录中…',
  'login.backupHint': '设备丢失？可使用备用码，或请主机管理员生成一次性登录链接。',
  'login.disabledHint': '尚未创建用户，用户名 / 密码登录已禁用。请在服务器上运行',
  'login.disabledHintMid': '（立即生效），然后点击',
  'login.disabledHintEnd': '。',
  'login.recheck': '重新检查',
  'login.invalidToken': '令牌无效。',
  'login.invalidCredentials': '用户名或密码错误。',
  'login.invalidCode': '验证码无效。',
  'login.invalidCodeTry': '验证码无效，请检查应用后重试。',
  'login.totpRequired': '请输入身份验证器应用中的 6 位验证码。',
  'login.rateLimited': '尝试次数过多，请等待一分钟后重试。',
  'login.serverUnreachable': '无法连接服务器，请检查服务是否运行。',
  'login.expired': '会话已过期，请重新登录。',
  'login.linkExpired': '此一次性登录链接无效或已过期。',
  'login.failed': '登录失败。',
  'login.noUsers': '仍没有密码用户。请在服务器上运行 `gocryptfs-web user add <name>`。',
  'login.qrTitle': '手机登录',
  'login.qrButton': '手机登录',
  'login.qrHint': '用手机扫描此二维码，或在同一网络的设备上打开链接。',
  'login.qrAria': '一次性登录二维码',
  'login.qrUrlAria': '登录链接',
  'login.qrError': '无法创建登录链接。',
  'login.qrMissing': '服务器未返回登录二维码。',

  // one-time link (QR modal) ------------------------------------------------
  'qr.expired': '此链接已过期。',
  'qr.copy': '复制',
  'qr.copied': '已复制',
  'qr.expires': '链接将在 {seconds} 秒后过期 · 仅可使用一次。',
  'qr.generating': '正在生成一次性链接…',
  'qr.creating': '创建中…',
  'qr.newLink': '新链接',
  'qr.createLink': '创建新链接',

  // navigation --------------------------------------------------------------
  'nav.vaults': '保险库',
  'nav.logs': '日志',
  'nav.meta': '信息',
  'nav.security': '安全',
  'nav.logout': '退出登录',
  'nav.phoneLogin': '手机登录',
  'nav.pagesAria': '页面',
  'nav.switchToChinese': '切换到中文',
  'nav.switchToEnglish': 'Switch to English',
  'nav.langToggleToZh': '中文',
  'nav.langToggleToEn': 'EN',

  // theme -------------------------------------------------------------------
  'theme.auto': '自动',
  'theme.light': '浅色',
  'theme.dark': '深色',
  'theme.title': '主题：{mode}（点击切换）',
  'theme.aria': '主题：{mode}。点击切换到{next}。',

  // vaults ------------------------------------------------------------------
  'vaults.title': '保险库',
  'vaults.name': '名称',
  'vaults.mountPoint': '挂载点',
  'vaults.status': '状态',
  'vaults.mounted': '已挂载',
  'vaults.notMounted': '未挂载',
  'vaults.mount': '挂载',
  'vaults.umount': '卸载',
  'vaults.empty': '尚未配置保险库。',
  'vaults.loadFailed': '无法加载保险库。',

  // mount dialog ------------------------------------------------------------
  'mount.title': '挂载 {name}',
  'mount.passphrase': '口令',
  'mount.cancel': '取消',
  'mount.confirm': '挂载',
  'mount.mounting': '挂载中…',
  'mount.failed': '无法挂载该保险库。',

  // background jobs ---------------------------------------------------------
  'job.running': '处理中…',
  'job.done': '完成',
  'job.failed': '任务失败',
  'job.progress': '进度',
  'job.exited': '任务退出，代码 {code}',
  'job.lostConnection': '与任务事件流的连接已断开',

  // logs --------------------------------------------------------------------
  'logs.title': '日志',
  'logs.time': '时间',
  'logs.source': '来源',
  'logs.action': '操作',
  'logs.target': '目标',
  'logs.result': '结果',
  'logs.detail': '详情',
  'logs.refresh': '刷新',
  'logs.empty': '暂无日志。',
  'logs.titleAria': '日志条目',
  'logs.loadFailed': '无法加载日志。',

  // meta --------------------------------------------------------------------
  'meta.title': '信息',
  'meta.version': '版本',
  'meta.paths': '路径',
  'meta.deps': '依赖',
  'meta.depsOk': '正常',
  'meta.depsMissing': '缺失',
  'meta.unknown': '未知',
  'meta.noDeps': '服务器未提供依赖信息。',
  'meta.remote': '远程访问',
  'meta.publicUrl': '对外地址',
  'meta.notSet': '未设置',
  'meta.trustProxy': '信任代理头',
  'meta.trustProxyOn': '是',
  'meta.trustProxyOff': '否',
  'meta.loadFailed': '无法加载服务器信息。',

  // security / 2FA ----------------------------------------------------------
  'security.title': '安全',
  'security.badge': '双重验证',
  'security.codesShown': '已显示备用码',
  'security.unavailable': '此服务器版本未提供双重验证支持。',
  'security.twoFactor': '双重验证',
  'security.enroll': '设置双重验证',
  'security.enrollHint':
    '为账户添加第二重验证。你需要一个身份验证器应用，例如 Aegis、1Password 或 Google Authenticator。',
  'security.preparing': '准备中…',
  'security.qrAria': '双重验证绑定二维码',
  'security.secret': '密钥',
  'security.activateHint': '用身份验证器扫描二维码，或手动输入密钥。',
  'security.codeLabel': '6 位验证码',
  'security.verifying': '验证中…',
  'security.activate': '启用',
  'security.activateFailed': '启用失败。',
  'security.enabled': '双重验证已启用。',
  'security.disabled': '双重验证已关闭。',
  'security.invalidCode': '验证码无效。',
  'security.invalidCodeTry': '验证码无效，请检查应用后重试。',
  'security.backupCodes': '备用码',
  'security.backupHint':
    '请立即保存这些备用码——它们只显示一次。每个备用码可在你无法使用身份验证器时使用一次。',
  'security.copyAll': '全部复制',
  'security.copied': '已复制',
  'security.saved': '我已保存',
  'security.generated': '已生成新的备用码。',
  'security.regenerate': '重新生成备用码',
  'security.regenerateHint': '需要有效的 6 位验证码。旧备用码将失效。',
  'security.generating': '生成中…',
  'security.generate': '生成新备用码',
  'security.disable': '关闭双重验证',
  'security.disableTitle': '关闭双重验证',
  'security.disableHint': '输入当前的 6 位验证码（或备用码）以关闭此账户的双重验证。',
  'security.disableAuthLabel': '验证码',
  'security.disabling': '关闭中…',
  'security.enrollFailed': '绑定失败。',
  'security.disableFailed': '无法关闭双重验证。',
  'security.regenFailed': '无法生成新的备用码。',
  'security.serverMissing': '服务器未返回双重验证数据。'
};

export const catalogs = { en, zh };

function interpolate(text, vars) {
  if (!vars) return text;
  return text.replace(/\{(\w+)\}/g, (match, name) =>
    Object.prototype.hasOwnProperty.call(vars, name) ? String(vars[name]) : match
  );
}

export function t(lang, key, vars) {
  const catalog = catalogs[lang] || catalogs.en;
  let text = catalog[key];
  if (text === undefined) text = catalogs.en[key];
  if (text === undefined) return key;
  return interpolate(text, vars);
}

export function makeT(lang) {
  return (key, vars) => t(lang, key, vars);
}

export function detectLang() {
  try {
    if (typeof localStorage !== 'undefined') {
      const stored = localStorage.getItem(LANG_KEY);
      if (LANGS.includes(stored)) return stored;
    }
  } catch (_) {
    /* storage unavailable (private mode / disabled) */
  }
  try {
    if (typeof navigator !== 'undefined') {
      const primary = String(navigator.language || '').toLowerCase();
      if (primary.startsWith('zh')) return 'zh';
    }
  } catch (_) {
    /* ignore */
  }
  return 'en';
}

export function saveLang(lang) {
  if (!LANGS.includes(lang)) return detectLang();
  try {
    if (typeof localStorage !== 'undefined') localStorage.setItem(LANG_KEY, lang);
  } catch (_) {
    /* ignore */
  }
  return lang;
}

export function assertParity() {
  const enKeys = Object.keys(catalogs.en);
  const zhKeys = Object.keys(catalogs.zh);
  const missingInZh = enKeys.filter((k) => !Object.prototype.hasOwnProperty.call(catalogs.zh, k));
  const missingInEn = zhKeys.filter((k) => !Object.prototype.hasOwnProperty.call(catalogs.en, k));
  const empty = LANGS.flatMap((lang) =>
    Object.entries(catalogs[lang])
      .filter(([, v]) => typeof v !== 'string' || v.trim() === '')
      .map(([k]) => `${lang}:${k}`)
  );
  if (missingInZh.length || missingInEn.length || empty.length) {
    const parts = [];
    if (missingInZh.length) parts.push(`missing in zh: ${missingInZh.join(', ')}`);
    if (missingInEn.length) parts.push(`missing in en: ${missingInEn.join(', ')}`);
    if (empty.length) parts.push(`empty values: ${empty.join(', ')}`);
    throw new Error(`i18n catalog mismatch — ${parts.join('; ')}`);
  }
  return enKeys.length;
}
