#!/usr/bin/env python3
"""Strip local signing configuration from HarmonyOS `build-profile.json5` for CI builds."""

import sys


def scan_json5_spans(text: str):
    """Return a code mask and literal string spans while ignoring JSON5 comments."""
    n = len(text)
    mask = [True] * n
    str_spans = []
    i = 0
    while i < n:
        ch = text[i]
        if ch in ('"', "'"):
            j = i + 1
            while j < n:
                if text[j] == "\\":
                    j += 2
                    continue
                if text[j] == ch:
                    j += 1
                    break
                j += 1
            j = min(j, n)
            str_spans.append((i, j))
            for k in range(i, j):
                mask[k] = False
            i = j
        elif ch == "/" and i + 1 < n and text[i + 1] == "/":
            j = text.find("\n", i)
            j = n if j < 0 else j + 1
            for k in range(i, j):
                mask[k] = False
            i = j
        elif ch == "/" and i + 1 < n and text[i + 1] == "*":
            j = text.find("*/", i + 2)
            j = n if j < 0 else j + 2
            for k in range(i, j):
                mask[k] = False
            i = j
        else:
            i += 1
    return mask, str_spans


def find_value_end(text: str, mask, colon: int) -> int:
    """Find the end index (exclusive) of the JSON5 value following `:`."""
    n = len(text)
    i = colon + 1
    while i < n:
        ch = text[i]
        if ch in " \t\r\n":
            i += 1
        elif not mask[i] and text[i : i + 2] == "//":
            j = text.find("\n", i)
            i = n if j < 0 else j + 1
        elif not mask[i] and text[i : i + 2] == "/*":
            j = text.find("*/", i + 2)
            if j < 0:
                raise ValueError("unterminated comment after colon")
            i = j + 2
        else:
            break

    if i >= n:
        raise ValueError("missing value after colon")

    ch = text[i]
    if ch in "[{":
        depth = 0
        while i < n:
            if not mask[i]:
                i += 1
                continue
            if text[i] in "[{":
                depth += 1
            elif text[i] in "]}":
                depth -= 1
                if depth == 0:
                    return i + 1
            i += 1
        raise ValueError("unbalanced brackets in value")

    if ch in ('"', "'"):
        j = i + 1
        while j < n:
            if text[j] == "\\":
                j += 2
                continue
            if text[j] == ch:
                return j + 1
            j += 1
        raise ValueError("unterminated string value")

    while i < n and text[i] not in ",]}\r\n":
        i += 1
    return i


def clean_dangling_comma(text: str, pos: int) -> str:
    n = len(text)
    left = pos - 1
    while left >= 0 and text[left] in " \t\r\n":
        left -= 1
    right = pos
    while right < n and text[right] in " \t\r\n":
        right += 1
    if left >= 0 and right < n and text[left] == "," and text[right] in ",}]":
        return text[:left] + text[left + 1 :]
    return text


def remove_property_once(text: str, target_key: str):
    mask, str_spans = scan_json5_spans(text)
    n = len(text)
    for start, end in str_spans:
        raw = text[start + 1 : end - 1]
        if raw != target_key:
            continue
        k = end
        while k < n and text[k] in " \t\r\n":
            k += 1
        if k >= n or text[k] != ":" or not mask[k]:
            continue

        val_end = find_value_end(text, mask, k)
        del_end = val_end
        p = del_end
        while p < n and text[p] in " \t":
            p += 1
        if p < n and text[p] == ",":
            del_end = p + 1
        while del_end < n and text[del_end] in " \t":
            del_end += 1
        if del_end < n and text[del_end : del_end + 2] == "\r\n":
            del_end += 2
        elif del_end < n and text[del_end] == "\n":
            del_end += 1

        line_start = text.rfind("\n", 0, start) + 1
        prefix = text[line_start:start]
        del_start = line_start if all(c in " \t" for c in prefix) else start
        new_text = clean_dangling_comma(text[:del_start] + text[del_end:], del_start)
        return new_text, True
    return text, False


def strip_signing_fields(text: str) -> str:
    for key in ("signingConfigs", "signingConfig"):
        while True:
            text, removed = remove_property_once(text, key)
            if not removed:
                break
    return text


def main() -> int:
    args = sys.argv[1:]
    if not args or any(flag in args for flag in ("-h", "--help")):
        print(f"Usage: {sys.argv[0]} [--check] <build-profile.json5>")
        return 1

    check_only = "--check" in args
    positional = [arg for arg in args if arg != "--check"]
    if len(positional) != 1:
        print(f"Usage: {sys.argv[0]} [--check] <build-profile.json5>", file=sys.stderr)
        return 1

    path = positional[0]
    with open(path, "r", encoding="utf-8") as fh:
        original = fh.read()

    updated = strip_signing_fields(original)
    changed = updated != original
    if check_only:
        print(f"{'would modify' if changed else 'clean'}: {path}")
        return 0

    if changed:
        with open(path, "w", encoding="utf-8") as fh:
            fh.write(updated)
        print(f"Stripped local signing fields from {path}")
    else:
        print(f"No local signing fields present in {path}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
