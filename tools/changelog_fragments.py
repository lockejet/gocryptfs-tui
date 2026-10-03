#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""把 changelog.d/ 片段合并进 git-cliff 生成的 CHANGELOG。

背景：CHANGELOG.md 由 git-cliff 从提交历史生成，手工直接编辑会在下次生成时丢失。
本工具让"手写内容"以片段文件的形式长期存在（随代码提交、可 review），
生成时自动合并进最新版本段落。

片段格式（changelog.d/*.md，一个文件一个主题）：

    ### Features
    - 新增 i18n（zh-CN / en-US）：`--lang`、配置 `language:`、运行期 `L` 键切换

    ### Bug Fixes
    - 修复英文界面标签错列

- 必须用 `### 分组名` 开头（与 git-cliff 的分组名一致时会自动合并到同一组）；
- 没有 `###` 的纯列表会归入 `Miscellaneous`；
- 不要使用 `##`（版本标题由生成器负责）；
- `README.md`、`_` 开头的文件、`archive/` 目录会被忽略。

用法：
    # 日常生成（片段保留，便于下次重新生成）
    python3 tools/changelog_fragments.py merge \
        --generated .staging/CHANGELOG.generated.md \
        --fragments changelog.d --output CHANGELOG.md

    # 发布生成（tag 段落 + 归档已消费片段）
    python3 tools/changelog_fragments.py merge --generated ... --fragments changelog.d \
        --merge-into first --tag v0.1.3 --archive changelog.d/archive --output CHANGELOG.md

    # 只看结果（不写文件、不归档）
    python3 tools/changelog_fragments.py merge --generated ... --fragments ... --output -

    # 片段语法自检 + 逻辑自检
    python3 tools/changelog_fragments.py --selftest
"""
import argparse
import datetime
import os
import re
import shutil
import sys

GROUP_RE = re.compile(r'^###\s+(.*\S)\s*$')
SECTION_RE = re.compile(r'^##\s+')
DEFAULT_GROUP = 'Miscellaneous'
# 合并后的分组顺序（与 cliff.toml 的 commit_parsers 保持一致）
GROUP_ORDER = ['Features', 'Bug Fixes', 'Documentation', 'Performance', 'Refactor',
               'Styling', 'Testing', 'Miscellaneous', 'Build']


def split_sections(text):
    """拆成 (前导, [版本段落...])。段落以行首 `## ` 划分。"""
    lines = text.split('\n')
    starts = [i for i, l in enumerate(lines) if SECTION_RE.match(l)]
    if not starts:
        return text, []
    preamble = '\n'.join(lines[:starts[0]])
    sections = []
    for idx, start in enumerate(starts):
        end = starts[idx + 1] if idx + 1 < len(starts) else len(lines)
        sections.append('\n'.join(lines[start:end]))
    return preamble, sections


def parse_section(section):
    """把版本段落拆成 (标题行, [(组名, [行])...])。"""
    lines = section.split('\n')
    heading = lines[0].strip() if lines else ''
    groups = []
    current = None
    loose = []
    for line in lines[1:]:
        m = GROUP_RE.match(line)
        if m:
            current = (m.group(1), [])
            groups.append(current)
            continue
        stripped = line.rstrip()
        if not stripped:
            continue
        if current is None:
            loose.append(stripped)
        else:
            current[1].append(stripped)
    if loose:
        # 没有 ### 的散装条目
        for name, items in groups:
            if name == DEFAULT_GROUP:
                items.extend(loose)
                break
        else:
            groups.append((DEFAULT_GROUP, loose))
    return heading, groups


def parse_fragment(text, path):
    """解析片段文件，返回 [(组名, [行])...]；语法错误时抛 ValueError。"""
    groups = []
    current = None
    for no, line in enumerate(text.split('\n'), 1):
        if SECTION_RE.match(line):
            raise ValueError(f'{path}:{no} 片段中不要使用 `##`（版本标题由生成器负责）')
        m = GROUP_RE.match(line)
        if m:
            current = (m.group(1), [])
            groups.append(current)
            continue
        stripped = line.rstrip()
        if not stripped:
            continue
        if current is None:
            groups.append((DEFAULT_GROUP, [stripped]))
            current = groups[-1]
        else:
            current[1].append(stripped)
    return [(name, items) for name, items in groups if items]


def collect_fragments(fragments_dir):
    """返回 [(路径, 文本)]，按文件名排序；忽略 README/_ 前缀/目录。"""
    if not os.path.isdir(fragments_dir):
        return []
    out = []
    for name in sorted(os.listdir(fragments_dir)):
        if not name.endswith('.md'):
            continue
        if name == 'README.md' or name.startswith('_') or name.startswith('.'):
            continue
        path = os.path.join(fragments_dir, name)
        if not os.path.isfile(path):
            continue
        with open(path, encoding='utf-8') as f:
            out.append((path, f.read()))
    return out


def group_rank(name):
    return (GROUP_ORDER.index(name), '') if name in GROUP_ORDER else (len(GROUP_ORDER), name)


def merge_groups(groups, extra):
    """把 extra 合并进 groups：同名组合并、去重；按 GROUP_ORDER 排序输出。"""
    index = {name: items for name, items in groups}
    for name, items in extra:
        index.setdefault(name, [])
        for line in items:
            if line not in index[name]:
                index[name].append(line)
    names = sorted((n for n in index if index[n]), key=group_rank)
    return [(name, index[name]) for name in names]


def render_section(heading, groups):
    parts = [f'{heading}\n']
    for name, items in groups:
        parts.append(f'### {name}\n' + '\n'.join(items) + '\n')
    return '\n'.join(parts).rstrip('\n')


def render(preamble, sections):
    blocks = []
    if preamble.strip():
        blocks.append(preamble.strip())
    blocks.extend(sec.strip('\n') for sec in sections if sec.strip())
    return '\n\n'.join(blocks) + '\n'


def do_merge(generated, fragments_dir, merge_into='unreleased', tag=''):
    """返回 (合并后的文本, 已消费的片段路径列表)。

    merge_into:
      * `unreleased`（默认，日常生成）：并入 `## [Unreleased]` 段；
        若 git-cliff 因"没有未发布提交"而未生成该段，则新建一个，
        绝不会污染已发布的版本段。
      * `first`（发布流程，配合 tag）：并入 tag 对应的版本段；
        该段不存在时（例如没有未发布提交）新建，同样不污染已发布版本。

    注意：日常生成**不要**归档片段，否则下次 git-cliff 重新生成时手写内容会消失。
    """
    preamble, sections = split_sections(generated)
    if not sections:
        raise SystemExit('错误: 生成结果里没有找到 `## ` 版本段落')

    extra = []
    consumed = []
    for path, text in collect_fragments(fragments_dir):
        extra.extend(parse_fragment(text, path))
        consumed.append(path)

    if merge_into == 'first':
        ver = tag.lstrip('vV')
        if tag:
            for i, sec in enumerate(sections):
                heading, groups = parse_section(sec)
                if ver in heading:
                    sections[i] = render_section(heading, merge_groups(groups, extra))
                    return render(preamble, sections), consumed
            if not extra:
                return render(preamble, sections), consumed
            heading = f'## [{ver}] - {datetime.date.today().isoformat()}'
            sections.insert(0, render_section(heading, merge_groups([], extra)))
            return render(preamble, sections), consumed
        heading, groups = parse_section(sections[0])
        sections[0] = render_section(heading, merge_groups(groups, extra))
        return render(preamble, sections), consumed

    # unreleased：优先找现成的 [Unreleased] 段
    for i, sec in enumerate(sections):
        heading, groups = parse_section(sec)
        if 'unreleased' in heading.lower():
            sections[i] = render_section(heading, merge_groups(groups, extra))
            return render(preamble, sections), consumed
    if extra:
        new_section = render_section('## [Unreleased]', merge_groups([], extra))
        sections.insert(0, new_section)
    return render(preamble, sections), consumed


def archive_fragments(paths, archive_dir):
    if not archive_dir or not paths:
        return
    os.makedirs(archive_dir, exist_ok=True)
    for path in paths:
        name = os.path.basename(path)
        dst = os.path.join(archive_dir, name)
        n = 1
        while os.path.exists(dst):
            dst = os.path.join(archive_dir, f'{os.path.splitext(name)[0]}.{n}.md')
            n += 1
        shutil.move(path, dst)


def cmd_check(args):
    errors = []
    for path, text in collect_fragments(args.fragments):
        try:
            groups = parse_fragment(text, path)
        except ValueError as e:
            errors.append(str(e))
            continue
        if not groups:
            errors.append(f'{path}: 片段为空（没有任何条目）')
    if errors:
        print(f'FAIL: {len(errors)} 个片段有问题')
        for e in errors:
            print('  -', e)
        return 1
    n = len(collect_fragments(args.fragments))
    print(f'OK: {n} 个片段格式正确')
    return 0


def cmd_merge(args):
    with open(args.generated, encoding='utf-8') as f:
        generated = f.read()
    merged, consumed = do_merge(generated, args.fragments, args.merge_into, args.tag)
    if args.output == '-':
        sys.stdout.write(merged)
        return 0
    with open(args.output, 'w', encoding='utf-8') as f:
        f.write(merged)
    print(f'>>> CHANGELOG: 合并 {len(consumed)} 个片段 -> {args.output}')
    if consumed:
        if args.archive:
            archive_fragments(consumed, args.archive)
            print(f'>>> 片段已归档到 {args.archive}/')
        else:
            print('>>> 片段未归档（--archive 未指定），已消费的片段仍在原处')
    return 0


def selftest():
    """逻辑自检：返回错误说明字符串，None 表示通过。"""
    generated = (
        '# Changelog\n\n本项目遵循 Semantic Versioning。\n'
        '## [Unreleased]\n\n### Features\n- 来自提交的条目\n\n'
        '## [0.1.2] - 2026-09-30\n\n### Features\n- 旧条目\n'
    )
    preamble, sections = split_sections(generated)
    if len(sections) != 2 or not sections[0].startswith('## [Unreleased]'):
        return f'段落解析错误: {sections!r}'
    if '## [0.1.2]' not in sections[1]:
        return '历史段落未保留'
    heading, groups = parse_section(sections[0])
    if groups != [('Features', ['- 来自提交的条目'])]:
        return f'分组解析错误: {groups}'
    merged = merge_groups(groups, [('Features', ['- 来自片段的条目']), ('Bug Fixes', ['- 修复'])])
    if merged != [('Features', ['- 来自提交的条目', '- 来自片段的条目']), ('Bug Fixes', ['- 修复'])]:
        return f'合并逻辑错误: {merged}'
    if merge_groups(groups, [('Features', ['- 来自提交的条目'])]) != groups:
        return '重复条目未去重'
    text = render(preamble, [render_section(heading, merged), sections[1]])
    if '### Bug Fixes' not in text or '## [0.1.2]' not in text or '- 来自片段的条目' not in text:
        return f'渲染结果缺内容:\n{text}'
    # 没有 [Unreleased] 段（无未发布提交）时：必须新建，绝不能污染已发布版本
    import tempfile
    released_only = '# Changelog\n\n## [0.1.2] - 2026-09-30\n\n### Features\n- 旧条目\n'
    with tempfile.TemporaryDirectory() as d:
        with open(os.path.join(d, 'x.md'), 'w', encoding='utf-8') as f:
            f.write('### Features\n- 来自片段的条目\n')
        text, consumed = do_merge(released_only, d, 'unreleased')
        if len(consumed) != 1:
            return f'片段未被消费: {consumed}'
        if '## [Unreleased]' not in text:
            return f'缺少未发布提交时未新建 [Unreleased] 段:\n{text}'
        if text.index('## [Unreleased]') > text.index('## [0.1.2]'):
            return '[Unreleased] 段位置错误（应在已发布版本之前）'
        released_part = text.split('## [0.1.2]')[1]
        if '来自片段的条目' in released_part:
            return f'片段污染了已发布版本段:\n{text}'
        # 发布模式：tag 对应段落不存在时新建，不污染已发布版本
        text, _ = do_merge(released_only, d, 'first', 'v0.2.0')
        if '## [Unreleased]' in text:
            return 'first 模式不应新建 [Unreleased] 段'
        if '## [0.2.0]' not in text:
            return f'发布模式未新建 tag 段落:\n{text}'
        if '来自片段的条目' in text.split('## [0.1.2]')[1]:
            return f'发布模式污染了已发布版本段:\n{text}'
        # tag 对应段落已存在时并入其中
        tagged = ('# Changelog\n\n## [0.2.0] - 2026-10-03\n\n### Features\n- 提交条目\n\n'
                  '## [0.1.2] - 2026-09-30\n\n### Features\n- 旧条目\n')
        text, _ = do_merge(tagged, d, 'first', 'v0.2.0')
        if '来自片段的条目' not in text.split('## [0.1.2]')[0]:
            return f'发布模式未并入 tag 段落:\n{text}'
    loose = parse_fragment('- 散装条目\n', 'x.md')
    if loose != [(DEFAULT_GROUP, ['- 散装条目'])]:
        return f'散装条目解析错误: {loose}'
    try:
        parse_fragment('## [1.0.0]\n', 'bad.md')
    except ValueError:
        pass
    else:
        return '片段里的 `##` 未被拒绝'
    return None


def main():
    parser = argparse.ArgumentParser(description='合并 changelog.d/ 片段到 git-cliff 生成结果')
    parser.add_argument('--selftest', action='store_true', help='运行自检后退出')
    sub = parser.add_subparsers(dest='cmd')
    m = sub.add_parser('merge', help='合并片段')
    m.add_argument('--generated', required=True, help='git-cliff 生成的文件')
    m.add_argument('--fragments', default='changelog.d', help='片段目录')
    m.add_argument('--archive', default='', help='消费后归档目录（留空则不归档）')
    m.add_argument('--output', required=True, help='输出文件；`-` 表示打印到 stdout')
    m.add_argument('--merge-into', choices=['unreleased', 'first'], default='unreleased',
                   help='合并位置：unreleased=并入/新建 [Unreleased] 段（默认）；first=并入 tag 对应版本段（发布用）')
    m.add_argument('--tag', default='', help='发布模式下的版本标签（如 v0.1.3）')
    c = sub.add_parser('check', help='校验片段语法')
    c.add_argument('--fragments', default='changelog.d', help='片段目录')
    args = parser.parse_args()

    if args.selftest:
        err = selftest()
        if err:
            print(f'FAIL: 自检未通过（{err}）')
            return 1
        print('OK: changelog_fragments 自检通过')
        return 0
    if args.cmd == 'merge':
        return cmd_merge(args)
    if args.cmd == 'check':
        return cmd_check(args)
    parser.print_help()
    return 0


if __name__ == '__main__':
    sys.exit(main())
