"""Regenerate the display language tables in src/i18n.

    python3 contrib/i18n/update.py LANGUAGE_INI_DIR

LANGUAGE_INI_DIR is Data/Texts/LanguageIni of an official Bloody 8 install.
Keys are the Chinese texts of the program (src, without main.rs and tests).

- English.tsv is edited by hand. Keys without English are listed; add them.
  Captions the official program has use its English text.
- ChineseT.tsv: OpenCC s2twp of every key, with the official captions.
- The other languages hold the official captions only and fall back to
  English at run time.
"""
import collections
import glob
import os
import re
import subprocess
import sys

sys.path.insert(0, os.path.dirname(__file__))
from rustlit import literals, CJK  # noqa: E402

ESC = {'n': '\n', 't': '\t', '"': '"', '\\': '\\', "'": "'", '0': '\0', 'r': '\r'}


def value(raw):
    body = raw[1:-1]
    out, i = [], 0
    while i < len(body):
        c = body[i]
        if c == '\\':
            nxt = body[i + 1]
            if nxt == '\n':  # line continuation
                i += 2
                while i < len(body) and body[i] in ' \t\n':
                    i += 1
                continue
            if nxt == 'u':
                end = body.index('}', i)
                out.append(chr(int(body[i + 3:end], 16)))
                i = end + 1
                continue
            out.append(ESC.get(nxt, nxt))
            i += 2
            continue
        out.append(c)
        i += 1
    return ''.join(out)


PLACEHOLDER = re.compile(r'\{\{|\}\}|\{[^{}]*\}')


def normalize(text, formatted):
    if not formatted:
        return text
    return PLACEHOLDER.sub(lambda m: {'{{': '{', '}}': '}'}.get(m.group(0), '{}'), text)


def keys(root):
    found = []
    files = sorted(glob.glob(f'{root}/src/**/*.rs', recursive=True))
    for path in files:
        if path.endswith('/main.rs') or path.endswith('/i18n.rs'):
            continue
        src = open(path).read()
        for start, end, raw in literals(src):
            if not CJK.search(raw):
                continue
            # Format templates: the literal follows `format!(`-like macros.
            before = src[max(0, start - 30):start]
            formatted = bool(re.search(r'(format|tf|write|writeln|format_args)!\(\s*$', before))
            text = normalize(value(raw), formatted)
            pieces = [text]
            if '\n' in text:
                pieces += [line for line in text.split('\n')]
            for piece in pieces:
                if CJK.search(piece) and piece not in found:
                    found.append(piece)
    return found



INI = ['BloodyWorkShop8', 'Oscar-Macro', 'Drag-Macro']
LANGUAGES = ['ChineseT', 'English', 'Dutch', 'French', 'German', 'Italian', 'Japanese',
             'Korean', 'Polish', 'Portuguese', 'Russian', 'Spanish', 'Turkish']


def ini(root, lang):
    table = {}
    for name in INI:
        path = f'{root}/{lang}/{name}_{lang}.ini'
        if not os.path.exists(path):
            continue
        section = ''
        for line in open(path, 'rb').read().decode('utf-16').splitlines():
            line = line.strip()
            if line.startswith('[') and line.endswith(']'):
                section = line
            elif '=' in line and not line.startswith('//'):
                key, value = line.split('=', 1)
                table[(name, section, key.strip())] = value.strip()
    return table


def official(root, keys):
    where = collections.defaultdict(list)
    for loc, value in ini(root, 'ChineseS').items():
        where[value].append(loc)
    result = {}
    for lang in LANGUAGES:
        table = ini(root, lang)
        result[lang] = {}
        for key in keys:
            votes = collections.Counter(table[loc] for loc in where.get(key, []) if table.get(loc))
            if votes:
                result[lang][key] = votes.most_common(1)[0][0]
    return result


def read(path):
    table = {}
    if os.path.exists(path):
        for line in open(path):
            line = line.rstrip('\n')
            if line and not line.startswith('#') and '\t' in line:
                key, value = line.split('\t', 1)
                table[key] = value
    return table


def write(path, note, keys, table):
    with open(path, 'w') as out:
        out.write(f'# {note}\n')
        for key in keys:
            if key in table and table[key] != key and not re.search(r'\{[a-z]+\}', key):
                out.write(f'{key}\t{table[key]}\n')


def main():
    repo = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
    out = f'{repo}/src/i18n'
    escaped = [k.replace('\\', '\\\\').replace('\n', '\\n').replace('\t', '\\t')
               for k in keys(repo)]
    given = official(sys.argv[1], escaped)
    english = read(f'{out}/English.tsv')
    english.update(given['English'])
    # Keys with named placeholders like {e} can never match; they are skipped.
    missing = [k for k in escaped
               if k not in english and not re.search(r'\{[a-z]+\}', k)]
    write(f'{out}/English.tsv',
          "English. Captions of the official program come from its English texts.",
          escaped, english)
    trad = subprocess.run(['opencc', '-c', 's2twp'], input='\n'.join(escaped),
                          capture_output=True, text=True, check=True).stdout.split('\n')
    chinese_t = dict(zip(escaped, trad))
    chinese_t.update(given['ChineseT'])
    write(f'{out}/ChineseT.tsv',
          "Traditional Chinese: OpenCC s2twp, with the official program's captions.",
          escaped, chinese_t)
    for lang in LANGUAGES[2:]:
        write(f'{out}/{lang}.tsv',
              f'{lang}: captions of the official program. Other text falls back to English.',
              escaped, given[lang])
    for key in missing:
        print(f'missing English: {key}')


if __name__ == '__main__':
    main()
