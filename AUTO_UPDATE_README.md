# Irium Auto-Update System

## Automatic Update Notifications

Your Irium node now automatically checks for updates every 6 hours and displays notifications when new versions are available.

## Manual Update (Recommended)

When you see an update notification, run:
```bash
cd ~/irium
git pull origin main
sudo systemctl restart irium-node.service
sudo systemctl restart irium-miner.service
```

## Opt-in Auto-Update

For automatic updates, you can run:
```bash
bash ~/irium/scripts/auto-update.sh
```

Or set up a cron job to check daily:
```bash
# Add to crontab: crontab -e
0 2 * * * cd /home/irium/irium && bash scripts/auto-update.sh >> /tmp/irium-autoupdate.log 2>&1
```

## How It Works

1. **Update Check**: Node checks GitHub every 6 hours
2. **Notification**: Displays alert if newer version available
3. **Instructions**: Shows exact commands to update
4. **Safe**: Never forces updates, you stay in control

## Security

- Updates are pulled from official GitHub repository
- Automatic backup before applying updates
- Rollback on failure
- You can disable by removing the checker code

