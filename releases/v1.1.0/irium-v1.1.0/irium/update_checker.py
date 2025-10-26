"""Automatic update notification system for Irium nodes."""

import requests
import json
import os
from typing import Optional, Dict
# Simple version comparison without external deps

GITHUB_API = "https://api.github.com/repos/iriumlabs/irium/releases/latest"
UPDATE_CHECK_FILE = os.path.expanduser("~/.irium/last_update_check.json")


class UpdateChecker:
    """Check for and notify about available updates."""
    
    def __init__(self, current_version: str):
        self.current_version = current_version
        
    def check_for_updates(self) -> Optional[Dict]:
        """Check GitHub for newer version."""
        try:
            response = requests.get(GITHUB_API, timeout=5)
            if response.status_code == 200:
                release_data = response.json()
                latest_version = release_data.get('tag_name', '').lstrip('v')
                
                if self._is_newer(latest_version):
                    return {
                        'available': True,
                        'latest_version': latest_version,
                        'current_version': self.current_version,
                        'download_url': release_data.get('html_url'),
                        'release_notes': release_data.get('body', ''),
                        'published_at': release_data.get('published_at')
                    }
            return None
        except Exception as e:
            # Fail silently - don't interrupt node operation
            return None
    
    def _is_newer(self, remote_version: str) -> bool:
        """Compare versions (simple comparison)."""
        try:
            # Simple version comparison: 1.1.0 vs 1.2.0
            remote_parts = [int(x) for x in remote_version.split('.')]
            current_parts = [int(x) for x in self.current_version.split('.')]
            return remote_parts > current_parts
        except:
            return False
    
    def save_check_time(self):
        """Save last check timestamp."""
        import time
        data = {'last_check': int(time.time())}
        os.makedirs(os.path.dirname(UPDATE_CHECK_FILE), exist_ok=True)
        with open(UPDATE_CHECK_FILE, 'w') as f:
            json.dump(data, f)
    
    def should_check_now(self, interval_hours: int = 6) -> bool:
        """Check if enough time passed since last check."""
        import time
        if not os.path.exists(UPDATE_CHECK_FILE):
            return True
        try:
            with open(UPDATE_CHECK_FILE) as f:
                data = json.load(f)
            last_check = data.get('last_check', 0)
            return (time.time() - last_check) > (interval_hours * 3600)
        except:
            return True


def display_update_notification(update_info: Dict):
    """Display prominent update notification."""
    print("\n" + "=" * 60)
    print("🚨 UPDATE AVAILABLE! 🚨")
    print("=" * 60)
    print(f"Current Version: {update_info['current_version']}")
    print(f"Latest Version:  {update_info['latest_version']}")
    print(f"Released: {update_info['published_at']}")
    print("\nTo update, run these commands:")
    print("  cd ~/irium")
    print("  git pull origin main")
    print("  sudo systemctl restart irium-node.service")
    print("  sudo systemctl restart irium-miner.service")
    print(f"\nRelease notes: {update_info['download_url']}")
    print("=" * 60 + "\n")

    def save_check_time(self):
        """Save last check timestamp."""
        import time
        data = {'last_check': int(time.time())}
        os.makedirs(os.path.dirname(UPDATE_CHECK_FILE), exist_ok=True)
        with open(UPDATE_CHECK_FILE, 'w') as f:
            json.dump(data, f)

    def should_check_now(self, interval_hours: int = 6) -> bool:
        """Check if enough time passed since last check."""
        import time
        if not os.path.exists(UPDATE_CHECK_FILE):
            return True
        try:
            with open(UPDATE_CHECK_FILE) as f:
                data = json.load(f)
            last_check = data.get('last_check', 0)
            return (time.time() - last_check) > (interval_hours * 3600)
        except:
            return True


def display_update_notification(update_info: Dict):
    """Display prominent update notification."""
    print("\n" + "=" * 60)
    print("🚨 UPDATE AVAILABLE! 🚨")
    print("=" * 60)
    print(f"Current Version: {update_info['current_version']}")
    print(f"Latest Version:  {update_info['latest_version']}")
    print(f"Released: {update_info['published_at']}")
    print("\nTo update, run these commands:")
    print("  cd ~/irium")
    print("  git pull origin main")
    print("  sudo systemctl restart irium-node.service")
    print("  sudo systemctl restart irium-miner.service")
    print(f"\nRelease notes: {update_info['download_url']}")
    print("=" * 60 + "\n")
