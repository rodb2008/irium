
export function satToIrm(sat: number): string {
  return (sat / 1e8).toLocaleString('en', { minimumFractionDigits: 2, maximumFractionDigits: 8 })
}

export function shortHash(h: string, n = 12): string {
  return h.length > n * 2 + 3 ? `${h.slice(0, n)}...${h.slice(-n)}` : h
}

export function timeAgo(iso: string): string {
  const secs = Math.floor((Date.now() - new Date(iso).getTime()) / 1000)
  if (secs < 60) return `${secs}s ago`
  if (secs < 3600) return `${Math.floor(secs / 60)}m ago`
  if (secs < 86400) return `${Math.floor(secs / 3600)}h ago`
  return `${Math.floor(secs / 86400)}d ago`
}

export function fmtTime(iso: string): string {
  return new Date(iso).toLocaleString()
}
