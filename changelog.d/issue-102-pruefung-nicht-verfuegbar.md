### Security
- If the optional AI second opinion or the AI check for injected instructions
  fails (e.g. expired key, rate limit, provider unreachable), this is now
  shown instead of being silently skipped. The affected action, or the next
  suggested action, then requires confirmation. If the second opinion is
  enabled but cannot be set up on connect (provider deleted, credentials
  missing), the session shows a notice.
