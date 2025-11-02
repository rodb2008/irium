import os
def load_seedlist(path=None):
    path = path or os.path.join(os.path.dirname(__file__), '..', '..', 'bootstrap', 'seedlist.txt')
    try:
        with open(path, 'r') as f:
            lines = [l.strip() for l in f if l.strip() and not l.strip().startswith('#')]
            # allow ip:port entries or ip entries
            return lines
    except FileNotFoundError:
        return []
