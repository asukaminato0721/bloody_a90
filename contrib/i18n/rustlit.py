"""Scan Rust source for string literals, skipping comments and test modules."""
import re

CJK = re.compile(r'[　-鿿＀-￯]')


def literals(src):
    """Yield (start, end, raw_text, value) for each normal "..." literal."""
    i, n = 0, len(src)
    test = src.find('#[cfg(test)]\nmod tests')
    if test < 0:
        test = n
    while i < n:
        if i >= test:
            return
        c = src[i]
        if src.startswith('//', i):
            i = src.find('\n', i)
            if i < 0:
                return
            continue
        if src.startswith('/*', i):
            i = src.find('*/', i) + 2
            continue
        m = re.match(r'b?r(#*)"', src[i:])
        if m and (i == 0 or not (src[i - 1].isalnum() or src[i - 1] == '_')):
            close = '"' + m.group(1)
            j = src.find(close, i + len(m.group(0)))
            i = j + len(close)
            continue
        if c == "'":
            m = re.match(r"'(\\.[^']*|[^\\'])'", src[i:])
            if m:
                i += len(m.group(0))
                continue
            i += 1
            continue
        if c == '"':
            j = i + 1
            while src[j] != '"':
                j += 2 if src[j] == '\\' else 1
            byte = i > 0 and src[i - 1] == 'b'
            raw = src[i:j + 1]
            if not byte:
                yield i, j + 1, raw
            i = j + 1
            continue
        i += 1


def enclosing_macro(src, start):
    """Name of the macro whose first argument starts at `start`, if any."""
    k = start - 1
    while k >= 0 and src[k] in ' \n\t':
        k -= 1
    if k < 0 or src[k] not in '([{':
        return None
    k -= 1
    if k < 0 or src[k] != '!':
        return None
    m = re.search(r'([A-Za-z_][A-Za-z0-9_]*)$', src[:k])
    return (m.group(1), m.start(1)) if m else None
