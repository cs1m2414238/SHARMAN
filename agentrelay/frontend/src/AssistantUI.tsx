import type { ReactNode } from 'react'

const paths = {
  chat: 'M21 11.5a8.4 8.4 0 0 1-.9 3.8 8.5 8.5 0 0 1-7.6 4.7 8.4 8.4 0 0 1-3.8-.9L3 21l1.9-5.7a8.4 8.4 0 0 1-.9-3.8 8.5 8.5 0 0 1 4.7-7.6 8.4 8.4 0 0 1 3.8-.9h.5a8.5 8.5 0 0 1 8 8v.5Z',
  plus: 'M12 5v14M5 12h14',
  close: 'm6 6 12 12M6 18 18 6',
  search: 'm21 21-4.4-4.4M19 11a8 8 0 1 1-16 0 8 8 0 0 1 16 0Z',
  inbox: 'M4 4h16l2 12v4H2v-4L4 4Zm-2 12h6l2 3h4l2-3h6',
  memory: 'M9 3a3 3 0 0 0-3 3 4 4 0 0 0-2 7 4 4 0 0 0 4 7c1 2 4 1 4-1V6c0-2-1-3-3-3Zm6 0a3 3 0 0 1 3 3 4 4 0 0 1 2 7 4 4 0 0 1-4 7c-1 2-4 1-4-1M7 9l2 1m8-1-2 1M7 16l2-1m8 1-2-1',
  shield: 'm12 3 8 3v6c0 5-8 9-8 9s-8-4-8-9V6l8-3Zm-4 9 3 3 5-6',
  up: 'M12 19V5m-6 6 6-6 6 6',
  down: 'M12 5v14m-6-6 6 6 6-6',
  copy: 'M9 9h12v12H9V9ZM15 5V3H3v12h2',
  mic: 'M9 6a3 3 0 0 1 6 0v6a3 3 0 0 1-6 0V6Zm-3 5v1a6 6 0 0 0 12 0v-1M12 18v3m-3 0h6',
  panel: 'M3 4h18v16H3V4Zm5 0v16',
  check: 'm5 12 4 4L19 6',
  file: 'M14 2H4v20h16V8l-6-6Zm0 0v6h6M8 13h8m-8 4h5',
  clock: 'M12 8v4l3 2m6-2a9 9 0 1 1-18 0 9 9 0 0 1 18 0Z',
} as const

export function ChatIcon({ name }: { name: keyof typeof paths }) {
  return <svg width="18" height="18" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.65" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true"><path d={paths[name]} /></svg>
}

function inline(text: string): ReactNode[] {
  return text.split(/(\*\*[^*\n]+\*\*|`[^`\n]+`)/g).map((part, i) => part.startsWith('**') && part.endsWith('**')
    ? <strong key={i}>{part.slice(2, -2)}</strong>
    : part.startsWith('`') && part.endsWith('`') ? <code key={i}>{part.slice(1, -1)}</code> : part)
}

// Text-only formatting: never interpret model HTML or activate model URLs.
export function MessageText({ text }: { text: string }) {
  return <>{text.split(/(```[\s\S]*?```)/g).filter(Boolean).map((block, i) => {
    if (block.length >= 6 && block.startsWith('```') && block.endsWith('```')) return <pre key={i}><code>{block.slice(3, -3).replace(/^[\w+#.-]*\n/, '')}</code></pre>
    return block.split(/\n\s*\n/).filter(Boolean).map((paragraph, j) => {
      const lines = paragraph.trim().split('\n')
      if (lines.every((line) => /^\s*[-*] /.test(line))) return <ul key={`${i}-${j}`}>{lines.map((line, k) => <li key={k}>{inline(line.replace(/^\s*[-*] /, ''))}</li>)}</ul>
      return <p key={`${i}-${j}`}>{inline(paragraph.replace(/^#{1,6} /gm, ''))}</p>
    })
  })}</>
}
