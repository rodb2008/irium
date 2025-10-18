# Irium v1.1.1 - Self-Detection Fix

## Critical Fix

Fixed self-detection bug that prevented users from connecting to seed node.

**Issue:** v1.1.0 had hardcoded seed IP in self-check, causing user nodes to skip the seed.
**Fix:** Dynamic self-detection using actual node IP.

All v1.1.0 users should update immediately!
