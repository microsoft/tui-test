import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import test from "node:test";
import { fileURLToPath } from "node:url";

const root = fileURLToPath(new URL("../../", import.meta.url));
const docsRoot = path.join(root, "docs", "website");
const allowedLanguages = ["JavaScript", "Python", "Rust", "Go", "CLI"];

function slugifyHeading(raw) {
  return raw
    .replace(/`([^`]*)`/g, "$1")
    .replace(/\[([^\]]+)\]\([^)]*\)/g, "$1")
    .toLowerCase()
    .trim()
    .replace(/[^\p{L}\p{N}\s-]/gu, "")
    .replace(/\s+/g, "-");
}

function stripCodeFences(content) {
  const lines = content.split(/\r?\n/);
  const kept = [];
  let fence = null;
  for (const line of lines) {
    const match = line.match(/^\s*(`{3,}|~{3,})/);
    if (match) {
      if (!fence) fence = match[1][0];
      else if (match[1][0] === fence) fence = null;
      continue;
    }
    if (!fence) kept.push(line);
  }
  return kept.join("\n");
}

function collectFiles(directory) {
  const results = [];
  for (const entry of fs.readdirSync(directory, { withFileTypes: true })) {
    const full = path.join(directory, entry.name);
    assert(!entry.isSymbolicLink(), `Documentation must not contain symlinks: ${full}`);
    if (entry.isDirectory()) results.push(...collectFiles(full));
    else results.push(full);
  }
  return results;
}

function collectSidebarSlugs(items, slugs = new Set()) {
  assert(Array.isArray(items) && items.length > 0, "Sidebar items must be a non-empty array");
  for (const item of items) {
    if (typeof item === "string") {
      slugs.add(item);
      continue;
    }
    assert(item && typeof item === "object", `Invalid sidebar item: ${JSON.stringify(item)}`);
    if (typeof item.slug === "string") {
      slugs.add(item.slug);
    } else if (item.autogenerate && typeof item.autogenerate.directory === "string") {
      const targetDir = path.join(docsRoot, item.autogenerate.directory);
      assert(fs.existsSync(targetDir) && fs.statSync(targetDir).isDirectory(),
        `Missing autogenerate directory: ${item.autogenerate.directory}`);
      for (const file of collectFiles(targetDir)) {
        if (file.endsWith(".mdx") || file.endsWith(".md")) {
          slugs.add(path.relative(docsRoot, file).split(path.sep).join("/").replace(/\.(mdx|md)$/, ""));
        }
      }
    } else if (Array.isArray(item.items)) {
      assert(typeof item.label === "string" && item.label.length > 0, "Sidebar group requires a label");
      collectSidebarSlugs(item.items, slugs);
    } else if (typeof item.link === "string") {
      assert(item.link.startsWith("/"), `Sidebar link must start with /: ${item.link}`);
    } else {
      assert.fail(`Unsupported sidebar item: ${JSON.stringify(item)}`);
    }
  }
  return slugs;
}

test("website navigation, slugs, tabs, and links stay valid", () => {
  const navigationPath = path.join(docsRoot, "navigation.json");
  const navigation = JSON.parse(fs.readFileSync(navigationPath, "utf8"));

  const pages = new Map();
  for (const file of collectFiles(docsRoot)) {
    if (!file.endsWith(".mdx") && !file.endsWith(".md")) continue;
    const relative = path.relative(docsRoot, file).split(path.sep).join("/");
    const slug = relative.replace(/(?:\/index)?\.(mdx|md)$/, "");
    const route = `/docs/${slug}/`;
    const raw = fs.readFileSync(file, "utf8");
    const frontmatter = raw.match(/^---\r?\n([\s\S]*?)\r?\n---/);
    assert(frontmatter, `${relative} is missing YAML frontmatter`);
    assert(/^title:\s*\S+/m.test(frontmatter[1]), `${relative} is missing frontmatter title`);
    assert(/^description:\s*\S+/m.test(frontmatter[1]), `${relative} is missing frontmatter description`);

    const body = stripCodeFences(raw.slice(frontmatter[0].length));
    assert(!/[—–“”‘’]/.test(body), `${relative} contains disallowed dash or curly quote punctuation in prose`);

    const headings = new Set();
    const counts = new Map();
    for (const match of body.matchAll(/^#{1,6}\s+(.+)$/gm)) {
      const base = slugifyHeading(match[1]);
      const seen = counts.get(base) ?? 0;
      counts.set(base, seen + 1);
      headings.add(seen === 0 ? base : `${base}-${seen}`);
    }

    for (const group of raw.matchAll(/<Tabs\b([^>]*)>([\s\S]*?)<\/Tabs>/g)) {
      const attrs = group[1];
      const inner = group[2];
      const labels = [...inner.matchAll(/<TabItem\s+label="([^"]+)"/g)].map(m => m[1]);
      assert(labels.length >= 2, `${relative} has a <Tabs> group with fewer than 2 items`);
      if (attrs.includes('syncKey="example-language"')) {
        let previousIndex = -1;
        for (const label of labels) {
          const index = allowedLanguages.indexOf(label);
          assert(index !== -1, `${relative} uses invalid synced language label "${label}"`);
          assert(index > previousIndex, `${relative} orders "${label}" out of canonical order (${allowedLanguages.join(", ")})`);
          previousIndex = index;
        }
      }
    }

    pages.set(route, { relative, raw, body, headings });
  }

  assert(pages.has(navigation.entry), `navigation.json entry ${navigation.entry} does not match a page`);
  assert(Array.isArray(navigation.redirects), "navigation.json redirects must be an array");
  for (const redirect of navigation.redirects) {
    assert(redirect.from.startsWith("/docs"), `Redirect source must be under /docs: ${redirect.from}`);
    assert(pages.has(redirect.to), `Redirect target ${redirect.to} does not match a page`);
  }

  const sidebarSlugs = collectSidebarSlugs(navigation.sidebar);
  for (const slug of sidebarSlugs) {
    assert(pages.has(`/docs/${slug}/`), `navigation.json references missing slug "${slug}"`);
  }
  for (const route of pages.keys()) {
    const slug = route.slice("/docs/".length, -1);
    assert(sidebarSlugs.has(slug), `Page ${slug} is not reachable from navigation.json sidebar`);
  }

  function verifyDocHref(sourceLabel, href) {
    const url = new URL(href, "https://tui-test.dev");
    if (url.origin !== "https://tui-test.dev") return;
    if (url.pathname.startsWith("/docs-assets/")) {
      if (url.pathname === "/docs-assets/trace-viewer/index.html") {
        const viewerTemplate = path.join(root, "crates", "tui-test", "assets", "trace-viewer", "report.html");
        assert(fs.existsSync(viewerTemplate), `${sourceLabel} requires ${viewerTemplate}`);
        return;
      }
      const assetPath = path.join(docsRoot, "assets", url.pathname.slice("/docs-assets/".length));
      assert(fs.existsSync(assetPath), `${sourceLabel} links to missing asset ${href}`);
      return;
    }
    if (!url.pathname.startsWith("/docs/")) return;
    if (url.pathname === "/docs/") return;
    const target = pages.get(url.pathname);
    assert(target, `${sourceLabel} links to missing documentation route ${href}`);
    if (url.hash) {
      const anchor = decodeURIComponent(url.hash.slice(1));
      assert(target.headings.has(anchor),
        `${sourceLabel} links to missing heading #${anchor} on ${url.pathname} (available: ${[...target.headings].join(", ")})`);
    }
  }

  for (const { relative, body } of pages.values()) {
    for (const [, target] of body.matchAll(/\]\(([^)\s]+)\)/g)) {
      if (target.startsWith("/docs") || target.startsWith("https://tui-test.dev/docs")) {
        verifyDocHref(relative, target);
      }
    }
  }

  for (const repoDoc of [
    "README.md",
    "bindings/go/README.md",
    "bindings/js/README.md",
    "bindings/python/README.md",
    "references/javascript.md",
    "references/python.md",
  ]) {
    const content = stripCodeFences(fs.readFileSync(path.join(root, repoDoc), "utf8"));
    for (const [fullUrl] of content.matchAll(/https:\/\/tui-test\.dev\/docs\/[^\s)"'>]*/g)) {
      verifyDocHref(repoDoc, fullUrl);
    }
  }
});
