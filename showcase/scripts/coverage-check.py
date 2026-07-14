#!/usr/bin/env python3
"""Showcase fence coverage matrix scanner."""
import re, sys, glob, os

SHOWCASE_DIR = os.path.join(os.path.dirname(__file__), "..", "showcase")
RUNTIME_TAGS = ["div","header","nav","p","span","strong","em","br","label","button",
                "a","img","canvas","input","textarea","select","option","progress",
                "ul","ol","li","template","slot"]
INPUT_TYPES = ["text","password","search","number","range","checkbox","radio"]
FORBIDDEN = ["h1","h2","h3","h4","h5","h6","meter","dialog","details","summary",
             "form","fieldset","legend","main","section","footer","article","aside","small"]
CSS_GROUPS = {
  "sizing": ["width","height","min-width","min-height","max-width","max-height","aspect-ratio"],
  "layout": ["display","flex-direction","flex-wrap","flex-grow","justify-content","align-items","gap","order"],
  "position": ["position","top","right","bottom","left"],
  "box-model": ["padding","margin"],
  "border": ["border-color","border-radius","border-image-slice"],
  "background": ["background-color","background-image","background-size","background-clip"],
  "visual": ["opacity","box-shadow","pointer-events","transform","filter"],
  "text": ["color","font-size","font-family","font-weight","text-align","line-height","letter-spacing","white-space","text-shadow","-webkit-text-stroke","font-effect"],
  "overflow": ["overflow-x","overflow-y"],
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

    if not custom:
        errors.append("MISSING custom-element (no hyphenated tag found)")

    for tag in FORBIDDEN:
        if tag in found_lower:
            errors.append("FORBIDDEN tag found: <%s>" % tag)

    for it in INPUT_TYPES:
        if 'type="%s"' % it not in body:
            errors.append("MISSING input type: %s" % it)

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
    print("COVERAGE OK: all 23 tags + custom-element + 7 input types + 9 CSS groups covered, no forbidden tags.")

if __name__ == "__main__":
    main()
