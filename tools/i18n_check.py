#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""i18n 一致性检查：

1. 每个 t!("key", ...) / tn!("key", ...)（含参数中的嵌套调用）的参数个数
   与翻译表中的占位符个数一致；
2. 所有键在 zh-CN / en-US 表中都存在且升序；
3. 业务源码中不残留中文界面文案。

用法：python3 tools/i18n_check.py [repo_root]
"""
import os
import re
import sys

KEYS_RE = re.compile(r'^\s*\("([^"]+)",\s*"(.*)"\),\s*$', re.M)
CJK = re.compile(r'[\u3000-\u303f\u3040-\u30ff\u3400-\u4dbf\u4e00-\u9fff\uf900-\ufaff\uff01-\uff5e]')


def parse_table(path):
    text = open(path, encoding='utf-8').read()
    entries = []
    for m in KEYS_RE.finditer(text):
        entries.append((m.group(1), m.group(2)))
    return entries


def count_placeholders(template):
    """统计 {} / {name} 占位符个数（跳过 {{ }} 转义）。"""
    t = template.replace('{{', '').replace('}}', '')
    return len(re.findall(r'\{[^{}]*\}', t))


def extract_calls(src):
    """返回 (line, macro, key, args_code) 列表。"""
    out = []
    i = 0
    n = len(src)
    while i < n:
        if src.startswith('//', i):
            i = src.find('\n', i)
            if i < 0:
                break
            continue
        if src.startswith('/*', i):
            j = src.find('*/', i + 2)
            i = n if j < 0 else j + 2
            continue
        if src[i] == "'":
            # 字符字面量（'a'、'\n'、'"'、'\x41'、'\u{1F600}'）或生命周期（'static）
            if i + 1 < n and src[i + 1] == '\\':
                j = i + 2
                if j < n and src[j] == 'x':
                    j += 3  # \xNN：跳过 'x' 与两位十六进制
                elif j < n and src[j] == 'u':
                    k = src.find('}', j)
                    j = (k + 1) if k >= 0 else j + 1
                else:
                    j += 1
                i = j + 1 if j < n and src[j] == "'" else i + 1
                continue
            if i + 2 < n and src[i + 2] == "'":
                i += 3
                continue
            i += 1
            continue
        if src[i] == '"':
            i = skip_string(src, i)
            continue
        m = re.compile(r'\b(tn?|tr|trf|tr_named)\!\(').match(src, i)
        if m:
            name = m.group(1)
            j = m.end()
            while j < n and src[j] in ' \t\n':
                j += 1
            if j < n and src[j] == '"':
                k_end = skip_string(src, j)
                key = src[j + 1:k_end - 1]
                # 定位参数列表结尾
                depth = 1
                k = m.end()
                while k < n and depth:
                    c = src[k]
                    if c == '"':
                        k = skip_string(src, k)
                        continue
                    if c in '([{':
                        depth += 1
                    elif c in ')]}':
                        depth -= 1
                    k += 1
                args_code = src[k_end:k - 1]
                args_code = args_code.strip()
                if args_code.startswith(','):
                    args_code = args_code[1:]
                out.append((src.count('\n', 0, i) + 1, name, key, args_code))
                # 只跳过宏名与 "t!("，继续扫描参数内部的嵌套调用（如 t!("a", t!("b"))）
                i = m.end()
                continue
        i += 1
    return out


def skip_string(src, i):
    i += 1
    while i < len(src):
        if src[i] == '\\':
            i += 2
            continue
        if src[i] == '"':
            return i + 1
        i += 1
    return i


def split_top_level(code):
    parts = []
    depth = 0
    cur = ''
    i = 0
    while i < len(code):
        c = code[i]
        if c == '"':
            j = skip_string(code, i)
            cur += code[i:j]
            i = j
            continue
        if c in '([{':
            depth += 1
        elif c in ')]}':
            depth -= 1
        if c == ',' and depth == 0:
            parts.append(cur)
            cur = ''
            i += 1
            continue
        cur += c
        i += 1
    if cur.strip():
        parts.append(cur)
    return [p for p in parts if p.strip()]


def check_source(src, table):
    """返回 [(line, key, got, expected)]：调用点参数数与占位符不符的清单。"""
    problems = []
    for line, _macro, key, args in extract_calls(src):
        expected = count_placeholders(table.get(key, ''))
        got = len(split_top_level(args))
        if got != expected:
            problems.append((line, key, got, expected))
    return problems


def selftest():
    """扫描/比对逻辑自检，防止再次出现「静默跳过调用点」这类缺陷。

    返回错误说明字符串；`None` 表示通过。
    """
    table = {'k.one': 'a {}', 'k.two': 'b {} {}', 'k.zero': 'c'}
    cases = [
        # (源码片段, 期望检测出的问题数)
        ('t!("k.one", x)', 0),
        ('t!("k.one", x, y)', 1),          # 参数过多
        ('t!("k.two", x)', 1),             # 参数过少
        ('t!("k.zero")', 0),
        ('let c = \'"\'; t!("k.one", x);', 0),   # 字符字面量不得干扰扫描
        ('let c = \'\\x7f\'; t!("k.one", x);', 0),
        ('t!("k.one", t!("k.two", x, y))', 0),   # 嵌套调用，各自参数都正确
        ('t!("k.one", t!("k.two", x))', 1),      # 嵌套调用里参数过少
    ]
    for src, want in cases:
        got = len(check_source(src, table))
        if got != want:
            return f'selftest: {src!r} 期望 {want} 个问题，实际 {got} 个'
    nested = extract_calls('t!("k.one", t!("k.two", x, y))')
    if len(nested) != 2:
        return f'selftest: 嵌套调用只扫描到 {len(nested)} 个（应为 2 个）'
    # 注释中的宏调用不得计入
    if extract_calls('// t!("k.one")\n/* t!("k.one") */'):
        return 'selftest: 注释中的调用被误计'

    # shell 侧：调用扫描与参数解析
    calls = extract_shell_calls('t k.one "$a" "$b"')
    if len(calls) != 1 or calls[0][3] != ['X', 'X']:
        return f'selftest: shell 调用/参数解析失败: {calls}'
    calls = extract_shell_calls('t k.one >&2')
    if len(calls) != 1 or calls[0][3]:
        return f'selftest: shell 重定向解析失败: {calls}'
    calls = extract_shell_calls('log_line "$(t k.one "$a")"')
    if len(calls) != 1 or len(calls[0][3]) != 1:
        return f'selftest: shell 命令替换中的调用扫描失败: {calls}'
    if extract_shell_calls('# t k.one "$a"'):
        return 'selftest: shell 注释中的调用被误计'
    return None


def main():
    root = sys.argv[1] if len(sys.argv) > 1 else os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    selftest_error = selftest()
    if selftest_error:
        print(f'FAIL: 校验器自检未通过（{selftest_error}）')
        sys.exit(1)
    zh = dict(parse_table(os.path.join(root, 'src/i18n/zh_cn.rs')))
    en = dict(parse_table(os.path.join(root, 'src/i18n/en_us.rs')))
    errors = []

    if list(zh) != sorted(zh):
        errors.append('zh_cn.rs 未按升序排列')
    if list(en) != sorted(en):
        errors.append('en_us.rs 未按升序排列')
    if set(zh) != set(en):
        errors.append(f'键不一致: 仅 zh={sorted(set(zh)-set(en))} 仅 en={sorted(set(en)-set(zh))}')

    # 同一键在两种语言中的占位符必须一致，否则运行期会丢参数
    for key in sorted(set(zh) & set(en)):
        n_zh = count_placeholders(zh[key])
        n_en = count_placeholders(en[key])
        if n_zh != n_en:
            errors.append(f'{key}: zh 有 {n_zh} 个占位符，en 有 {n_en} 个')

    for path in ('src/main.rs', 'src/cli.rs'):
        full = os.path.join(root, path)
        src = open(full, encoding='utf-8').read()
        for line, macro, key, args in extract_calls(src):
            if key not in zh:
                errors.append(f'{path}:{line} 键未定义: {key}')
                continue
            expected = count_placeholders(zh[key])
            got = len(split_top_level(args))
            if got != expected:
                errors.append(
                    f'{path}:{line} {macro}!({key}) 参数 {got} 个，但占位符 {expected} 个')
        # 中文残留检查（跳过注释）
        stripped = re.sub(r'//[^\n]*', '', src)
        stripped = re.sub(r'/\*.*?\*/', '', stripped, flags=re.S)
        for m in re.finditer(r'"((?:[^"\\]|\\.)*)"', stripped):
            if CJK.search(m.group(1)):
                line = stripped.count('\n', 0, m.start()) + 1
                errors.append(f'{path}:{line} 未接入 i18n 的中文文案: {m.group(1)!r}')

    shell_calls = check_shell(root, errors)

    if errors:
        print(f'FAIL: {len(errors)} 个问题')
        for e in errors:
            print('  -', e)
        sys.exit(1)
    print(
        f'OK: TUI {len(zh)} 键 / {len(en)} 键，Shell {shell_calls} 个调用点，'
        'zh/en 一致、参数匹配、无中文残留')




# ============================================================
# Shell 侧（shell/lib/i18n.sh 消息表 + gocryptfs-cli / gocryptfs-lib.sh 调用点）
# ============================================================

SHELL_I18N = 'shell/lib/i18n.sh'
SHELL_FILES = ('shell/gocryptfs-cli', 'shell/lib/gocryptfs-lib.sh')
SHELL_KEY_RE = re.compile(r'\[\s*([a-z][a-z0-9_.]*)\s*\]=\'(.*?)\'\n', re.S)


def parse_shell_table(path):
    """解析 shell 关联数组消息表，返回 {key: 模板}。"""
    text = open(path, encoding='utf-8').read()
    tables = {}
    for name in ('I18N_ZH', 'I18N_EN'):
        m = re.search(rf'{name}=\(\n(.*?)\n\)', text, re.S)
        tables[name] = dict(SHELL_KEY_RE.findall(m.group(1) + '\n')) if m else {}
    return tables


def shell_count_placeholders(template):
    """统计 shell 文案里的 printf 占位符（%s / %d 等，忽略 %%。）。"""
    return len(re.findall(r'%(?!%)[a-zA-Z]', template))


def strip_shell_comments(src):
    """去掉整行注释（保留行号：注释行替换为空行）。"""
    out = []
    for line in src.split('\n'):
        out.append('' if line.lstrip().startswith('#') else line)
    return '\n'.join(out)


def extract_shell_calls(src):
    """返回 (line, macro, key, [args...]) 列表，识别 t/te 调用。"""
    src = strip_shell_comments(src)
    out = []
    n = len(src)
    i = 0
    while i < n:
        m = re.compile(r'(?<![A-Za-z0-9_$])(te?)[ \t]+([a-z][a-z0-9_.]*)(?=[ \t\n>&|;)])').match(src, i)
        if m:
            macro, key = m.group(1), m.group(2)
            args, end = shell_args(src, m.end())
            out.append((src.count('\n', 0, i) + 1, macro, key, args))
            i = max(end, m.end())
            continue
        i += 1
    return out


def shell_args(src, i):
    """从 i 开始解析 shell 命令参数，遇到命令分隔符（; > | && || ) 换行）结束。"""
    args, cur, depth = [], '', 0
    n = len(src)
    while i < n:
        c = src[i]
        if depth == 0 and c in '\n;>|':
            break
        if depth == 0 and c == '&' and i + 1 < n and src[i + 1] == '&':
            break
        if depth == 0 and c == ')':
            break
        if c == '\\' and i + 1 < n:
            i += 2
            continue
        if c in '"\'':
            quote = c
            i += 1
            while i < n and src[i] != quote:
                if src[i] == '\\':
                    i += 1
                i += 1
            i += 1
            cur += 'X'
            continue
        if c == '$' and i + 1 < n and src[i + 1] in '({':
            depth += 1
            i += 2
            continue
        if depth > 0 and c in ')}':
            depth -= 1
            i += 1
            continue
        if c in ' \t' and depth == 0:
            if cur:
                args.append(cur)
                cur = ''
            i += 1
            continue
        cur += c
        i += 1
    if cur:
        args.append(cur)
    return args, i


def check_shell(root, errors):
    i18n_path = os.path.join(root, SHELL_I18N)
    if not os.path.exists(i18n_path):
        errors.append(f'缺少 {SHELL_I18N}')
        return 0
    tables = parse_shell_table(i18n_path)
    zh, en = tables['I18N_ZH'], tables['I18N_EN']
    if set(zh) != set(en):
        errors.append(
            f'shell 消息表键不一致: 仅 zh={sorted(set(zh) - set(en))} 仅 en={sorted(set(en) - set(zh))}')
    for key in sorted(set(zh) & set(en)):
        n_zh = shell_count_placeholders(zh[key])
        n_en = shell_count_placeholders(en[key])
        if n_zh != n_en:
            errors.append(f'shell {key}: zh 有 {n_zh} 个占位符，en 有 {n_en} 个')

    checked = 0
    for rel in SHELL_FILES:
        full = os.path.join(root, rel)
        src = open(full, encoding='utf-8').read()
        for line, macro, key, args in extract_shell_calls(src):
            checked += 1
            if key not in zh:
                errors.append(f'{rel}:{line} shell 键未定义: {key}')
                continue
            expected = shell_count_placeholders(zh[key])
            if len(args) != expected:
                errors.append(
                    f'{rel}:{line} {macro} {key} 参数 {len(args)} 个，但占位符 {expected} 个')
        for i, raw in enumerate(strip_shell_comments(src).split('\n'), 1):
            if CJK.search(raw):
                errors.append(f'{rel}:{i} 未接入 i18n 的中文文案: {raw.strip()[:60]}')
    if checked < 50:
        errors.append(f'shell 调用点仅扫描到 {checked} 个，扫描逻辑可能失效')
    return checked


if __name__ == '__main__':
    main()
