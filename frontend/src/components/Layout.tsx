
import { Link, Outlet } from 'react-router-dom'
import SearchBar from './SearchBar'

export default function Layout() {
  return (
    <div className="min-h-screen flex flex-col">
      <header className="border-b border-zinc-800 bg-zinc-950/80 backdrop-blur sticky top-0 z-50">
        <div className="max-w-7xl mx-auto px-4 py-3 flex items-center gap-6">
          <Link to="/" className="flex items-center gap-2 shrink-0">
            <img src="/irium-logo.png" alt="Irium" className="h-7 w-7 object-contain" />
            <span className="font-semibold text-white tracking-wide">Explorer</span>
          </Link>
          <nav className="flex items-center gap-5 text-sm text-zinc-400">
            <Link to="/" className="hover:text-white transition-colors">Blocks</Link>
            <Link to="/miners" className="hover:text-white transition-colors">Miners</Link>
          </nav>
          <div className="ml-auto w-80">
            <SearchBar />
          </div>
        </div>
      </header>
      <main className="flex-1 max-w-7xl mx-auto w-full px-4 py-8">
        <Outlet />
      </main>
      <footer className="border-t border-zinc-800 text-center text-xs text-zinc-600 py-4">
        Irium Blockchain Explorer
      </footer>
    </div>
  )
}
