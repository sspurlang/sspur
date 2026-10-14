#!/usr/bin/env python3
"""Check every internal link in the built site (docs/book by default).

A link is internal when it has no scheme. Its file must exist, and a #fragment must
match an id in that file. Links that leave the book (to ../README.md, a source
directory, and so on) are reported too, since they 404 on GitHub Pages. Absolute links
to the deployed site itself (such as SSPUR Pulse's footer links to the docs) are
checked as internal links too.
Exits 1 if anything is broken.
"""

import html.parser
import os
import sys
import urllib.parse

SITE = "https://utkarshavardhana.github.io/sspur/"


class Page(html.parser.HTMLParser):
    def __init__(self):
        super().__init__()
        self.links = []
        self.ids = set()

    def handle_starttag(self, tag, attrs):
        a = dict(attrs)
        if "id" in a:
            self.ids.add(a["id"])
        if tag == "a" and "name" in a:
            self.ids.add(a["name"])
        for key in ("href", "src"):
            if a.get(key) and tag in ("a", "link", "script", "img"):
                self.links.append(a[key])


def parse(path, cache):
    if path not in cache:
        p = Page()
        with open(path, encoding="utf-8") as f:
            p.feed(f.read())
        cache[path] = p
    return cache[path]


def main():
    root = os.path.abspath(sys.argv[1] if len(sys.argv) > 1 else os.path.join(os.path.dirname(__file__), "..", "docs", "book"))
    if not os.path.isfile(os.path.join(root, "index.html")):
        sys.exit(f"no built site at {root}; run `mdbook build docs` first")
    cache, broken, checked = {}, [], 0
    for dirpath, _, names in os.walk(root):
        for name in sorted(names):
            if not name.endswith(".html"):
                continue
            page_path = os.path.join(dirpath, name)
            page = parse(page_path, cache)
            for link in page.links:
                if link.startswith(SITE):
                    link = os.path.relpath(root, dirpath).replace(os.sep, "/") + "/" + link[len(SITE):]
                url = urllib.parse.urlsplit(link)
                if url.scheme or url.netloc or link.startswith("mailto:"):
                    continue
                checked += 1
                target = page_path if not url.path else os.path.normpath(os.path.join(dirpath, urllib.parse.unquote(url.path)))
                if os.path.isdir(target):
                    target = os.path.join(target, "index.html")
                rel = os.path.relpath(page_path, root)
                if os.path.relpath(target, root).startswith(".."):
                    broken.append(f"{rel}: {link} leaves the site")
                elif not os.path.isfile(target):
                    broken.append(f"{rel}: {link} does not exist")
                elif url.fragment and target.endswith(".html"):
                    if urllib.parse.unquote(url.fragment) not in parse(target, cache).ids:
                        broken.append(f"{rel}: {link} has no #{url.fragment}")
    for b in broken:
        print(b)
    print(f"{checked} internal links in {len(cache)} pages checked, {len(broken)} broken")
    sys.exit(1 if broken else 0)


if __name__ == "__main__":
    main()
