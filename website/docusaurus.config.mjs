// Prometheus Parking Lot documentation site.
// Scaffolded with the build-branded-docusaurus skill. Brand tokens (know-me
// UI/UX standard, Flat 2.0) live in brand/tokens.mjs and src/css/custom.css.
import {codeThemes} from './src/theme/codeThemes.mjs';

// Local development defaults. GitHub Pages builds set
// SITE_URL=https://prometheus-ags.github.io BASE_URL=/prometheus-parking-lot-rs/
const siteUrl = process.env.SITE_URL || 'http://localhost:3000';
const baseUrl = process.env.BASE_URL || '/';

const repoUrl = 'https://github.com/Prometheus-AGS/prometheus-parking-lot-rs';

/** @type {import('@docusaurus/types').Config} */
const config = {
  title: 'Prometheus Parking Lot',
  tagline: 'A resource-aware parking-lot scheduler for Rust services and AI agents.',
  favicon: 'img/favicon.svg',
  url: siteUrl,
  baseUrl,
  trailingSlash: false,
  onBrokenLinks: 'throw',
  onBrokenAnchors: 'throw',
  organizationName: 'Prometheus-AGS',
  projectName: 'prometheus-parking-lot-rs',
  i18n: {defaultLocale: 'en', locales: ['en']},
  markdown: {
    mermaid: true,
    hooks: {onBrokenMarkdownLinks: 'throw'},
  },
  themes: ['@docusaurus/theme-mermaid'],
  // Relative paths only: absolute paths would be serialised into the client bundle.
  clientModules: ['./src/fonts.js'],
  presets: [
    [
      'classic',
      {
        docs: {sidebarPath: './sidebars.js', routeBasePath: 'docs'},
        blog: false,
        theme: {customCss: './src/css/custom.css'},
      },
    ],
  ],
  plugins: [
    [
      '@easyops-cn/docusaurus-search-local',
      {
        hashed: true,
        indexDocs: true,
        indexPages: false,
        indexBlog: false,
        docsRouteBasePath: '/docs',
        language: ['en'],
        highlightSearchTermsOnTargetPage: false,
        searchResultLimits: 8,
      },
    ],
  ],
  themeConfig: {
    image: 'img/logo.svg',
    colorMode: {defaultMode: 'dark', respectPrefersColorScheme: true},
    navbar: {
      title: 'Prometheus Parking Lot',
      logo: {alt: 'Prometheus Parking Lot monogram', src: 'img/logo.svg', width: 32, height: 32},
      items: [
        {to: '/docs/intro', label: 'Overview', position: 'left'},
        {to: '/docs/concepts', label: 'Concepts', position: 'left'},
        {to: '/docs/guides', label: 'Guides', position: 'left'},
        {href: repoUrl, label: 'GitHub', position: 'right'},
      ],
    },
    footer: {
      links: [
        {
          title: 'Documentation',
          items: [
            {label: 'Overview', to: '/docs/intro'},
            {label: 'Concepts', to: '/docs/concepts'},
            {label: 'Guides', to: '/docs/guides'},
          ],
        },
        {
          title: 'Project',
          items: [
            {label: 'Source on GitHub', href: repoUrl},
            {label: 'Changelog', href: `${repoUrl}/blob/main/CHANGELOG.md`},
          ],
        },
      ],
      copyright: 'Prometheus Parking Lot is MIT licensed.',
    },
    prism: {
      theme: codeThemes.light,
      darkTheme: codeThemes.dark,
      additionalLanguages: ['toml', 'bash'],
    },
    mermaid: {theme: {light: 'base', dark: 'base'}},
  },
};

export default config;
