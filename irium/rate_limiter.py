"""Simple rate limiter for API endpoints."""

import time
from collections import defaultdict
from typing import Dict, Tuple


class RateLimiter:
    """Token bucket rate limiter."""
    
    def __init__(self, requests_per_minute: int = 60):
        self.requests_per_minute = requests_per_minute
        self.buckets: Dict[str, Tuple[int, float]] = defaultdict(lambda: (requests_per_minute, time.time()))
        self.cleanup_interval = 300  # Clean up old entries every 5 minutes
        self.last_cleanup = time.time()
    
    def is_allowed(self, client_ip: str) -> bool:
        """Check if request from client_ip is allowed."""
        current_time = time.time()
        
        # Periodic cleanup
        if current_time - self.last_cleanup > self.cleanup_interval:
            self._cleanup()
        
        tokens, last_update = self.buckets[client_ip]
        
        # Refill tokens based on time passed
        time_passed = current_time - last_update
        tokens_to_add = int(time_passed * (self.requests_per_minute / 60.0))
        tokens = min(self.requests_per_minute, tokens + tokens_to_add)
        
        if tokens > 0:
            self.buckets[client_ip] = (tokens - 1, current_time)
            return True
        else:
            self.buckets[client_ip] = (0, current_time)
            return False
    
    def _cleanup(self):
        """Remove old entries."""
        current_time = time.time()
        to_remove = []
        for ip, (tokens, last_update) in self.buckets.items():
            if current_time - last_update > 3600:  # Remove after 1 hour
                to_remove.append(ip)
        for ip in to_remove:
            del self.buckets[ip]
        self.last_cleanup = current_time
