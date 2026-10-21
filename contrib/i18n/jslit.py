"""Scan JavaScript source for string literals, skipping comments.

Template literals become keys with `{}` for each `${...}`, the placeholder
form that i18n::tr_text matches at run time.
"""


def literals(src):
    """Yield the text of each '...', "..." and `...` literal."""
    i, n = 0, len(src)
    while i < n:
        if src.startswith('//', i):
            i = src.find('\n', i)
            if i < 0:
                return
            continue
        if src.startswith('/*', i):
            i = src.find('*/', i) + 2
            continue
        c = src[i]
        if c in '\'"':
            text, i = quoted(src, i)
            yield text
            continue
        if c == '`':
            yield from template(src, i)
            i = template_end(src, i)
            continue
        i += 1


ESC = {'n': '\n', 't': '\t', 'r': '\r', '0': '\0'}


def quoted(src, i):
    quote, out = src[i], []
    i += 1
    while src[i] != quote:
        if src[i] == '\\':
            out.append(ESC.get(src[i + 1], src[i + 1]))
            i += 2
        else:
            out.append(src[i])
            i += 1
    return ''.join(out), i + 1


def expression_end(src, i):
    """Index after the `}` closing the `${` expression that starts at i."""
    depth = 1
    while depth:
        c = src[i]
        if c in '\'"':
            _, i = quoted(src, i)
            continue
        if c == '`':
            i = template_end(src, i)
            continue
        depth += {'{': 1, '}': -1}.get(c, 0)
        i += 1
    return i


def template_end(src, i):
    i += 1
    while src[i] != '`':
        if src[i] == '\\':
            i += 2
        elif src.startswith('${', i):
            i = expression_end(src, i + 2)
        else:
            i += 1
    return i + 1


def template(src, i):
    """The template's text with `{}` placeholders, then its nested literals."""
    out, nested = [], []
    i += 1
    while src[i] != '`':
        if src[i] == '\\':
            out.append(ESC.get(src[i + 1], src[i + 1]))
            i += 2
        elif src.startswith('${', i):
            end = expression_end(src, i + 2)
            nested.extend(literals(src[i + 2:end - 1]))
            out.append('{}')
            i = end
        else:
            out.append(src[i])
            i += 1
    yield ''.join(out)
    yield from nested
