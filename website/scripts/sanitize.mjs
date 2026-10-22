// Public-content sanitizer (vendored from build-branded-docusaurus, extended).
//
// Fails when site sources contain machine-local paths, secret-like values or
// raw agent wiki/session content. Runs before every build.
//
//   node scripts/sanitize.mjs            scan the default site sources
//   node scripts/sanitize.mjs DIR...     scan the given directories or files
import fs from 'node:fs';
import path from 'node:path';
import {fileURLToPath} from 'node:url';

const siteRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');

const DEFAULT_SOURCES = [
  'docs',
  'src',
  'static',
  'brand',
  'docusaurus.config.mjs',
  'sidebars.js',
];

const SCANNED = /\.(?:md|mdx|js|jsx|mjs|cjs|ts|tsx|json|ya?ml|css|html|svg|txt)$/i;

// Patterns are built from fragments so this file does not match itself if it
// is ever scanned.
const rules = [
  {name: 'machine-local path (macOS home)', re: new RegExp('/' + 'Users/[A-Za-z0-9._-]+')},
  {name: 'machine-local path (mounted volume)', re: new RegExp('/' + 'Volumes/[A-Za-z0-9._-]+')},
  {name: 'machine-local path (Linux home)', re: new RegExp('/' + 'home/[a-z_][a-z0-9_-]*/')},
  {name: 'machine-local path (Windows profile)', re: new RegExp('[A-Za-z]:\\\\' + 'Users\\\\', 'i')},
  {name: 'private key block', re: new RegExp('BEGIN (?:RSA |EC |DSA |OPENSSH |ENCRYPTED )?' + 'PRIVATE KEY')},
  {name: 'AWS access key id', re: /\b(?:AKIA|ASIA)[0-9A-Z]{16}\b/},
  {name: 'GitHub token', re: /\b(?:gh[pousr]_[A-Za-z0-9]{36,}|github_pat_[A-Za-z0-9_]{40,})\b/},
  {name: 'Slack token', re: /\bxox[abposr]-[A-Za-z0-9-]{10,}\b/},
  {name: 'API key literal', re: /\b(?:sk|pk|rk)-(?:live|test|proj|ant)?[-_]?[A-Za-z0-9]{24,}\b/},
  {name: 'bearer token literal', re: /\bBearer\s+[A-Za-z0-9._~+/-]{24,}=*/},
  {
    name: 'credential assignment',
    re: /\b(?:api[_-]?key|client[_-]?secret|secret[_-]?key|access[_-]?token|auth[_-]?token|password|passwd)\b\s*[:=]\s*["'][^"'\s]{8,}["']/i,
  },
  {name: 'connection string with password', re: /\b[a-z][a-z0-9+.-]*:\/\/[^\s:/@]+:[^\s@/]{4,}@/i},
  {name: 'raw agent knowledge/wiki path', re: new RegExp('\\.' + 'prometheus/(?:knowledge|events|recovery|sessions)')},
  {name: 'raw agent wiki session page', re: new RegExp('karpathy' + '-session-[0-9a-f]{8,}')},
  {name: 'KBD runtime state path', re: new RegExp('\\.' + 'kbd-orchestrator/[^\\s)]*\\.jsonl?')},
  {name: 'raw session event log', re: new RegExp('events' + '\\.jsonl')},
];

function* walk(target) {
  const stat = fs.statSync(target);
  if (stat.isFile()) {
    yield target;
    return;
  }
  for (const entry of fs.readdirSync(target, {withFileTypes: true})) {
    if (entry.name === 'node_modules' || entry.name.startsWith('.docusaurus')) continue;
    const item = path.join(target, entry.name);
    if (entry.isDirectory()) yield* walk(item);
    else if (SCANNED.test(entry.name)) yield item;
  }
}

function scan(targets) {
  const findings = [];
  let files = 0;
  for (const target of targets) {
    if (!fs.existsSync(target)) continue;
    for (const file of walk(target)) {
      files += 1;
      const lines = fs.readFileSync(file, 'utf8').split(/\r?\n/);
      lines.forEach((line, index) => {
        for (const rule of rules) {
          if (rule.re.test(line)) {
            findings.push(`${path.relative(siteRoot, file) || file}:${index + 1}: ${rule.name}`);
          }
        }
      });
    }
  }
  return {findings, files};
}

const args = process.argv.slice(2);
const targets = args.length > 0
  ? args.map((arg) => path.resolve(arg))
  : DEFAULT_SOURCES.map((source) => path.join(siteRoot, source));

const {findings, files} = scan(targets);
if (findings.length > 0) {
  console.error(`public-content sanitization FAILED (${findings.length} finding(s) in ${files} file(s)):`);
  for (const finding of findings) console.error(`  ${finding}`);
  process.exit(1);
}
console.log(`public-content sanitization passed (${files} file(s) scanned)`);
