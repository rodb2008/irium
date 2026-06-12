
import type { ReactNode } from 'react'

export default function StatRow({ label, value }: { label: string; value: ReactNode }) {
  return (
    <div className="flex justify-between py-2 border-b border-zinc-800 last:border-0">
      <span className="text-zinc-400 text-sm">{label}</span>
      <span className="text-sm mono text-zinc-100">{value}</span>
    </div>
  )
}
