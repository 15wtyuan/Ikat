#!/usr/bin/env python3
"""Showcase fence coverage matrix scanner.

Verifies the showcase HTML exercises the full fence surface: the 6 runtime
tags, the WAI-ARIA control/list roles, the data-slot visual parts, every CSS
group, and a custom element. Controls and lists have no dedicated tag in the
fence -- authors express them with `role` on a `<div>` -- so coverage is
measured in tags + roles, not in retired tags like input/select/ul.
"""
import re, sys, glob, os

SHOWCASE_DIR = os.path.join(os.path.dirname(__file__), "..", "showcase")

# 6 runtime fence tags (div/span/button/img/template/slot).
RUNTIME_TAGS = ["div", "span", "button", "img", "template", "slot"]

# WAI-ARIA roles that drive control/list SemanticKind (fence §2.3).
ROLES = ["combobox", "listbox", "option", "slider", "spinbutton", "switch",
         "radio", "progressbar", "textbox", "list", "listitem"]

# Control visual parts expressed with `data-slot` (fence §2.3).
DATA_SLOTS = ["fill", "thumb", "value"]

FORBIDDEN = ["h1", "h2", "h3", "h4", "h5", "h6", "meter", "dialog", "details", "summary",
             "form", "fieldset", "legend", "main", "section", "footer", "article", "aside", "small",
             # retired fence tags must not reappear
             "input", "textarea", "select", "option", "progress", "ul", "ol", "li",
             "p", "header", "nav", "canvas", "strong", "em", "br", "label", "a"]

CSS_GROUPS = {
  "sizing": ["width", "height", "min-width", "min-height", "max-width", "max-height", "aspect-ratio"],
  "layout": ["display", "flex-direction", "flex-wrap", "flex-grow", "justify-content", "align-items", "gap", "order"],
  "position": ["position", "top", "right", "bottom", "left"],
  "box-model": ["padding", "margin"],
  "border": ["border-color", "border-radius", "border-image-slice"],
  "background": ["background-color", "background-image", "background-size", "background-clip"],
  "visual": ["opacity", "box-shadow", "pointer-events", "transform", "filter"],
  "text": ["color", "font-size", "font-family", "font-weight", "text-align", "line-height", "letter-spacing", "white-space", "text-shadow", "-webkit-text-stroke", "font-effect"],
  "overflow": ["overflow-x", "overflow-y"],
}

def body_html():
    all_html = ""
    for f in glob.glob(os.path.join(SHOWCASE_DIR, "*.html")):
        t = open(f, encoding="utf-8").read()
        m = re.search(r"<body[^>]*>(.*)</body>", t, re.S | re.I)
        all_html += m.group(1) if m else t
    return all_html

def main():
    body = body_html()
    tag_re = re.compile(r"<(\w[\w-]*)")
    found_tags = set(tag_re.findall(body))
    found_lower = {t.lower() for t in found_tags}
    custom = {t for t in found_lower if "-" in t}
    errors = []

    for tag in RUNTIME_TAGS:
        if tag not in found_lower:
            errors.append("MISSING tag: <%s>" % tag)

    for role in ROLES:
        if 'role="%s"' % role not in body:
            errors.append("MISSING role: %s" % role)

    for slot in DATA_SLOTS:
        if 'data-slot="%s"' % slot not in body:
            errors.append("MISSING data-slot: %s" % slot)

    if not custom:
        errors.append("MISSING custom-element (no hyphenated tag found)")

    for tag in FORBIDDEN:
        if tag in found_lower:
            errors.append("FORBIDDEN/retired tag found: <%s>" % tag)

    full = ""
    for f in glob.glob(os.path.join(SHOWCASE_DIR, "*.html")):
        full += open(f, encoding="utf-8").read()
    for group, props in CSS_GROUPS.items():
        hit = any(re.search(r"\b" + re.escape(p) + r"\b", full) for p in props)
        if not hit:
            errors.append("MISSING CSS group: %s" % group)

    if errors:
        print("COVERAGE GAPS:")
        for e in errors:
            print("  -", e)
        sys.exit(1)
    print("COVERAGE OK: 6 tags + %d roles + data-slots + custom-element + 9 CSS groups covered, no retired tags." % len(ROLES))

if __name__ == "__main__":
    main()
