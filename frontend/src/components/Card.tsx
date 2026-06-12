
import type { ReactNode } from 'react'
import { clsx } from 'clsx'

interface Props { title?: string; children: ReactNode; className?: string }

export default function Card({ title, children, className }: Props) {
  return (
    <div className={clsx('bg-zinc-900 border border-zinc-800 rounded-xl p-5', className)}>
      {title && <h2 className="text-sm font-medium text-zinc-400 mb-4 uppercase tracking-wider">{title}</h2>}
      {children}
    </div>
  )
}
