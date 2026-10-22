// Browser and link checks over the production build (called by verify.sh).
//
// 1. Internal link check over every built HTML file, honouring BASE_URL.
// 2. Serves build/ under BASE_URL and, for each route x theme x viewport:
//    screenshot, brand-token check, Flat 2.0 check, keyboard-focus check and
//    an axe-core scan that fails on serious or critical violations.
// Results: verify-output/report.json and verify-output/screenshots/*.png.
import fs from 'node:fs';
import http from 'node:http';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
import {chromium} from 'playwright';
import {AxeBuilder} from '@axe-core/playwright';
import {tokens, cssVar} from '../brand/tokens.mjs';

const siteRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const buildDir = path.join(siteRoot, 'build');
const outDir = path.join(siteRoot, 'verify-output');
const shotDir = path.join(outDir, 'screenshots');
const baseUrl = process.env.BASE_URL || '/';

const routes = [
  {id: 'home', path: ''},
  {id: 'docs-intro', path: 'docs/intro', mermaid: true},
  {id: 'docs-placeholder', path: 'docs/concepts/leases'},
];
const themes = ['light', 'dark'];
const viewports = [
  {id: 'desktop', width: 1440, height: 900},
  {id: 'mobile', width: 390, height: 844},
];
const AXE_TAGS = ['wcag2a', 'wcag2aa', 'wcag21a', 'wcag21aa', 'wcag22aa'];

const failures = [];
const fail = (msg) => failures.push(msg);

// ---------- 1. Link check ----------
function resolveBuilt(urlPath) {
  const clean = decodeURIComponent(urlPath.split(/[?#]/)[0]);
  if (!clean.startsWith(baseUrl)) return null;
  const rel = clean.slice(baseUrl.length);
  const candidates = [rel, `${rel}.html`, path.join(rel, 'index.html')];
  for (const c of candidates) {
    const file = path.join(buildDir, c);
    if (file.startsWith(buildDir) && fs.existsSync(file) && fs.statSync(file).isFile()) return file;
  }
  return null;
}

function* htmlFiles(dir) {
  for (const entry of fs.readdirSync(dir, {withFileTypes: true})) {
    const item = path.join(dir, entry.name);
    if (entry.isDirectory()) yield* htmlFiles(item);
    else if (entry.name.endsWith('.html')) yield item;
  }
}

function checkLinks() {
  let pages = 0;
  let links = 0;
  const attr = /\b(?:href|src)="([^"]+)"/g;
  for (const file of htmlFiles(buildDir)) {
    pages += 1;
    const html = fs.readFileSync(file, 'utf8');
    const pageUrl = '/' + path.relative(buildDir, file).split(path.sep).join('/');
    for (const match of html.matchAll(attr)) {
      const raw = match[1].replaceAll('&amp;', '&');
      if (/^(?:[a-z][a-z0-9+.-]*:|#|\/\/)/i.test(raw)) continue;
      links += 1;
      const target = raw.startsWith('/')
        ? raw
        : new URL(raw, `http://site${baseUrl}${pageUrl.slice(1)}`).pathname;
      if (!resolveBuilt(target)) fail(`broken internal link in ${pageUrl}: ${raw}`);
    }
  }
  return {pages, links};
}

// ---------- 2. Static server ----------
const MIME = {
  '.html': 'text/html; charset=utf-8', '.js': 'text/javascript', '.css': 'text/css',
  '.svg': 'image/svg+xml', '.json': 'application/json', '.woff2': 'font/woff2',
  '.woff': 'font/woff', '.png': 'image/png', '.ico': 'image/x-icon', '.txt': 'text/plain',
  '.xml': 'application/xml',
};

function serve() {
  const server = http.createServer((req, res) => {
    const file = resolveBuilt(new URL(req.url, 'http://x').pathname);
    if (!file) {
      res.writeHead(404, {'content-type': MIME['.html']});
      res.end(fs.readFileSync(path.join(buildDir, '404.html')));
      return;
    }
    res.writeHead(200, {'content-type': MIME[path.extname(file)] || 'application/octet-stream'});
    res.end(fs.readFileSync(file));
  });
  return new Promise((resolve) => server.listen(0, '127.0.0.1', () => resolve(server)));
}

// ---------- In-page checks ----------
function inPageAudit() {
  const out = {flat: [], vars: {}, computed: {}};
  const visible = (el) => {
    const r = el.getBoundingClientRect();
    const s = getComputedStyle(el);
    return r.width > 0 && r.height > 0 && s.visibility !== 'hidden' && s.display !== 'none';
  };
  const transparent = (c) => c === 'transparent' || /rgba?\([^)]*,\s*0\)$/.test(c) || /\/\s*0\)$/.test(c);
  const label = (el) => {
    const cls = typeof el.className === 'string' ? el.className.trim().split(/\s+/).slice(0, 2).join('.') : '';
    return `${el.tagName.toLowerCase()}${el.id ? '#' + el.id : ''}${cls ? '.' + cls : ''}`;
  };
  for (const el of document.querySelectorAll('body *')) {
    if (el.closest('svg') || !visible(el)) continue;
    const s = getComputedStyle(el);
    for (const side of ['Top', 'Right', 'Bottom', 'Left']) {
      if (parseFloat(s[`border${side}Width`]) > 0 && s[`border${side}Style`] !== 'none'
          && !transparent(s[`border${side}Color`])) {
        out.flat.push(`border-${side.toLowerCase()} on ${label(el)} (${s[`border${side}Color`]})`);
      }
    }
    if (s.boxShadow && s.boxShadow !== 'none') out.flat.push(`box-shadow on ${label(el)}`);
    if (/gradient\(/.test(s.backgroundImage)) out.flat.push(`gradient on ${label(el)}`);
    if (el !== document.activeElement && s.outlineStyle !== 'none' && parseFloat(s.outlineWidth) > 0
        && !transparent(s.outlineColor)) {
      out.flat.push(`decorative outline on ${label(el)}`);
    }
    if (el.tagName === 'HR' && el.getBoundingClientRect().height > 0 && !transparent(s.backgroundColor)) {
      out.flat.push(`visible rule on ${label(el)}`);
    }
  }
  const root = getComputedStyle(document.documentElement);
  for (const name of window.__plVars) out.vars[name] = root.getPropertyValue(name).trim();
  const bg = (sel) => {
    const el = document.querySelector(sel);
    return el ? getComputedStyle(el).backgroundColor : null;
  };
  out.computed = {
    theme: document.documentElement.dataset.theme,
    body: bg('body'),
    navbar: bg('.navbar'),
    footer: bg('.footer'),
    sidebar: bg('.theme-doc-sidebar-container'),
    primaryButton: bg('.button--primary'),
    mermaidEdge: (() => {
      const p = document.querySelector('.docusaurus-mermaid-container svg .flowchart-link, .docusaurus-mermaid-container svg .edgePath .path');
      return p ? getComputedStyle(p).stroke : null;
    })(),
    text: getComputedStyle(document.body).color,
  };
  return out;
}

// Minifiers shorten #FFFFFF to #fff; compare canonical six-digit upper case.
const canonHex = (value) => {
  const v = (value || '').trim().toUpperCase();
  return /^#[0-9A-F]{3}$/.test(v) ? `#${v[1]}${v[1]}${v[2]}${v[2]}${v[3]}${v[3]}` : v;
};

const hexToRgb = (hex) => {
  const n = parseInt(hex.slice(1), 16);
  return `rgb(${(n >> 16) & 255}, ${(n >> 8) & 255}, ${n & 255})`;
};

async function focusAudit(page) {
  const results = [];
  for (let i = 0; i < 4; i += 1) {
    await page.keyboard.press('Tab');
    results.push(await page.evaluate(() => {
      const el = document.activeElement;
      if (!el || el === document.body) return {ok: false, what: 'body'};
      const s = getComputedStyle(el);
      const ring = s.outlineStyle !== 'none' && parseFloat(s.outlineWidth) >= 2;
      const what = `${el.tagName.toLowerCase()} "${(el.textContent || el.getAttribute('aria-label') || '').trim().slice(0, 40)}"`;
      return {ok: ring, what, outline: `${s.outlineWidth} ${s.outlineStyle} ${s.outlineColor}`};
    }));
  }
  return results;
}

// ---------- Run ----------
async function main() {
  if (!fs.existsSync(buildDir)) throw new Error('build/ missing: run npm run build first');
  fs.mkdirSync(shotDir, {recursive: true});

  const linkStats = checkLinks();
  console.log(`link check: ${linkStats.links} internal links across ${linkStats.pages} pages`);

  const server = await serve();
  const origin = `http://127.0.0.1:${server.address().port}`;
  const browser = await chromium.launch();
  const report = {baseUrl, links: linkStats, runs: []};
  const varNames = Object.values(cssVar);

  try {
    for (const theme of themes) {
      for (const vp of viewports) {
        const context = await browser.newContext({
          viewport: {width: vp.width, height: vp.height},
          colorScheme: theme,
          reducedMotion: 'reduce',
        });
        await context.addInitScript(({t, names}) => {
          window.localStorage.setItem('theme', t);
          window.__plVars = names;
        }, {t: theme, names: varNames});
        const page = await context.newPage();
        for (const route of routes) {
          const tag = `${route.id}/${theme}/${vp.id}`;
          const url = `${origin}${baseUrl}${route.path}`;
          const response = await page.goto(url, {waitUntil: 'networkidle'});
          if (!response || response.status() !== 200) fail(`${tag}: HTTP ${response && response.status()}`);
          await page.evaluate(() => document.fonts.ready);
          if (route.mermaid) {
            await page.waitForSelector('.docusaurus-mermaid-container svg', {timeout: 15000})
              .catch(() => fail(`${tag}: Mermaid diagram did not render`));
          }

          const audit = await page.evaluate(inPageAudit);
          if (audit.computed.theme !== theme) fail(`${tag}: data-theme is ${audit.computed.theme}`);
          for (const issue of audit.flat) fail(`${tag}: Flat 2.0 violation: ${issue}`);

          const expected = tokens[theme];
          for (const [name, varName] of Object.entries(cssVar)) {
            if (canonHex(audit.vars[varName]) !== canonHex(expected[name])) {
              fail(`${tag}: token ${varName} is "${audit.vars[varName]}", expected ${expected[name]}`);
            }
          }
          const want = {
            body: expected.canvas, navbar: expected.chrome, footer: expected.chrome, text: expected.text,
          };
          if (vp.id === 'desktop' && route.path.startsWith('docs')) want.sidebar = expected.chrome;
          if (route.id === 'home') want.primaryButton = expected.ember;
          if (route.mermaid) want.mermaidEdge = expected.cyan;
          for (const [key, hex] of Object.entries(want)) {
            if (audit.computed[key] !== hexToRgb(hex)) {
              fail(`${tag}: ${key} colour ${audit.computed[key]}, expected ${hex}`);
            }
          }

          const shot = path.join(shotDir, `${route.id}-${theme}-${vp.id}.png`);
          await page.screenshot({path: shot, fullPage: true});

          const axe = await new AxeBuilder({page}).withTags(AXE_TAGS).analyze();
          const blocking = axe.violations.filter((v) => v.impact === 'serious' || v.impact === 'critical');
          for (const v of blocking) {
            fail(`${tag}: axe ${v.impact} ${v.id}: ${v.help} (${v.nodes.map((n) => n.target.join(' ')).slice(0, 3).join('; ')})`);
          }

          let focus = null;
          if (vp.id === 'desktop') {
            await page.goto(url, {waitUntil: 'networkidle'});
            focus = await focusAudit(page);
            for (const f of focus) if (!f.ok) fail(`${tag}: focus not visible on ${f.what}`);
          }

          report.runs.push({
            route: route.id, theme, viewport: vp.id, screenshot: path.relative(siteRoot, shot),
            axe: {
              violations: axe.violations.map((v) => ({id: v.id, impact: v.impact, nodes: v.nodes.length})),
              passes: axe.passes.length,
            },
            flatViolations: audit.flat.length, focus,
          });
          const minor = axe.violations.length - blocking.length;
          console.log(`${tag.padEnd(34)} axe blocking=${blocking.length} other=${minor} passes=${axe.passes.length} flat=${audit.flat.length}${focus ? ` focus=${focus.filter((f) => f.ok).length}/${focus.length}` : ''}`);
        }
        await context.close();
      }
    }
  } finally {
    await browser.close();
    server.close();
  }

  report.failures = failures;
  fs.writeFileSync(path.join(outDir, 'report.json'), JSON.stringify(report, null, 2));
  console.log(`report: ${path.relative(siteRoot, path.join(outDir, 'report.json'))}`);
  console.log(`screenshots: ${path.relative(siteRoot, shotDir)}/ (${report.runs.length})`);
  if (failures.length > 0) {
    console.error(`\nverification FAILED (${failures.length}):`);
    for (const f of failures) console.error(`  - ${f}`);
    process.exit(1);
  }
  console.log('browser checks passed');
}

main().catch((error) => {
  console.error(error);
  process.exit(1);
});
