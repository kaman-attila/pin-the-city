import { readFileSync, rmSync, writeFileSync } from 'node:fs'
import { pathToFileURL } from 'node:url'

const SITE = 'https://pinthecity.me'

const PAGES = [
  { file: 'index.html', path: '/', render: 'menu' },
  {
    file: 'privacy.html',
    path: '/privacy',
    render: 'privacy',
    title: 'Privacy - PinTheCity',
    description:
      'What PinTheCity stores, how long it is kept, who else is involved and how to delete your account and data.',
  },
  {
    file: 'archive.html',
    path: '/archive',
    render: 'archive',
    title: 'Daily Archive - PinTheCity',
    description:
      'Every PinTheCity daily challenge so far. Play the five-city geography puzzles you missed, or look back at how you did.',
  },
]

const { pages } = await import(pathToFileURL('dist-ssr/prerender.js').href)
const template = readFileSync('dist/index.html', 'utf8')

function attr(value) {
  return value.replace(/&/g, '&amp;').replace(/"/g, '&quot;').replace(/</g, '&lt;')
}

function setOnce(html, pattern, value) {
  if (!pattern.test(html)) throw new Error(`prerender: ${pattern} not found in dist/index.html`)
  return html.replace(pattern, (_, before, after) => before + value + after)
}

for (const page of PAGES) {
  let html = setOnce(template, /(<div id="root">)(<\/div>)/, pages[page.render]())
  if (page.title) {
    html = setOnce(html, /(<title>)[^<]*(<\/title>)/, attr(page.title))
    html = setOnce(html, /(<meta property="og:title" content=")[^"]*(")/, attr(page.title))
  }
  if (page.description) {
    html = setOnce(html, /(<meta\s+name="description"\s+content=")[^"]*(")/, attr(page.description))
    html = setOnce(
      html,
      /(<meta\s+property="og:description"\s+content=")[^"]*(")/,
      attr(page.description),
    )
  }
  html = setOnce(html, /(<link rel="canonical" href=")[^"]*(")/, SITE + page.path)
  html = setOnce(html, /(<meta property="og:url" content=")[^"]*(")/, SITE + page.path)
  writeFileSync(`dist/${page.file}`, html)
}

rmSync('dist-ssr', { recursive: true, force: true })
