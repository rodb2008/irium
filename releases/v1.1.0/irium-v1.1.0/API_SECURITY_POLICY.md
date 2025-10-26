# Irium API Security Policy

## CORS (Cross-Origin Resource Sharing)

### Current Status
- **Explorer API**: CORS enabled with `Access-Control-Allow-Origin: *`
- **Wallet API**: CORS enabled for read operations

### Justification
CORS is intentionally open for public blockchain explorers to allow:
- Web-based block explorers
- Third-party integrations
- Developer tools
- Mobile apps

### Security Measures
1. **Rate Limiting**: 120 requests/minute per IP
2. **Read-Only**: No write operations exposed
3. **No Authentication Required**: Public blockchain data

### Recommendation for Private Deployments
If running a private node, restrict CORS:

```python
# In API files, change:
self.send_header('Access-Control-Allow-Origin', '*')
# To:
self.send_header('Access-Control-Allow-Origin', 'https://your-domain.com')
```

## Rate Limiting

- **Default**: 120 requests per minute per IP
- **Method**: Token bucket algorithm
- **Response**: HTTP 429 when exceeded
- **Retry-After**: 60 seconds

## Authentication

- **Current**: Not required (read-only public data)
- **Future**: Optional API keys for premium features
- **Wallet API**: Local access recommended (bind to 127.0.0.1)

## DDoS Protection

Additional measures for production:
1. Use reverse proxy (nginx/cloudflare)
2. IP-based blocking for abusive clients
3. Firewall rules for P2P port
4. Monitor and alert on unusual traffic

